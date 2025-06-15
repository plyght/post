use post_core::*;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

pub struct Daemon {
    config: PostConfig,
    clipboard: Arc<dyn ClipboardManager>,
    transport: Arc<dyn Transport>,
    sync_manager: Arc<SyncManager>,
}

impl Daemon {
    pub async fn new(config: PostConfig) -> Result<Self> {
        let clipboard = Arc::new(SystemClipboard::new()?);

        // Use the new detection method that tries multiple socket paths
        let transport = match TailscaleTransport::new_with_detection(config.network.port).await {
            Ok(transport) => Arc::new(transport),
            Err(e) => {
                // Fallback to old method for compatibility
                warn!(
                    "Failed to detect Tailscale with new method: {}, falling back to default",
                    e
                );
                let transport = Arc::new(TailscaleTransport::new(config.network.port));

                // Check Tailscale connectivity before proceeding
                if !transport.is_connected().await? {
                    return Err(PostError::Tailscale(
                        "Tailscale is not connected. Please ensure Tailscale is running and connected to your tailnet.".to_string()
                    ));
                }
                transport
            }
        };

        let node_id = transport.get_node_id().await?;
        let sync_manager = Arc::new(SyncManager::new(clipboard.clone(), node_id.clone())?);

        info!("Daemon initialized with Tailscale node ID: {}", node_id);

        Ok(Self {
            config,
            clipboard,
            transport,
            sync_manager,
        })
    }

    pub async fn run(&self) -> Result<()> {
        info!("Starting Post daemon");

        let (tx, mut rx) = mpsc::unbounded_channel();

        let sync_manager = Arc::clone(&self.sync_manager);
        let transport_clone = Arc::clone(&self.transport);

        tokio::spawn(async move {
            if let Err(e) = transport_clone.start_listening(tx).await {
                error!("Transport listener failed: {}", e);
            }
        });

        let transport_send = Arc::clone(&self.transport);
        let sync_manager_clone = Arc::clone(&self.sync_manager);

        sync_manager
            .start_sync_loop(move |message| {
                let transport = Arc::clone(&transport_send);
                tokio::spawn(async move {
                    if let Err(e) = transport.send_message(message).await {
                        error!("Failed to send message: {}", e);
                    }
                });
            })
            .await?;

        let sync_manager_cleanup = Arc::clone(&self.sync_manager);
        let cleanup_interval = self.config.network.discovery_interval * 10; // Cleanup every 10 discovery intervals
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(cleanup_interval));
            loop {
                interval.tick().await;
                if let Err(e) = sync_manager_cleanup
                    .cleanup_stale_nodes(cleanup_interval * 2)
                    .await
                {
                    error!("Failed to cleanup stale nodes: {}", e);
                }
            }
        });

        // Heartbeat task
        let transport_heartbeat = Arc::clone(&self.transport);
        let heartbeat_interval = self.config.network.heartbeat_interval;
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(heartbeat_interval));
            loop {
                interval.tick().await;
                // Get tailnet nodes and send heartbeat to each
                if let Ok(nodes) = transport_heartbeat.get_tailnet_nodes().await {
                    debug!("Heartbeat tick - found {} nodes", nodes.len());
                } else {
                    debug!("Heartbeat tick - failed to get nodes");
                }
            }
        });

        // Clipboard health check task
        let clipboard_health = Arc::clone(&self.clipboard);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Err(e) = clipboard_health.get_contents().await {
                    error!("Clipboard health check failed: {}", e);
                }
            }
        });

        // Tailscale connectivity health check task
        let transport_health = Arc::clone(&self.transport);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                match transport_health.is_connected().await {
                    Ok(true) => {
                        debug!("Tailscale connectivity check: OK");
                    }
                    Ok(false) => {
                        error!("Tailscale connectivity check: FAILED - Tailscale not connected");
                    }
                    Err(e) => {
                        error!("Tailscale connectivity check: ERROR - {}", e);
                    }
                }
            }
        });

        while let Some(message) = rx.recv().await {
            if let Err(e) = sync_manager_clone.handle_message(message).await {
                error!("Failed to handle message: {}", e);
            }
        }

        Ok(())
    }
}

#[cfg(unix)]
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

            let devnull = File::open("/dev/null").map_err(PostError::Io)?;

            unsafe {
                libc::dup2(devnull.as_raw_fd(), 0);
                libc::dup2(devnull.as_raw_fd(), 1);
                libc::dup2(devnull.as_raw_fd(), 2);
            }

            std::env::set_current_dir("/").map_err(PostError::Io)?;
        }
        Err(e) => {
            return Err(PostError::Other(format!("Fork failed: {}", e)));
        }
    }

    Ok(())
}

#[cfg(not(unix))]
pub async fn daemonize() -> Result<()> {
    info!("Daemonization not supported on this platform");
    Ok(())
}
