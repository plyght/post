use clap::{Parser, Subcommand};
use post_core::*;
use std::sync::Arc;
use tracing::info;

mod service;

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

    /// Stop the running daemon
    Stop,

    /// Restart the daemon
    Restart {
        #[arg(short, long)]
        foreground: bool,
    },

    /// Show daemon status (running/stopped)
    DaemonStatus,

    /// Install daemon as system service (boot startup)
    Install,

    /// Uninstall daemon system service
    Uninstall,

    /// Show daemon logs
    Logs {
        #[arg(short, long)]
        follow: bool,
        #[arg(short, long, default_value = "50")]
        lines: usize,
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

    // Handle config command first, before trying to load config
    if let Some(Commands::Config) = args.command {
        let config_path = PostConfig::config_path()?;
        let config = PostConfig::default();
        config.save().await?;
        println!("Generated default config at: {}", config_path.display());
        return Ok(());
    }

    let config = if let Some(ref config_path) = args.config {
        let contents = tokio::fs::read_to_string(config_path).await?;
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
                #[cfg(target_os = "macos")]
                {
                    // On macOS, avoid fork() entirely by spawning a new process
                    use std::env;
                    use std::process::Command;

                    let current_exe = env::current_exe().map_err(|e| {
                        PostError::Other(format!("Failed to get current executable: {}", e))
                    })?;

                    let mut cmd = Command::new(&current_exe);
                    cmd.arg("daemon").arg("--foreground"); // Start in foreground in the new process

                    if let Some(ref config_path) = args.config {
                        cmd.arg("--config").arg(config_path);
                    }

                    if args.verbose {
                        cmd.arg("--verbose");
                    }

                    // Redirect stdout/stderr to log file
                    let log_path = post_daemon::get_log_file_path()?;
                    let log_file = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log_path)
                        .map_err(PostError::Io)?;

                    cmd.stdout(log_file.try_clone().map_err(PostError::Io)?)
                        .stderr(log_file)
                        .stdin(std::process::Stdio::null());

                    let child = cmd.spawn().map_err(|e| {
                        PostError::Other(format!("Failed to spawn daemon process: {}", e))
                    })?;

                    println!("Daemon started with PID: {}", child.id());
                    return Ok(());
                }

                #[cfg(not(target_os = "macos"))]
                {
                    post_daemon::daemonize().await?;
                    let daemon = post_daemon::Daemon::new(config).await?;
                    daemon.run().await?;
                }
            } else {
                // Even in foreground mode, write PID file for status checking
                post_daemon::write_pid_file()?;
                info!("Running daemon in foreground mode");
                let daemon = post_daemon::Daemon::new(config).await?;
                daemon.run().await?;
            }
        }

        Some(Commands::Stop) => {
            match post_daemon::is_daemon_running()? {
                Some(pid) => {
                    #[cfg(unix)]
                    {
                        use nix::sys::signal::{kill, Signal};
                        use nix::unistd::Pid;

                        kill(Pid::from_raw(pid as i32), Signal::SIGTERM).map_err(|e| {
                            PostError::Other(format!("Failed to stop daemon: {}", e))
                        })?;

                        // Wait a bit for graceful shutdown
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                        // Check if it's still running
                        match post_daemon::is_daemon_running()? {
                            Some(_) => println!("Daemon stop initiated (PID: {})", pid),
                            None => println!("Daemon stopped successfully"),
                        }
                    }

                    #[cfg(not(unix))]
                    {
                        println!("Daemon stop not supported on this platform. PID: {}", pid);
                    }
                }
                None => {
                    println!("Daemon is not running");
                }
            }
        }

        Some(Commands::Restart { foreground }) => {
            // Stop the daemon first
            if let Some(pid) = post_daemon::is_daemon_running()? {
                #[cfg(unix)]
                {
                    use nix::sys::signal::{kill, Signal};
                    use nix::unistd::Pid;

                    kill(Pid::from_raw(pid as i32), Signal::SIGTERM)
                        .map_err(|e| PostError::Other(format!("Failed to stop daemon: {}", e)))?;

                    // Wait for graceful shutdown
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
                println!("Stopped existing daemon (PID: {})", pid);
            }

            // Start new daemon directly in this process
            if !foreground {
                #[cfg(target_os = "macos")]
                {
                    // On macOS, avoid fork() entirely by spawning a new process
                    use std::env;
                    use std::process::Command;

                    let current_exe = env::current_exe().map_err(|e| {
                        PostError::Other(format!("Failed to get current executable: {}", e))
                    })?;

                    let mut cmd = Command::new(&current_exe);
                    cmd.arg("daemon").arg("--foreground"); // Start in foreground in the new process

                    if let Some(ref config_path) = args.config {
                        cmd.arg("--config").arg(config_path);
                    }

                    if args.verbose {
                        cmd.arg("--verbose");
                    }

                    // Redirect stdout/stderr to log file
                    let log_path = post_daemon::get_log_file_path()?;
                    let log_file = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log_path)
                        .map_err(PostError::Io)?;

                    cmd.stdout(log_file.try_clone().map_err(PostError::Io)?)
                        .stderr(log_file)
                        .stdin(std::process::Stdio::null());

                    let child = cmd.spawn().map_err(|e| {
                        PostError::Other(format!("Failed to spawn daemon process: {}", e))
                    })?;

                    println!("Daemon restarted with PID: {}", child.id());
                    return Ok(());
                }

                #[cfg(not(target_os = "macos"))]
                {
                    post_daemon::daemonize().await?;
                    let daemon = post_daemon::Daemon::new(config).await?;
                    daemon.run().await?;
                }
            } else {
                // Even in foreground mode, write PID file for status checking
                post_daemon::write_pid_file()?;
                println!("Starting daemon in foreground...");
                let daemon = post_daemon::Daemon::new(config).await?;
                daemon.run().await?;
            }
        }

        Some(Commands::DaemonStatus) => {
            match post_daemon::is_daemon_running()? {
                Some(pid) => {
                    println!("Daemon is running (PID: {})", pid);

                    // Show additional info
                    let pid_file = post_daemon::get_pid_file_path()?;
                    let log_file = post_daemon::get_log_file_path()?;
                    println!("PID file: {}", pid_file.display());
                    println!("Log file: {}", log_file.display());
                }
                None => {
                    println!("Daemon is not running");
                }
            }
        }

        Some(Commands::Install) => {
            service::install_service().await?;
        }

        Some(Commands::Uninstall) => {
            service::uninstall_service().await?;
        }

        Some(Commands::Logs { follow, lines }) => {
            show_logs(follow, lines).await?;
        }

        Some(Commands::Config) => {
            // This is handled earlier in main() before config loading
            unreachable!("Config command should be handled before this match")
        }

        None => {
            // Show help when no command is provided
            use clap::CommandFactory;
            Args::command().print_help()?;
            println!();
        }
    }

    Ok(())
}

