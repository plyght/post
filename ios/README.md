# Post iOS App

A secure clipboard synchronization iOS companion app for the Post system, featuring iOS 26 BGContinuedProcessingTask integration and comprehensive Shortcuts support.

## Architecture Overview

### Core Components

- **PostApp.swift**: Main SwiftUI app entry point
- **ContentView.swift**: Main UI with Status, Peers, and Settings tabs
- **PostManager**: Central coordinator for clipboard sync operations
- **PostClient**: Network client for Tailscale daemon communication
- **CryptoService**: ChaCha20-Poly1305 encryption matching post daemon
- **ClipboardService**: iOS pasteboard operations and monitoring
- **TailscaleService**: Tailscale network discovery and status
- **PostBackgroundTaskManager**: iOS 26 BGContinuedProcessingTask integration

### App Intents (Shortcuts Integration)

- **SyncClipboardIntent**: Sync clipboard across devices
- **PushClipboardIntent**: Send content to clipboard and sync
- **PullClipboardIntent**: Get latest clipboard from other devices
- **GetClipboardStatusIntent**: Check sync status

## Key Features

### iOS 26 Integration
- **BGContinuedProcessingTask**: Long-running background sync operations
- **System Progress UI**: Native iOS progress indicators
- **Background Continuation**: User-initiated tasks continue after app backgrounding

### Shortcuts Support
- **Back Tap Integration**: Double/triple tap triggers clipboard sync
- **Action Button**: iPhone 15 Pro+ direct action button support
- **Voice Commands**: "Sync my clipboard", "Post my clipboard"
- **Control Center**: Custom controls for quick access

### Security
- **End-to-End Encryption**: ChaCha20-Poly1305 (matching post daemon)
- **Key Exchange**: X25519 elliptic curve Diffie-Hellman
- **Digital Signatures**: Ed25519 authentication
- **Keychain Storage**: Secure key storage with hardware protection

### Network
- **Tailscale Integration**: Auto-discovery of post daemons
- **HTTP API**: RESTful communication with post daemon
- **Fallback Support**: Works on older iOS versions

## Project Structure

```
ios/
├── PostiOS.xcodeproj/           # Xcode project
├── PostiOS/                     # Main app target
│   ├── Sources/
│   │   ├── PostApp.swift        # App entry point
│   │   └── ContentView.swift    # Main UI
│   └── Resources/
│       ├── Info.plist          # App configuration
│       └── Assets.xcassets/    # App icons/images
├── PostCommon/                  # Shared framework
│   └── Sources/
│       ├── Models.swift         # Data models
│       ├── PostManager.swift    # Main coordinator
│       ├── PostClient.swift     # Network client
│       ├── CryptoService.swift  # Encryption
│       ├── ClipboardService.swift # Pasteboard ops
│       ├── TailscaleService.swift # Network discovery
│       ├── KeychainService.swift  # Secure storage
│       └── PostBackgroundTaskManager.swift # iOS 26 tasks
├── PostIntents/                 # App Intents target
│   └── Sources/
│       └── AppIntents.swift     # Shortcuts integration
├── PostShareExtension/          # Share sheet target (TODO)
└── README.md                    # This file
```

## Usage Instructions

### Setup
1. Install Tailscale on iOS device
2. Ensure post daemon running on desktop/server
3. Launch Post iOS app
4. Grant clipboard permissions

### Back Tap Setup
1. Settings → Accessibility → Touch → Back Tap
2. Choose Double Tap or Triple Tap
3. Select "Shortcuts"
4. Choose "Sync Clipboard" shortcut

### Action Button Setup (iPhone 15 Pro+)
1. Settings → Action Button
2. Choose "Shortcut"
3. Select "Sync Clipboard"

### Siri Setup
Say "Sync my clipboard" or "Post my clipboard"

## Development

### Requirements
- iOS 26 beta for BGContinuedProcessingTask
- Xcode 16.0+
- Swift 5.9+
- Tailscale iOS app installed

### Building
```bash
cd ios/
open PostiOS.xcodeproj
# Build and run in Xcode
```

### Testing
- Test on iOS 26 beta device for full functionality
- Test Action Button on iPhone 15 Pro+
- Test Back Tap on older devices
- Verify Tailscale connectivity

## Integration with Post Daemon

The iOS app communicates with existing post daemons via:
- **API Endpoints**: /api/v1/clipboard/sync, /api/v1/peers, /api/v1/status
- **Encryption**: Same ChaCha20-Poly1305 as desktop clients
- **Network**: Tailscale mesh networking (port 8412)
- **Discovery**: Tailscale local API for peer discovery

## Next Steps

1. Implement share sheet extension
2. Add iOS 26 beta testing
3. Create App Store assets
4. Add comprehensive unit tests
5. Performance optimization

The iOS app maintains full compatibility with the existing post ecosystem while leveraging iOS-specific features for optimal user experience.