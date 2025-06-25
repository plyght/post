# Post

A secure, distributed clipboard synchronization system built in Rust, leveraging Tailscale for network connectivity.

## Overview

Post enables seamless clipboard sharing across multiple devices using encrypted peer-to-peer communication over Tailscale networks. The system consists of a daemon for background synchronization and an optional TUI for monitoring and management.

## Project Structure

```
post/
├── Cargo.toml                 # Workspace configuration and main binary
├── Cargo.lock                 # Locked dependency versions
├── README.md                  # This file
├── src/                       # Main application sources
│   ├── main.rs               # CLI application entry point
│   └── daemon.rs             # Daemon binary entry point
├── crates/                    # Workspace crates
│   ├── post_core/            # Core functionality
│   │   ├── Cargo.toml        # Core library configuration
│   │   └── src/
│   │       ├── lib.rs        # Core library exports
│   │       ├── clipboard.rs  # Clipboard interaction
│   │       ├── config.rs     # Configuration management
│   │       ├── crypto.rs     # End-to-end encryption
│   │       ├── error.rs      # Error types and handling
│   │       ├── sync.rs       # Synchronization logic
│   │       └── transport.rs  # Network transport layer
│   ├── post_daemon/          # Background daemon
│   │   ├── Cargo.toml        # Daemon configuration
│   │   └── src/
│   │       ├── lib.rs        # Daemon library
│   │       └── main.rs       # Daemon entry point
│   └── post_tui/             # Terminal user interface
│       ├── Cargo.toml        # TUI configuration
│       └── src/
│           └── lib.rs        # TUI implementation
└── target/                   # Build artifacts (generated)
```

## Architecture

### Core Components

- **post_core**: Core library containing all fundamental functionality
  - Multi-platform clipboard operations and monitoring (Linux, macOS, Windows, WSL)
  - Intelligent clipboard backend selection with automatic fallback
  - End-to-end encryption using ChaCha20-Poly1305 and X25519
  - Peer discovery and synchronization logic
  - Tailscale integration for network connectivity
  - Configuration management
  
- **post_daemon**: Background service binary (`postd`)
  - Runs as a system service/daemon
  - Handles automatic clipboard synchronization
  - Manages peer connections and discovery
  - Supports Unix and Windows service frameworks
  
- **post_tui**: Terminal user interface (optional)
  - Real-time monitoring of clipboard sync status
  - Peer connection visualization
  - Built with Ratatui for cross-platform terminal UI
  
- **post**: Main CLI application
  - Command-line interface for configuration
  - Manual sync operations
  - Status reporting and diagnostics

### Security Architecture

- **End-to-End Encryption**: All clipboard data is encrypted using ChaCha20-Poly1305
- **Key Exchange**: Secure key exchange via X25519 elliptic curve
- **Authentication**: Ed25519 digital signatures for peer authentication
- **Network Security**: Leverages Tailscale's secure mesh networking

## Features

- 🔒 End-to-end encrypted clipboard synchronization
- 🌐 Peer-to-peer network discovery via Tailscale
- 🔄 Background daemon operation with service integration
- 📊 Terminal-based monitoring interface with real-time updates
- 🖥️ Cross-platform support (Linux, macOS, Windows, WSL) with intelligent clipboard backend selection
- ⚡ Low-latency clipboard detection and synchronization
- 🔧 Multiple clipboard backends (wl-clipboard, xclip, xsel, native APIs) with automatic fallback
- 🔧 Flexible configuration system
- 📝 Structured logging and diagnostics

## Clipboard Support

Post provides comprehensive clipboard integration across all major platforms with automatic detection and fallback mechanisms:

### Linux
- **Wayland**: `wl-clipboard` (wl-copy/wl-paste) - recommended for Wayland sessions
- **X11**: `xclip` or `xsel` - automatic detection and preference for X11 sessions  
- **Hybrid**: Combines Wayland and X11 support for maximum compatibility
- **Desktop Environments**: Automatic detection for KDE, GNOME, i3, dwm, Sway, and others
- **System**: Fallback to system clipboard APIs

### Windows
- **Native**: Windows system clipboard API
- **WSL**: Integrated Windows clipboard access via clip.exe and PowerShell
- **Auto-detection**: Automatically detects WSL environment and uses appropriate backend

### macOS
- **Native**: macOS system clipboard with Universal Clipboard support
- **Advanced**: Pasteboard change detection for efficient monitoring

### Installation Requirements

**Linux (choose one or more):**
```bash
# Ubuntu/Debian
sudo apt install wl-clipboard xclip  # or xsel

# Fedora/RHEL
sudo dnf install wl-clipboard xclip  # or xsel

# Arch Linux
sudo pacman -S wl-clipboard xclip    # or xsel
```

**WSL:**
Ensure Windows clipboard utilities are accessible:
- PowerShell (recommended for reading)
- clip.exe (for writing)

### Configuration

The clipboard backend can be configured in `config.toml`:

