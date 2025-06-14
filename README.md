# Post

A secure, distributed clipboard synchronization system built in Rust, leveraging Tailscale for network connectivity.

## Overview

Post enables seamless clipboard sharing across multiple devices using encrypted peer-to-peer communication over Tailscale networks. The system consists of a daemon for background synchronization and an optional TUI for monitoring and management.

## Architecture

- **post_core**: Core library with clipboard, crypto, and sync functionality
- **post_daemon**: Background service for clipboard synchronization
- **post_tui**: Terminal user interface for system monitoring
- **post**: Main CLI application
- **postd**: Daemon binary

## Features

- End-to-end encrypted clipboard synchronization
- Peer-to-peer network discovery via Tailscale
- Background daemon operation
- Terminal-based monitoring interface
- Cross-platform support

## Installation

```bash
cargo build --release
```

## Usage

Start the daemon:
```bash
./target/release/postd
```

Monitor with TUI:
```bash
./target/release/post
```

## Configuration

Configuration is managed through `config.toml` files. See the source for available options.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.