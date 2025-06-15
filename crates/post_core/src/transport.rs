use crate::{PostError, PostMessage, Result};
use async_trait::async_trait;
use serde_json;
use std::net::SocketAddr;
use std::path::Path;
use tailscale_localapi::{LocalApi, UnixStreamClient};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send_message(&self, message: PostMessage) -> Result<()>;
    async fn start_listening(&self, sender: mpsc::UnboundedSender<PostMessage>) -> Result<()>;
    async fn get_node_id(&self) -> Result<String>;
    async fn get_tailnet_nodes(&self) -> Result<Vec<String>>;
    async fn is_connected(&self) -> Result<bool>;
}

pub struct TailscaleTransport {
    local_api: LocalApi<UnixStreamClient>,
    port: u16,
    socket_path: String,
}

impl TailscaleTransport {
    pub fn new(port: u16) -> Self {
        let socket_path = Self::detect_tailscale_socket_path();
        debug!("Using Tailscale socket path: {}", socket_path);
        Self {
            local_api: LocalApi::<UnixStreamClient>::new_with_socket_path(&socket_path),
            port,
            socket_path: socket_path.clone(),
        }
    }

    pub async fn new_with_detection(port: u16) -> Result<Self> {
        let socket_paths = Self::get_possible_socket_paths();

        for socket_path in socket_paths {
            debug!("Trying Tailscale socket path: {}", socket_path);

            // Check if socket exists and is accessible
            if Self::is_socket_accessible(&socket_path).await {
                let transport = Self {
                    local_api: LocalApi::<UnixStreamClient>::new_with_socket_path(&socket_path),
                    port,
                    socket_path: socket_path.clone(),
                };

                // Test if we can actually connect and get status
                if transport.test_connection().await.is_ok() {
                    info!("Successfully connected to Tailscale at: {}", socket_path);
                    return Ok(transport);
                }
            }
        }

        Err(PostError::Tailscale(
            "Could not connect to Tailscale daemon. Please ensure Tailscale is installed and running.".to_string()
        ))
    }

    fn detect_tailscale_socket_path() -> String {
        // Check if running in container first
        if Self::is_running_in_container() {
            return "/tmp/tailscaled.sock".to_string();
        }

        #[cfg(target_os = "windows")]
        {
            r"\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled-pipe".to_string()
        }
        #[cfg(target_os = "macos")]
        {
            // Try Unix socket first, will fallback to TCP in connection logic
            "/var/run/tailscaled.socket".to_string()
        }
        #[cfg(target_os = "linux")]
        {
            "/var/run/tailscaled.socket".to_string()
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            "/var/run/tailscaled.socket".to_string()
        }
    }

    fn is_running_in_container() -> bool {
        // Check for common container indicators
        std::path::Path::new("/.dockerenv").exists()
            || std::env::var("KUBERNETES_SERVICE_HOST").is_ok()
            || std::env::var("container").is_ok()
    }

    pub fn get_possible_socket_paths() -> Vec<String> {
        let mut paths = Vec::new();

        // Check container first
        if Self::is_running_in_container() {
            paths.push("/tmp/tailscaled.sock".to_string());
            return paths;
        }

        #[cfg(target_os = "linux")]
        {
            paths.push("/var/run/tailscaled.socket".to_string());
            // Fallback to old path for compatibility
            paths.push("/var/run/tailscale/tailscaled.sock".to_string());
        }

        #[cfg(target_os = "macos")]
        {
            // Try Unix socket first (open source version)
            paths.push("/var/run/tailscaled.socket".to_string());
            // Legacy path
            paths.push("/var/run/tailscale/tailscaled.sock".to_string());

            // Check for App Store version TCP port
            if let Some(tcp_port) = Self::detect_macos_tcp_port() {
                info!(
                    "Detected macOS App Store Tailscale version on port {}",
                    tcp_port
                );
                // TODO: Implement TCP connection support
                // For now, we'll note this but can't use it with current tailscale-localapi crate
                // paths.push(format!("[::1]:{}", tcp_port));
            }
        }

        #[cfg(target_os = "windows")]
        {
            paths.push(
                r"\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled-pipe".to_string(),
            );
            // Fallback to old path
            paths.push(r"\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled".to_string());
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            paths.push("/var/run/tailscaled.socket".to_string());
            paths.push("/var/run/tailscale/tailscaled.sock".to_string());
        }

        paths
    }

