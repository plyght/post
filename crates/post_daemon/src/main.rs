use clap::Parser;
use futures_util::stream::StreamExt;
use post_core::{PostConfig, Result};
use post_daemon::{daemonize, Daemon};
use signal_hook::consts::SIGTERM;
use signal_hook_tokio::Signals;
use std::sync::Arc;
use tokio::sync::Notify;
use tracing::{error, info};
use tracing_subscriber;

#[derive(Parser)]
#[command(name = "postd")]
#[command(about = "Post clipboard daemon")]
struct Args {
    #[arg(short, long)]
    config: Option<String>,

    #[arg(short, long)]
    foreground: bool,

    #[arg(short, long)]
    verbose: bool,
}

pub async fn daemon_main() -> Result<()> {
    let args = Args::parse();

    if args.verbose {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .init();
    }

    let config = if let Some(config_path) = args.config {
        let contents = tokio::fs::read_to_string(&config_path).await?;
        toml::from_str(&contents)?
    } else {
        PostConfig::load().await?
    };

    if !args.foreground {
        daemonize().await?;
    } else {
        // Even in foreground mode, write PID file for status checking
        post_daemon::write_pid_file()?;
        info!(
            "Running in foreground mode with PID: {}",
            std::process::id()
        );
    }

    let shutdown = Arc::new(Notify::new());
    let shutdown_clone = Arc::clone(&shutdown);

    tokio::spawn(async move {
        let mut signals = match Signals::new(&[SIGTERM]) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to create signal handler: {}", e);
                return;
            }
        };

        while let Some(signal) = signals.next().await {
            match signal {
                SIGTERM => {
                    info!("Received SIGTERM, shutting down gracefully");
                    shutdown_clone.notify_one();
                    break;
                }
                _ => {}
            }
        }
    });

    let daemon = Daemon::new(config).await?;

    tokio::select! {
        result = daemon.run() => {
            if let Err(e) = result {
                error!("Daemon error: {}", e);
            }
        }
        _ = shutdown.notified() => {
            info!("Shutting down daemon");
        }
    }

    // Cleanup PID file on shutdown
    if let Err(e) = post_daemon::remove_pid_file() {
        error!("Failed to remove PID file: {}", e);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    daemon_main().await
}