```toml
[clipboard]
# Backend selection: auto, system, wayland, xclip, xsel, wsl, windows
backend = "auto"

# Enable Wayland fallback for hybrid environments
wayland_fallback = true

# Polling interval for clipboard changes (milliseconds)
poll_interval_ms = 500

# Maximum clipboard content size (bytes)
max_content_size = 1048576

# Enable Sway-specific optimizations
sway_optimizations = true
```

## Installation

### Prerequisites

- Rust toolchain (1.70.0 or later)
- Tailscale installed and configured on all devices
- Platform-specific dependencies:
  - **macOS**: Xcode command line tools
  - **Linux**: X11 or Wayland development libraries, clipboard utilities (see Clipboard Support section)
  - **Windows**: Visual Studio Build Tools
  - **WSL**: Windows clipboard integration (clip.exe, PowerShell)

### Building from Source

```bash
# Clone the repository
git clone https://github.com/yourorg/post.git
cd post

# Build all binaries in release mode
cargo build --release

# Build specific components
cargo build --release --bin postd    # Daemon only
cargo build --release --bin post     # CLI only
cargo build --release --no-default-features  # Without TUI
```

### Installation

```bash
# Install to system PATH
cargo install --path .

# Or copy binaries manually
cp target/release/post* /usr/local/bin/
```

## Usage

### Starting the Daemon

```bash
# Start daemon in foreground
postd

# Start daemon in background
postd --daemon

# Start with custom config
postd --config /path/to/config.toml

# Enable verbose logging
postd --verbose
```

### CLI Commands

```bash
# Show current status
post status

# Start TUI monitoring interface
post

# Manual synchronization
post sync

# Show peer information
post peers

# Configuration management
post config --show
post config --set key=value

# Clipboard diagnostics
post clipboard-diag
```

### TUI Interface

The terminal user interface provides real-time monitoring:

- **Status Panel**: Current clipboard content and sync status
- **Peers Panel**: Connected nodes and their status
- **Logs Panel**: Real-time logging and diagnostics
- **Help Panel**: Keyboard shortcuts and commands

**Keyboard Shortcuts:**
- `q` or `Ctrl+C`: Quit
- `r`: Refresh/force sync
- `Tab`: Switch between panels
- `↑/↓`: Navigate lists

## Configuration

Configuration is managed through TOML files located at:

- **Linux**: `~/.config/post/config.toml`
- **macOS**: `~/Library/Preferences/post/config.toml`
- **Windows**: `%APPDATA%\post\config.toml`

### Example Configuration

```toml
[general]
# Node identifier (auto-generated if not specified)
node_id = "my-laptop"

# Sync interval in seconds
sync_interval = 5

# Enable debug logging
debug = false

[network]
# Tailscale local API socket (auto-detected if not specified)
tailscale_socket = "/var/run/tailscale/tailscaled.sock"

# Network port for peer communication
port = 8412

[clipboard]
# Backend selection: auto, system, wayland, xclip, xsel, wsl, windows
backend = "auto"

# Enable Wayland fallback for hybrid environments  
wayland_fallback = true

# Polling interval for clipboard changes (milliseconds)
poll_interval_ms = 500

# Maximum clipboard content size (bytes)
max_content_size = 1048576

# Enable Sway-specific optimizations
sway_optimizations = true

[encryption]
# Key derivation rounds (higher = more secure, slower)
pbkdf2_rounds = 100000

# Rotate encryption keys (hours)
key_rotation_interval = 24
```

## Development

### Building

```bash
# Development build
cargo build

# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run --bin post

# Format code
cargo fmt

# Lint code
cargo clippy
```

### Workspace Structure

This project uses Cargo workspaces for modular development:

- **Shared dependencies** are defined in the workspace `Cargo.toml`
- **Cross-crate dependencies** use path-based references
- **Feature flags** control optional functionality (e.g., TUI support)

### Platform-Specific Code

Platform-specific implementations are isolated using:

- **Conditional compilation**: `#[cfg(target_os = "...")]`
- **Platform-specific dependencies** in `Cargo.toml`
- **Runtime detection** for feature availability

## Troubleshooting

### Common Issues

**Daemon won't start:**
- Ensure Tailscale is running and authenticated
- Check that the configured port is not in use
- Verify configuration file syntax

**Clipboard not syncing:**
- Confirm all nodes are connected to the same Tailscale network
- Check firewall settings for the configured port
- Verify clipboard permissions on the local system
- Run clipboard diagnostics: `post clipboard-diag`
- Ensure required clipboard utilities are installed (see Clipboard Support section)
- Try different clipboard backends in configuration if auto-detection fails

**Performance issues:**
- Reduce sync frequency in configuration
- Enable debug logging to identify bottlenecks
- Check network connectivity between peers

### Logging

Enable detailed logging:

```bash
# Environment variable
RUST_LOG=post=debug,post_core=debug postd

# Configuration file
debug = true
```

Log files are written to:
- **Linux**: `~/.local/share/post/logs/`
- **macOS**: `~/Library/Logs/post/`
- **Windows**: `%APPDATA%\post\logs\`

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

## Contributing

Contributions are welcome! Please read our contributing guidelines and submit pull requests to our repository.

## Security

For security-related issues, please email security@yourorg.com instead of using the public issue tracker.