    #[cfg(target_os = "macos")]
    fn detect_macos_tcp_port() -> Option<u16> {
        // Check for App Store version TCP port file
        let port_file_path = "/Library/Tailscale/ipnport";

        if let Ok(contents) = std::fs::read_to_string(port_file_path) {
            if let Ok(port) = contents.trim().parse::<u16>() {
                debug!("Found macOS Tailscale TCP port: {}", port);
                return Some(port);
            }
        }

        // Check if the symlink exists but couldn't read it
        if Path::new(port_file_path).exists() {
            warn!("Found {} but couldn't read port number", port_file_path);
        }

        None
    }

    pub fn get_socket_path(&self) -> &str {
        &self.socket_path
    }

    async fn is_socket_accessible(socket_path: &str) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::net::UnixStream;
            Path::new(socket_path).exists() && UnixStream::connect(socket_path).is_ok()
        }

        #[cfg(windows)]
        {
            // For Windows named pipes, we need to try to connect
            // This is a simplified check - in reality we'd use Windows APIs
            Path::new(socket_path).exists()
        }

        #[cfg(not(any(unix, windows)))]
        {
            Path::new(socket_path).exists()
        }
    }

    async fn test_connection(&self) -> Result<()> {
        // Try to get status to verify connection works
        match self.local_api.status().await {
            Ok(_) => {
                debug!("Tailscale connection test successful");
                Ok(())
            }
            Err(e) => {
                debug!("Tailscale connection test failed: {}", e);
                Err(PostError::Tailscale(format!(
                    "Connection test failed: {}",
                    e
                )))
            }
        }
    }

    pub async fn is_tailscale_connected(&self) -> Result<bool> {
        match self.local_api.status().await {
            Ok(status) => {
                // Check if Tailscale is in a connected state
                use tailscale_localapi::BackendState;
                match status.backend_state {
                    BackendState::Running => Ok(true),
                    BackendState::Stopped
                    | BackendState::NoState
                    | BackendState::NeedsLogin
                    | BackendState::NeedsMachineAuth => Ok(false),
                    _ => {
                        debug!(
                            "Unknown Tailscale backend state: {:?}",
                            status.backend_state
                        );
                        Ok(false)
                    }
                }
            }
            Err(e) => {
                debug!("Failed to get Tailscale status: {}", e);
                Ok(false)
            }
        }
    }

    async fn send_to_node(&self, node_ip: &str, message: &PostMessage) -> Result<()> {
        let serialized = serde_json::to_string(message)
            .map_err(|e| PostError::Serialization(format!("Failed to serialize message: {}", e)))?;

        debug!("Sending message to {}: {} bytes", node_ip, serialized.len());

        let addr = format!("{}:{}", node_ip, self.port);
        let mut stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| PostError::Network(format!("Failed to connect to {}: {}", addr, e)))?;

        stream
            .write_all(serialized.as_bytes())
            .await
            .map_err(|e| PostError::Network(format!("Failed to write message: {}", e)))?;

        stream
            .write_all(b"\n")
            .await
            .map_err(|e| PostError::Network(format!("Failed to write newline: {}", e)))?;

        stream
            .shutdown()
            .await
            .map_err(|e| PostError::Network(format!("Failed to shutdown connection: {}", e)))?;

        Ok(())
    }
}

#[async_trait]
impl Transport for TailscaleTransport {
    async fn send_message(&self, message: PostMessage) -> Result<()> {
        if !self.is_tailscale_connected().await? {
            return Err(PostError::Tailscale(
                "Cannot send message: Tailscale not connected".to_string(),
            ));
        }

        let nodes = self.get_tailnet_nodes().await?;
        let mut errors = vec![];

        if nodes.is_empty() {
            debug!("No online Tailscale nodes found to send message to");
            return Ok(());
        }

        for node in &nodes {
            if let Err(e) = self.send_to_node(node, &message).await {
                warn!("Failed to send message to {}: {}", node, e);
                errors.push(e);
            }
        }

        if !errors.is_empty() && errors.len() == nodes.len() {
            return Err(PostError::Network(
                "Failed to send message to any nodes".to_string(),
            ));
        }

        debug!("Message sent to {} nodes", nodes.len() - errors.len());
        Ok(())
    }

