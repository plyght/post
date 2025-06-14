use post_core::*;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

pub struct Daemon {
    #[allow(dead_code)]
    config: PostConfig,
    #[allow(dead_code)]
    clipboard: Arc<dyn ClipboardManager>,
    transport: Arc<dyn Transport>,
    sync_manager: Arc<SyncManager>,
}

impl Daemon {
    pub async fn new(config: PostConfig) -> Result<Self> {
        let clipboard = Arc::new(SystemClipboard::new()?);
        let transport = Arc::new(TailscaleTransport::new(config.network.port));
        let node_id = transport.get_node_id().await?;
        let sync_manager = Arc::new(SyncManager::new(clipboard.clone(), node_id));

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
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                interval.tick().await;
                if let Err(e) = sync_manager_cleanup.cleanup_stale_nodes(600).await {
                    error!("Failed to cleanup stale nodes: {}", e);
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
