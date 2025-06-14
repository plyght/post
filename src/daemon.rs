// Simple wrapper that just spawns the daemon directly
use clap::Parser;
use post_core::{PostConfig, Result};
use post_daemon::{daemonize, Daemon};
use signal_hook::consts::SIGTERM;
use signal_hook_tokio::Signals;
use std::sync::Arc;
use tokio::sync::Notify;
use tracing::{error, info};

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

#[tokio::main]
async fn main() -> Result<()> {
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
    }

    let shutdown = Arc::new(Notify::new());
    let shutdown_clone = Arc::clone(&shutdown);

    tokio::spawn(async move {
        let mut signals = Signals::new([SIGTERM]).expect("Failed to create signal handler");

        use futures_util::stream::StreamExt;
        while let Some(signal) = signals.next().await {
            if signal == SIGTERM {
                info!("Received SIGTERM, shutting down gracefully");
                shutdown_clone.notify_one();
                break;
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

    Ok(())
}
