use clap::{Parser, Subcommand};
use post_core::*;
use std::sync::Arc;
use tracing::info;

#[cfg(feature = "tui")]
use post_tui::{run_tui, App};

#[derive(Parser)]
#[command(name = "post")]
#[command(about = "Universal clipboard sync daemon for Tailscale")]
#[command(version = "0.1.0")]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(short, long)]
    config: Option<String>,

    #[arg(short, long)]
    verbose: bool,

    #[arg(short, long)]
    foreground: bool,
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

    if args.verbose || args.foreground {
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
            println!("Post Clipboard Status");

            // Try the improved detection method first
            match TailscaleTransport::new_with_detection(config.network.port).await {
                Ok(transport) => {
                    println!("Tailscale: Connected");

                    match transport.get_node_id().await {
                        Ok(node_id) => println!("Node ID: {}", node_id),
                        Err(e) => println!("Node ID: Failed to get ({:?})", e),
                    }

                    match transport.get_tailnet_nodes().await {
                        Ok(nodes) => {
                            println!("Connected nodes: {}", nodes.len());
                            for node in nodes {
                                println!("  - {}", node);
                            }
                        }
                        Err(e) => println!("Connected nodes: Failed to get ({:?})", e),
                    }
                }
                Err(e) => {
                    println!("Tailscale: Could not connect to daemon");
                    println!("Error: {}", e);
                    println!("Please ensure Tailscale is installed and running");

                    // Show what paths were tried for debugging
                    if args.verbose {
                        println!("\nDebugging information:");
                        let paths = TailscaleTransport::get_possible_socket_paths();
                        for path in paths {
                            let exists = std::path::Path::new(&path).exists();
                            println!("  Tried: {} (exists: {})", path, exists);
                        }

                        #[cfg(target_os = "macos")]
                        {
                            if let Some(tcp_port) = TailscaleTransport::detect_macos_tcp_port() {
                                println!("  Tried: TCP localhost:{}", tcp_port);
                            }
                        }
                    }
                }
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
            // Default behavior: start the daemon
            info!("Starting Post daemon (use --help for other options)");

            if !args.foreground {
                #[cfg(unix)]
                post_daemon::daemonize().await?;
            }

            let daemon = post_daemon::Daemon::new(config).await?;
            daemon.run().await?;
        }
    }

    Ok(())
}
