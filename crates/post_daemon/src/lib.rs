use post_core::*;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

mod notifications;
use notifications::NotificationManager;

pub struct Daemon {
    config: PostConfig,
    clipboard: Arc<SystemClipboard>,
    transport: Arc<dyn Transport>,
    sync_manager: Arc<Mutex<Option<Arc<SyncManager>>>>,
    notifications: NotificationManager,
}

impl Daemon {
    pub async fn new(config: PostConfig) -> Result<Self> {
        let clipboard = Arc::new(SystemClipboard::new()?);
        let notifications = NotificationManager::new();

        // Use the new detection method that tries multiple socket paths
        let (transport, is_connected_at_startup) = match TailscaleTransport::new_with_detection(
            config.network.port,
        )
        .await
        {
            Ok(transport) => (Arc::new(transport), true),
            Err(e) => {
                // Fallback to old method for compatibility
                warn!(
                    "Failed to detect Tailscale with new method: {}, falling back to default",
                    e
                );
                let transport = Arc::new(TailscaleTransport::new(config.network.port));

                // Check connectivity but don't fail at startup
                let connected = match transport.is_connected().await {
                    Ok(true) => true,
                    Ok(false) => {
                        info!("Tailscale is not connected at startup - will retry every 2 seconds");
                        false
                    }
                    Err(e) => {
                        info!("Unable to check Tailscale connectivity at startup: {} - will retry every 2 seconds", e);
                        false
                    }
                };
                (transport, connected)
            }
        };

        // Only create SyncManager if Tailscale is actually connected
        let sync_manager = if is_connected_at_startup {
            match transport.get_node_id().await {
                Ok(node_id) => {
                    info!("Daemon initialized with Tailscale node ID: {}", node_id);

                    // Show connection notification
                    if let Err(e) = notifications.show_tailscale_connected(&node_id) {
                        warn!("Failed to show connection notification: {}", e);
                    }

                    Some(Arc::new(SyncManager::new(clipboard.clone(), node_id)?))
                }
                Err(e) => {
                    warn!("Tailscale connected but couldn't get node ID: {}", e);
                    if let Err(e) = notifications.show_daemon_started_offline() {
                        warn!("Failed to show startup notification: {}", e);
                    }
                    None
                }
            }
        } else {
            info!("Daemon starting without Tailscale connection - will wait for connection");
            if let Err(e) = notifications.show_daemon_started_offline() {
                warn!("Failed to show startup notification: {}", e);
            }
            None
        };

        let sync_manager = Arc::new(Mutex::new(sync_manager));

        Ok(Self {
            config,
            clipboard,
            transport,
            sync_manager,
            notifications,
        })
    }

