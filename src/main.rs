use clap::{Parser, Subcommand};
use post_core::*;
use std::sync::Arc;

#[cfg(feature = "tui")]
use post_tui::{run_tui, App};

#[derive(Parser)]
#[command(name = "post")]
#[command(about = "Universal clipboard sync for Tailscale")]
#[command(version = "0.1.0")]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(short, long)]
    config: Option<String>,

    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Show clipboard status and nodes
    Status,

    /// Get current clipboard content
    Get,

    /// Set clipboard content
    Set {
        /// Content to set
        content: String,
    },

    /// Run the TUI interface
    #[cfg(feature = "tui")]
    Tui,

    /// Start the daemon
    Daemon {
        #[arg(short, long)]
        foreground: bool,
    },

    /// Generate default configuration
    Config,
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

    match args.command {
        Some(Commands::Status) => {
            let transport = TailscaleTransport::new(config.network.port);
            let node_id = transport.get_node_id().await?;
            let nodes = transport.get_tailnet_nodes().await?;

            println!("Post Clipboard Status");
            println!("Node ID: {}", node_id);
            println!("Connected nodes: {}", nodes.len());
            for node in nodes {
                println!("  - {}", node);
            }
        }

        Some(Commands::Get) => {
            let clipboard = SystemClipboard::new()?;
            let content = clipboard.get_contents().await?;
            println!("{}", content);
        }

        Some(Commands::Set { content }) => {
            let clipboard = SystemClipboard::new()?;
            clipboard.set_contents(&content).await?;
            println!("Clipboard updated");
        }

        #[cfg(feature = "tui")]
        Some(Commands::Tui) => {
            let app = Arc::new(App::new(config));
            run_tui(app).await?;
        }

        Some(Commands::Daemon { foreground }) => {
            if !foreground {
                #[cfg(unix)]
                post_daemon::daemonize().await?;
            }

            let daemon = post_daemon::Daemon::new(config).await?;
            daemon.run().await?;
        }

        Some(Commands::Config) => {
            let config_path = PostConfig::config_path()?;
            let config = PostConfig::default();
            config.save().await?;
            println!("Generated default config at: {}", config_path.display());
        }

        None => {
            #[cfg(feature = "tui")]
            {
                let app = Arc::new(App::new(config));
                run_tui(app).await?;
            }

            #[cfg(not(feature = "tui"))]
            {
                println!("Post clipboard sync tool");
                println!("Use 'post --help' for available commands");
            }
        }
    }

    Ok(())
}