async fn show_logs(follow: bool, lines: usize) -> Result<()> {
    let log_path = post_daemon::get_log_file_path()?;

    if !log_path.exists() {
        println!("Log file not found: {}", log_path.display());
        println!("The daemon may not have been started yet, or logging is not configured.");
        return Ok(());
    }

    if follow {
        println!(
            "Following log file: {} (Press Ctrl+C to stop)",
            log_path.display()
        );

        // Try to use tail -f, fallback to native implementation
        let tail_result = tokio::process::Command::new("tail")
            .args([
                "-f",
                log_path
                    .to_str()
                    .ok_or_else(|| PostError::Other("Invalid log path".to_string()))?,
            ])
            .spawn();

        match tail_result {
            Ok(mut cmd) => {
                tokio::select! {
                    _ = cmd.wait() => {},
                    _ = tokio::signal::ctrl_c() => {
                        cmd.kill().await.ok();
                        println!("\nStopped following logs");
                    }
                }
            }
            Err(_) => {
                // Fallback to native log following
                println!("tail command not available, using native log following");

                use std::io::{Read, Seek, SeekFrom};
                use std::time::Duration;

                let mut file = std::fs::File::open(&log_path).map_err(PostError::Io)?;
                file.seek(SeekFrom::End(0)).map_err(PostError::Io)?;

                loop {
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {
                            println!("\nStopped following logs");
                            break;
                        }
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {
                            let mut temp_buffer = String::new();
                            match file.read_to_string(&mut temp_buffer) {
                                Ok(0) => continue, // No new data
                                Ok(_) => {
                                    print!("{}", temp_buffer);
                                    std::io::Write::flush(&mut std::io::stdout()).ok();
                                }
                                Err(_) => break, // File may have been rotated or removed
                            }
                        }
                    }
                }
            }
        }
    } else {
        // Show last N lines - try tail command first, fallback to native implementation
        let tail_result = tokio::process::Command::new("tail")
            .args([
                "-n",
                &lines.to_string(),
                log_path
                    .to_str()
                    .ok_or_else(|| PostError::Other("Invalid log path".to_string()))?,
            ])
            .output()
            .await;

        match tail_result {
            Ok(output) if output.status.success() => {
                println!("Last {} lines from {}", lines, log_path.display());
                println!("---");
                print!("{}", String::from_utf8_lossy(&output.stdout));
            }
            _ => {
                // Fallback to reading file directly (either tail failed or command not found)
                let content = tokio::fs::read_to_string(&log_path)
                    .await
                    .map_err(PostError::Io)?;
                let lines_vec: Vec<&str> = content.lines().collect();
                let start = if lines_vec.len() > lines {
                    lines_vec.len() - lines
                } else {
                    0
                };

                println!("Last {} lines from {} (native)", lines, log_path.display());
                println!("---");
                for line in &lines_vec[start..] {
                    println!("{}", line);
                }
            }
        }
    }

    Ok(())
}