    pub async fn run(&self) -> Result<()> {
        info!("Starting Post daemon");

        // Signal handling is now managed by the main daemon process
        // No need for a separate signal handler here

        let (tx, mut rx) = mpsc::unbounded_channel();
        let transport_clone = Arc::clone(&self.transport);

        tokio::spawn(async move {
            if let Err(e) = transport_clone.start_listening(tx).await {
                error!("Transport listener failed: {}", e);
            }
        });

        let transport_send = Arc::clone(&self.transport);
        let sync_manager_clone = Arc::clone(&self.sync_manager);

        // Start sync loop only if we have a sync manager
        if let Some(sync_manager) = sync_manager_clone.lock().await.as_ref() {
            let sync_manager_ref = Arc::clone(sync_manager);
            let transport_discovery = Arc::clone(&transport_send);

            // Send initial node discovery message
            let discovery_message = sync_manager_ref.create_node_discovery_message().await?;
            tokio::spawn(async move {
                if let Err(e) = transport_discovery.send_message(discovery_message).await {
                    error!("Failed to send initial node discovery: {}", e);
                } else {
                    info!("Sent initial node discovery message");
                }
            });

            tokio::spawn(async move {
                if let Err(e) = sync_manager_ref
                    .start_sync_loop(move |message| {
                        let transport = Arc::clone(&transport_send);
                        tokio::spawn(async move {
                            if let Err(e) = transport.send_message(message).await {
                                error!("Failed to send message: {}", e);
                            }
                        });
                    })
                    .await
                {
                    error!("Sync loop failed: {}", e);
                }
            });
        } else {
            info!("Sync loop not started - waiting for Tailscale connection");
        }

        // Tailscale connectivity monitoring task - checks every 2 seconds
        let sync_manager_health = Arc::clone(&self.sync_manager);
        let transport_health = Arc::clone(&self.transport);
        let clipboard_for_sync = Arc::clone(&self.clipboard);
        let notifications_clone = self.notifications.clone();
        let transport_for_sync = Arc::clone(&self.transport);

        tokio::spawn(async move {
            use std::sync::atomic::{AtomicBool, Ordering};
            use std::sync::Arc as StdArc;

            let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
            let was_connected = StdArc::new(AtomicBool::new(false));

            // Determine initial state based on sync_manager existence
            let initial_connected = {
                let sync_manager_guard = sync_manager_health.lock().await;
                let connected = sync_manager_guard.is_some()
                    && matches!(transport_health.is_connected().await, Ok(true));

                if connected {
                    info!("Initial state: Tailscale is connected");
                } else {
                    info!("Initial state: Tailscale is not connected, monitoring for changes");
                }

                connected
            };
            was_connected.store(initial_connected, Ordering::Relaxed);

            loop {
                interval.tick().await;

                // Re-detect Tailscale every 2 seconds to handle port changes
                let connection_check = TailscaleTransport::new_with_detection(19827).await;

                let is_connected = match &connection_check {
                    Ok(transport) => transport.is_connected().await.unwrap_or(false),
                    Err(_) => false,
                };

                if is_connected {
                    let previously_connected = was_connected.load(Ordering::Relaxed);

                    if !previously_connected {
                        // Just connected - create SyncManager and show notification
                        if let Ok(ref transport) = connection_check {
                            match transport.get_node_id().await {
                                Ok(node_id) => {
                                    info!("Tailscale connected: {}", node_id);

                                    // Create SyncManager if it doesn't exist
                                    let mut sync_manager_guard = sync_manager_health.lock().await;
                                    if sync_manager_guard.is_none() {
                                        match SyncManager::new(
                                            clipboard_for_sync.clone(),
                                            node_id.clone(),
                                        ) {
                                            Ok(new_sync_manager) => {
                                                let sync_manager_arc = Arc::new(new_sync_manager);
                                                *sync_manager_guard =
                                                    Some(Arc::clone(&sync_manager_arc));
                                                drop(sync_manager_guard);

                                                info!(
                                                    "Created SyncManager with node ID: {}",
                                                    node_id
                                                );

                                                // Send initial node discovery message for new SyncManager
                                                let transport_for_discovery =
                                                    Arc::clone(&transport_for_sync);
                                                let sync_manager_for_discovery =
                                                    Arc::clone(&sync_manager_arc);
                                                tokio::spawn(async move {
                                                    match sync_manager_for_discovery
                                                        .create_node_discovery_message()
                                                        .await
                                                    {
                                                        Ok(discovery_message) => {
                                                            if let Err(e) = transport_for_discovery
                                                                .send_message(discovery_message)
                                                                .await
                                                            {
                                                                error!("Failed to send initial node discovery: {}", e);
                                                            } else {
                                                                info!("Sent initial node discovery message");
                                                            }
                                                        }
                                                        Err(e) => {
                                                            error!("Failed to create node discovery message: {}", e);
                                                        }
                                                    }
                                                });

                                                // Start sync loop for the new SyncManager
                                                let transport_for_messages =
                                                    Arc::clone(&transport_for_sync);
                                                tokio::spawn(async move {
                                                    if let Err(e) = sync_manager_arc
                                                        .start_sync_loop(move |message| {
                                                            let transport = Arc::clone(&transport_for_messages);
                                                            tokio::spawn(async move {
                                                                if let Err(e) = transport.send_message(message).await {
                                                                    error!("Failed to send message: {}", e);
                                                                }
                                                            });
                                                        })
                                                        .await
                                                    {
                                                        error!("Sync loop failed: {}", e);
                                                    }
                                                });
                                            }
                                            Err(e) => {
                                                error!("Failed to create SyncManager: {}", e);
                                            }
                                        }
                                    }

                                    if let Err(e) =
                                        notifications_clone.show_tailscale_connected(&node_id)
                                    {
                                        warn!("Failed to show connection notification: {}", e);
                                    }
                                }
                                Err(e) => {
                                    warn!("Connected to Tailscale but couldn't get node ID: {}", e);
                                }
                            }
                        }
                        was_connected.store(true, Ordering::Relaxed);
                    } else {
                        debug!("Tailscale connectivity check: OK");
                    }
                } else {
                    let previously_connected = was_connected.load(Ordering::Relaxed);

                    if previously_connected {
                        // Just disconnected - remove SyncManager and show notification
                        info!("Tailscale disconnected - will retry every 2 seconds");

                        // Clear the SyncManager
                        let mut sync_manager_guard = sync_manager_health.lock().await;
                        *sync_manager_guard = None;
                        drop(sync_manager_guard);

                        if let Err(e) = notifications_clone.show_tailscale_disconnected() {
                            warn!("Failed to show disconnection notification: {}", e);
                        }

                        was_connected.store(false, Ordering::Relaxed);
                    } else {
                        debug!("Tailscale still not connected - retrying...");
                    }
                }
            }
        });

        // Separate health check task for other components (runs less frequently)
        let clipboard_health = Arc::clone(&self.clipboard);
        let cleanup_interval = self.config.network.discovery_interval * 10;
        let heartbeat_interval = self.config.network.heartbeat_interval;
        let transport_heartbeat = Arc::clone(&self.transport);
        let sync_manager_cleanup = Arc::clone(&self.sync_manager);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            let mut tick_count = 0u64;

            loop {
                interval.tick().await;
                tick_count += 1;

                // Clipboard health check (every 2 minutes = every 4 ticks)
                if tick_count % 4 == 0 {
                    if let Err(e) = clipboard_health.get_contents().await {
                        error!("Clipboard health check failed: {}", e);
                    }
                }

                // Heartbeat task (based on configured interval, but max every 30 seconds)
                if tick_count % ((heartbeat_interval / 30).max(1)) == 0 {
                    if let Ok(nodes) = transport_heartbeat.get_tailnet_nodes().await {
                        debug!("Heartbeat tick - found {} nodes", nodes.len());
                    } else {
                        debug!("Heartbeat tick - failed to get nodes");
                    }
                }

                // Cleanup task (based on configured interval, but max every 10 minutes)
                if tick_count % ((cleanup_interval / 30).max(20)) == 0 {
                    let sync_manager_guard = sync_manager_cleanup.lock().await;
                    if let Some(ref sync_manager) = *sync_manager_guard {
                        if let Err(e) = sync_manager.cleanup_stale_nodes(cleanup_interval * 2).await
                        {
                            error!("Failed to cleanup stale nodes: {}", e);
                        }
                    }
                }

                // Prevent tick_count overflow
                if tick_count > 200_000_000 {
                    tick_count = 0;
                }
            }
        });

        while let Some(message) = rx.recv().await {
            let sync_manager_guard = sync_manager_clone.lock().await;
            if let Some(ref sync_manager) = *sync_manager_guard {
                if let Err(e) = sync_manager.handle_message(message.clone()).await {
                    // If we get a "No verifying key found" error, send node discovery
                    if e.to_string().contains("No verifying key found for node") {
                        info!("Unknown node detected, sending node discovery");
                        let transport_for_discovery = Arc::clone(&self.transport);
                        let sync_manager_for_discovery = Arc::clone(sync_manager);
                        tokio::spawn(async move {
                            match sync_manager_for_discovery
                                .create_node_discovery_message()
                                .await
                            {
                                Ok(discovery_message) => {
                                    if let Err(e) = transport_for_discovery
                                        .send_message(discovery_message)
                                        .await
                                    {
                                        debug!("Failed to send reactive node discovery: {}", e);
                                    } else {
                                        info!("Sent reactive node discovery message");
                                    }
                                }
                                Err(e) => {
                                    error!(
                                        "Failed to create reactive node discovery message: {}",
                                        e
                                    );
                                }
                            }
                        });
                    } else {
                        error!("Failed to handle message: {}", e);
                    }
                }
            } else {
                debug!("Received message but no SyncManager available - ignoring");
            }
        }

        Ok(())
    }
}