    async fn start_listening(&self, sender: mpsc::UnboundedSender<PostMessage>) -> Result<()> {
        info!("Starting TCP listener on port {}", self.port);

        let addr = SocketAddr::from(([0, 0, 0, 0], self.port));
        let listener = TcpListener::bind(addr).await.map_err(|e| {
            PostError::Network(format!("Failed to bind to port {}: {}", self.port, e))
        })?;

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    debug!("Accepted connection from {}", addr);
                    let sender = sender.clone();

                    tokio::spawn(async move {
                        let mut buffer = Vec::new();
                        let mut temp_buf = [0u8; 1024];

                        loop {
                            match stream.try_read(&mut temp_buf) {
                                Ok(0) => break, // EOF
                                Ok(n) => {
                                    buffer.extend_from_slice(&temp_buf[..n]);

                                    // Look for complete messages (newline-delimited)
                                    while let Some(newline_pos) =
                                        buffer.iter().position(|&b| b == b'\n')
                                    {
                                        let message_bytes =
                                            buffer.drain(..newline_pos + 1).collect::<Vec<u8>>();
                                        let message_str = String::from_utf8_lossy(&message_bytes);
                                        let trimmed = message_str.trim();

                                        if !trimmed.is_empty() {
                                            match serde_json::from_str::<PostMessage>(trimmed) {
                                                Ok(message) => {
                                                    debug!(
                                                        "Received message: {:?}",
                                                        message.message_type
                                                    );
                                                    if let Err(e) = sender.send(message) {
                                                        error!("Failed to forward message: {}", e);
                                                        return;
                                                    }
                                                }
                                                Err(e) => {
                                                    warn!("Failed to parse message: {}", e);
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                    // No data available right now, yield and try again
                                    tokio::task::yield_now().await;
                                }
                                Err(e) => {
                                    debug!("Connection error: {}", e);
                                    break;
                                }
                            }
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    async fn get_node_id(&self) -> Result<String> {
        if !self.is_tailscale_connected().await? {
            return Err(PostError::Tailscale(
                "Tailscale not connected or running".to_string(),
            ));
        }

        let status = self
            .local_api
            .status()
            .await
            .map_err(|e| PostError::Tailscale(format!("Failed to get status: {}", e)))?;

        let node_id = status.self_status.id.clone();
        debug!("Got Tailscale node ID: {}", node_id);
        Ok(node_id)
    }

    async fn get_tailnet_nodes(&self) -> Result<Vec<String>> {
        if !self.is_tailscale_connected().await? {
            return Err(PostError::Tailscale(
                "Tailscale not connected or running".to_string(),
            ));
        }

        let status = self
            .local_api
            .status()
            .await
            .map_err(|e| PostError::Tailscale(format!("Failed to get status: {}", e)))?;

        let mut nodes = Vec::new();

        for (_, peer) in status.peer {
            if peer.online && !peer.tailscale_ips.is_empty() {
                // Use the first Tailscale IP
                nodes.push(peer.tailscale_ips[0].to_string());
            }
        }

        debug!("Found {} online Tailscale nodes", nodes.len());
        Ok(nodes)
    }

    async fn is_connected(&self) -> Result<bool> {
        self.is_tailscale_connected().await
    }
}

pub struct MockTransport {
    node_id: String,
}

impl MockTransport {
    pub fn new(node_id: String) -> Self {
        Self { node_id }
    }
}

#[async_trait]
impl Transport for MockTransport {
    async fn send_message(&self, message: PostMessage) -> Result<()> {
        debug!(
            "Mock transport: would send message {:?}",
            message.message_type
        );
        Ok(())
    }

    async fn start_listening(&self, _sender: mpsc::UnboundedSender<PostMessage>) -> Result<()> {
        debug!("Mock transport: listening (no-op)");
        tokio::time::sleep(std::time::Duration::from_secs(u64::MAX)).await;
        Ok(())
    }

    async fn get_node_id(&self) -> Result<String> {
        Ok(self.node_id.clone())
    }

    async fn get_tailnet_nodes(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn is_connected(&self) -> Result<bool> {
        Ok(true) // Mock transport is always "connected"
    }
}
