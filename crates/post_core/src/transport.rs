use crate::{PostError, PostMessage, Result};
use async_trait::async_trait;
use serde_json;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send_message(&self, message: PostMessage) -> Result<()>;
    async fn start_listening(&self, sender: mpsc::UnboundedSender<PostMessage>) -> Result<()>;
    async fn get_node_id(&self) -> Result<String>;
    async fn get_tailnet_nodes(&self) -> Result<Vec<String>>;
}

pub struct TailscaleTransport {
    tailscale_bin: String,
    port: u16,
}

impl TailscaleTransport {
    pub fn new(port: u16) -> Self {
        Self {
            tailscale_bin: "tailscale".to_string(),
            port,
        }
    }

    pub fn with_binary_path(mut self, path: String) -> Self {
        self.tailscale_bin = path;
        self
    }

    async fn run_tailscale_command(&self, args: &[&str]) -> Result<String> {
        let output = Command::new(&self.tailscale_bin)
            .args(args)
            .output()
            .await
            .map_err(|e| PostError::Tailscale(format!("Failed to run tailscale command: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PostError::Tailscale(format!(
                "Tailscale command failed: {}",
                stderr
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    async fn send_to_node(&self, node_ip: &str, message: &PostMessage) -> Result<()> {
        let serialized = serde_json::to_string(message)
            .map_err(|e| PostError::Serialization(format!("Failed to serialize message: {}", e)))?;

        debug!("Sending message to {}: {} bytes", node_ip, serialized.len());

        let mut cmd = Command::new("nc")
            .args(&["-q", "1", node_ip, &self.port.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| PostError::Network(format!("Failed to start nc: {}", e)))?;

        if let Some(stdin) = cmd.stdin.as_mut() {
            stdin
                .write_all(serialized.as_bytes())
                .await
                .map_err(|e| PostError::Network(format!("Failed to write to nc: {}", e)))?;
            stdin
                .write_all(b"\n")
                .await
                .map_err(|e| PostError::Network(format!("Failed to write newline to nc: {}", e)))?;
        }

        let status = cmd
            .wait()
            .await
            .map_err(|e| PostError::Network(format!("Failed to wait for nc: {}", e)))?;

        if !status.success() {
            return Err(PostError::Network(format!(
                "nc command failed with status: {}",
                status
            )));
        }

        Ok(())
    }
}

#[async_trait]
impl Transport for TailscaleTransport {
    async fn send_message(&self, message: PostMessage) -> Result<()> {
        let nodes = self.get_tailnet_nodes().await?;
        let mut errors = vec![];

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

        let mut cmd = Command::new("nc")
            .args(&["-l", "-k", &self.port.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| PostError::Network(format!("Failed to start nc listener: {}", e)))?;

        let stdout = cmd
            .stdout
            .take()
            .ok_or_else(|| PostError::Network("Failed to get stdout from nc".to_string()))?;

        let mut reader = BufReader::new(stdout);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    debug!("nc listener EOF, restarting...");
                    break;
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    match serde_json::from_str::<PostMessage>(trimmed) {
                        Ok(message) => {
                            debug!("Received message: {:?}", message.message_type);
                            if let Err(e) = sender.send(message) {
                                error!("Failed to forward message: {}", e);
                                break;
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse message: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to read from nc: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }

    async fn get_node_id(&self) -> Result<String> {
        let status_output = self.run_tailscale_command(&["status", "--json"]).await?;

        let status: serde_json::Value = serde_json::from_str(&status_output)
            .map_err(|e| PostError::Tailscale(format!("Failed to parse status JSON: {}", e)))?;

        let node_id = status
            .get("Self")
            .and_then(|self_info| self_info.get("ID"))
            .and_then(|id| id.as_str())
            .ok_or_else(|| PostError::Tailscale("Failed to get node ID from status".to_string()))?;

        debug!("Got Tailscale node ID: {}", node_id);
        Ok(node_id.to_string())
    }

    async fn get_tailnet_nodes(&self) -> Result<Vec<String>> {
        let status_output = self.run_tailscale_command(&["status", "--json"]).await?;

        let status: serde_json::Value = serde_json::from_str(&status_output)
            .map_err(|e| PostError::Tailscale(format!("Failed to parse status JSON: {}", e)))?;

        let mut nodes = Vec::new();

        if let Some(peers) = status.get("Peer").and_then(|p| p.as_object()) {
            for (_, peer_info) in peers {
                if let Some(tailscale_ips) =
                    peer_info.get("TailscaleIPs").and_then(|ips| ips.as_array())
                {
                    if let Some(ip) = tailscale_ips.first().and_then(|ip| ip.as_str()) {
                        if peer_info
                            .get("Online")
                            .and_then(|o| o.as_bool())
                            .unwrap_or(false)
                        {
                            nodes.push(ip.to_string());
                        }
                    }
                }
            }
        }

        debug!("Found {} online Tailscale nodes", nodes.len());
        Ok(nodes)
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
}