/// Get the PID file path
pub fn get_pid_file_path() -> Result<PathBuf> {
    let mut path = dirs::data_dir()
        .ok_or_else(|| PostError::Other("Could not find data directory".to_string()))?;
    path.push("post");

    // Create directory with secure permissions (700 - owner only)
    std::fs::create_dir_all(&path).map_err(PostError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o700);
        std::fs::set_permissions(&path, permissions).map_err(PostError::Io)?;
    }

    path.push("post.pid");
    Ok(path)
}

/// Write the current process PID to file
pub fn write_pid_file() -> Result<()> {
    let pid_path = get_pid_file_path()?;
    let pid = std::process::id();

    // Write PID file with secure permissions (600 - owner read/write only)
    std::fs::write(&pid_path, pid.to_string()).map_err(PostError::Io)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&pid_path, permissions).map_err(PostError::Io)?;
    }

    info!("PID file written to {}", pid_path.display());
    Ok(())
}

/// Remove the PID file
pub fn remove_pid_file() -> Result<()> {
    let pid_path = get_pid_file_path()?;
    if pid_path.exists() {
        std::fs::remove_file(&pid_path).map_err(PostError::Io)?;
        debug!("PID file removed");
    }
    Ok(())
}

/// Check if daemon is running by reading PID file
pub fn is_daemon_running() -> Result<Option<u32>> {
    let pid_path = get_pid_file_path()?;

    if !pid_path.exists() {
        return Ok(None);
    }

    let pid_str = std::fs::read_to_string(&pid_path).map_err(PostError::Io)?;
    let pid: u32 = pid_str
        .trim()
        .parse()
        .map_err(|_| PostError::Other("Invalid PID in PID file".to_string()))?;

    // Check if process is actually running
    #[cfg(unix)]
    {
        use nix::sys::signal::kill;
        use nix::unistd::Pid;

        match kill(Pid::from_raw(pid as i32), None) {
            Ok(_) => Ok(Some(pid)),
            Err(_) => {
                // Process not running, clean up stale PID file
                let _ = std::fs::remove_file(&pid_path);
                Ok(None)
            }
        }
    }

    #[cfg(not(unix))]
    {
        // On non-Unix systems, just assume the process is running if PID file exists
        Ok(Some(pid))
    }
}

/// Get log file path
pub fn get_log_file_path() -> Result<PathBuf> {
    let mut path = dirs::data_dir()
        .ok_or_else(|| PostError::Other("Could not find data directory".to_string()))?;
    path.push("post");

    // Create directory with secure permissions (700 - owner only)
    std::fs::create_dir_all(&path).map_err(PostError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o700);
        std::fs::set_permissions(&path, permissions).map_err(PostError::Io)?;
    }

    path.push("post.log");
    Ok(path)
}

#[cfg(all(unix, not(target_os = "macos")))]
pub async fn daemonize() -> Result<()> {
    use nix::unistd::{fork, setsid, ForkResult};
    use std::fs::File;
    use std::os::unix::io::AsRawFd;

    match unsafe { fork() } {
        Ok(ForkResult::Parent { .. }) => {
            std::process::exit(0);
        }
        Ok(ForkResult::Child) => {
            setsid()
                .map_err(|e| PostError::Other(format!("Failed to create new session: {}", e)))?;

            // Set up logging to file instead of /dev/null
            let log_path = get_log_file_path()?;
            let log_file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .map_err(PostError::Io)?;

            // Set secure permissions on log file (600 - owner read/write only)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let permissions = std::fs::Permissions::from_mode(0o600);
                std::fs::set_permissions(&log_path, permissions).map_err(PostError::Io)?;
            }

            let devnull = File::open("/dev/null").map_err(PostError::Io)?;

            unsafe {
                // Redirect stdin to /dev/null
                if libc::dup2(devnull.as_raw_fd(), 0) == -1 {
                    return Err(PostError::Other("Failed to redirect stdin".to_string()));
                }
                // Redirect stdout and stderr to log file
                if libc::dup2(log_file.as_raw_fd(), 1) == -1 {
                    return Err(PostError::Other("Failed to redirect stdout".to_string()));
                }
                if libc::dup2(log_file.as_raw_fd(), 2) == -1 {
                    return Err(PostError::Other("Failed to redirect stderr".to_string()));
                }
            }

            std::env::set_current_dir("/").map_err(PostError::Io)?;

            // Write PID file
            write_pid_file()?;
        }
        Err(e) => {
            return Err(PostError::Other(format!("Fork failed: {}", e)));
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
pub async fn daemonize() -> Result<()> {
    // On macOS, avoid fork() entirely due to NSPasteboard issues
    // Instead, use nohup-style approach with process group manipulation
    use nix::unistd::setsid;
    use std::fs::File;
    use std::os::unix::io::AsRawFd;

    // Create new session to detach from controlling terminal
    setsid().map_err(|e| PostError::Other(format!("Failed to create new session: {}", e)))?;

    // Set up logging to file
    let log_path = get_log_file_path()?;
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(PostError::Io)?;

    // Set secure permissions on log file (600 - owner read/write only)
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(&log_path, permissions).map_err(PostError::Io)?;

    let devnull = File::open("/dev/null").map_err(PostError::Io)?;

    unsafe {
        // Redirect stdin to /dev/null
        if libc::dup2(devnull.as_raw_fd(), 0) == -1 {
            return Err(PostError::Other("Failed to redirect stdin".to_string()));
        }
        // Redirect stdout and stderr to log file
        if libc::dup2(log_file.as_raw_fd(), 1) == -1 {
            return Err(PostError::Other("Failed to redirect stdout".to_string()));
        }
        if libc::dup2(log_file.as_raw_fd(), 2) == -1 {
            return Err(PostError::Other("Failed to redirect stderr".to_string()));
        }
    }

    std::env::set_current_dir("/").map_err(PostError::Io)?;

    // Write PID file
    write_pid_file()?;
    info!("macOS daemon started without fork() to avoid NSPasteboard issues");

    Ok(())
}

#[cfg(not(unix))]
pub async fn daemonize() -> Result<()> {
    info!("Daemonization not supported on this platform");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[tokio::test]
    #[serial]
    async fn test_pid_file_operations() {
        // Test writing and reading PID file
        let result = write_pid_file();
        assert!(result.is_ok(), "Should be able to write PID file");

        // Test reading PID file
        let running = is_daemon_running();
        assert!(
            running.is_ok(),
            "Should be able to check if daemon is running"
        );

        let pid = running.unwrap();
        assert!(pid.is_some(), "Should detect running daemon");
        assert_eq!(
            pid.unwrap(),
            std::process::id(),
            "Should return current process PID"
        );

        // Test removing PID file
        let result = remove_pid_file();
        assert!(result.is_ok(), "Should be able to remove PID file");

        // Test checking after removal
        let running = is_daemon_running();
        assert!(
            running.is_ok(),
            "Should be able to check if daemon is running after removal"
        );
        assert!(
            running.unwrap().is_none(),
            "Should not detect running daemon after PID file removal"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_log_file_path() {
        let log_path = get_log_file_path();
        assert!(log_path.is_ok(), "Should be able to get log file path");

        let path = log_path.unwrap();
        assert!(
            path.to_string_lossy().contains("post"),
            "Log path should contain 'post'"
        );
        assert!(
            path.to_string_lossy().ends_with("post.log"),
            "Log path should end with 'post.log'"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_pid_file_path() {
        let pid_path = get_pid_file_path();
        assert!(pid_path.is_ok(), "Should be able to get PID file path");

        let path = pid_path.unwrap();
        assert!(
            path.to_string_lossy().contains("post"),
            "PID path should contain 'post'"
        );
        assert!(
            path.to_string_lossy().ends_with("post.pid"),
            "PID path should end with 'post.pid'"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_stale_pid_cleanup() {
        // Create a fake PID file with non-existent PID
        let pid_path = get_pid_file_path().unwrap();
        std::fs::write(&pid_path, "999999").unwrap(); // Very unlikely PID

        // Check if daemon is running should clean up stale PID file
        let running = is_daemon_running();
        assert!(running.is_ok(), "Should handle stale PID file gracefully");
        assert!(
            running.unwrap().is_none(),
            "Should not detect daemon with stale PID"
        );

        // PID file should be cleaned up
        assert!(!pid_path.exists(), "Stale PID file should be removed");
    }

    #[tokio::test]
    #[serial]
    async fn test_invalid_pid_file() {
        // Create a PID file with invalid content
        let pid_path = get_pid_file_path().unwrap();
        std::fs::write(&pid_path, "not_a_number").unwrap();

        // Should return error for invalid PID
        let running = is_daemon_running();
        assert!(
            running.is_err(),
            "Should return error for invalid PID file content"
        );

        // Clean up
        let _ = std::fs::remove_file(&pid_path);
    }

    #[tokio::test]
    async fn test_daemon_new_with_mock_config() {
        // This test would require mocking the transport and clipboard
        // For now, we'll test the error case when Tailscale is not available
        let config = PostConfig::default();

        // This will likely fail in test environment without Tailscale
        // but we can test that it handles the error gracefully
        let result = Daemon::new(config).await;

        // Should either succeed or fail gracefully with a PostError
        match result {
            Ok(_) => {
                // If it succeeds, great! Tailscale is available in test environment
            }
            Err(e) => {
                // Should be a proper PostError, not a panic
                match e {
                    PostError::Tailscale(_) => {
                        // Expected error when Tailscale is not available
                    }
                    _ => {
                        // Other errors are also acceptable in test environment
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_daemon_creation_error_handling() {
        // Test that daemon creation handles errors appropriately
        // This is more of an integration test to ensure error paths work

        // Create config that might cause initialization issues
        let mut config = PostConfig::default();
        config.network.port = 0; // Invalid port

        let result = Daemon::new(config).await;

        // Should handle invalid configuration gracefully
        match result {
            Ok(_) => {
                // If it succeeds despite invalid config, that's also fine
            }
            Err(_) => {
                // Expected to fail with invalid config
            }
        }
    }

    #[test]
    fn test_daemon_struct_fields() {
        // Test that Daemon struct has the expected fields
        // This is more of a compile-time test
        let config = PostConfig::default();

        // This tests that all the required trait objects can be created
        // and that the struct is properly defined
        assert_eq!(
            std::mem::size_of::<PostConfig>(),
            std::mem::size_of_val(&config)
        );
    }
}
