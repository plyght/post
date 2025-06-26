import AppIntents
import UIKit

@available(iOS 16.0, *)
struct SyncClipboardIntent: AppIntent {
    static var title: LocalizedStringResource = "Sync Clipboard"
    static var description = IntentDescription("Sync clipboard content across all your devices using Post")
    static var openAppWhenRun: Bool = false
    
    @Parameter(title: "Target Device", description: "Specific device to sync with (optional)")
    var targetDevice: DeviceEntity?
    
    @Parameter(title: "Show Progress", description: "Show sync progress in system UI")
    var showProgress: Bool
    
    init() {
        self.showProgress = true
    }
    
    init(targetDevice: DeviceEntity? = nil, showProgress: Bool = true) {
        self.targetDevice = targetDevice
        self.showProgress = showProgress
    }
    
    func perform() async throws -> some IntentResult & ProvidesDialog {
        if #available(iOS 26.0, *), showProgress {
            return try await performWithBackgroundTask()
        } else {
            return try await performQuickSync()
        }
    }
    
    @available(iOS 26.0, *)
    private func performWithBackgroundTask() async throws -> some IntentResult & ProvidesDialog {
        await PostBackgroundTaskManager.shared.startBackgroundSync {
            await self.performSyncOperation()
        }
        
        return .result(dialog: "Clipboard sync started in background")
    }
    
    private func performQuickSync() async throws -> some IntentResult & ProvidesDialog {
        await performSyncOperation()
        return .result(dialog: "Clipboard synced successfully")
    }
    
    private func performSyncOperation() async {
        let postManager = PostManager()
        await postManager.syncClipboard()
    }
}

@available(iOS 16.0, *)
struct PushClipboardIntent: AppIntent {
    static var title: LocalizedStringResource = "Push Clipboard Content"
    static var description = IntentDescription("Send specific content to your clipboard and sync across devices")
    static var openAppWhenRun: Bool = false
    
    @Parameter(title: "Content", description: "Text content to add to clipboard")
    var content: String
    
    @Parameter(title: "Content Type", description: "Type of content being shared")
    var contentType: ClipboardContentTypeEntity
    
    init() {
        self.content = ""
        self.contentType = ClipboardContentTypeEntity(type: .text)
    }
    
    init(content: String, contentType: ClipboardContentTypeEntity = ClipboardContentTypeEntity(type: .text)) {
        self.content = content
        self.contentType = contentType
    }
    
    func perform() async throws -> some IntentResult & ProvidesDialog {
        guard !content.isEmpty else {
            throw AppIntentError.invalidParameter
        }
        
        let clipboardService = ClipboardService()
        await clipboardService.setClipboard(content)
        
        let postManager = PostManager()
        await postManager.syncClipboard()
        
        return .result(dialog: "Content added to clipboard and synced")
    }
}

@available(iOS 16.0, *)
struct PullClipboardIntent: AppIntent {
    static var title: LocalizedStringResource = "Pull Latest Clipboard"
    static var description = IntentDescription("Get the latest clipboard content from your other devices")
    static var openAppWhenRun: Bool = false
    
    func perform() async throws -> some IntentResult & ReturnsValue<String> {
        let postManager = PostManager()
        let postClient = PostClient(configuration: PostConfiguration.default)
        
        await postClient.connect()
        
        if let clipboardContent = await postClient.pullClipboard() {
            let clipboardService = ClipboardService()
            await clipboardService.setClipboard(clipboardContent.content)
            
            return .result(value: clipboardContent.content, dialog: "Latest clipboard content retrieved")
        } else {
            return .result(value: "", dialog: "No new clipboard content found")
        }
    }
}

@available(iOS 16.0, *)
struct GetClipboardStatusIntent: AppIntent {
    static var title: LocalizedStringResource = "Get Clipboard Status"
    static var description = IntentDescription("Check the status of Post clipboard sync")
    static var openAppWhenRun: Bool = false
    
    func perform() async throws -> some IntentResult & ProvidesDialog {
        let postManager = PostManager()
        let connectionInfo = postManager.connectionStatusText
        let lastSync = postManager.lastSyncText
        
        let statusMessage = "Connection: \(connectionInfo)\nLast sync: \(lastSync)"
        
        return .result(dialog: statusMessage)
    }
}

@available(iOS 16.0, *)
struct DeviceEntity: AppEntity {
    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "Device")
    static var defaultQuery = DeviceQuery()
    
    let id: String
    let name: String
    let ipAddress: String
    let isOnline: Bool
    
    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(
            title: "\(name)",
            subtitle: isOnline ? "Online" : "Offline",
            image: .init(systemName: isOnline ? "checkmark.circle.fill" : "xmark.circle.fill")
        )
    }
    
    init(id: String, name: String, ipAddress: String, isOnline: Bool) {
        self.id = id
        self.name = name
        self.ipAddress = ipAddress
        self.isOnline = isOnline
    }
    
    init(from peer: PostPeer) {
        self.id = peer.id
        self.name = peer.name
        self.ipAddress = peer.ipAddress
        self.isOnline = peer.isOnline
    }
}

@available(iOS 16.0, *)
struct DeviceQuery: EntityQuery {
    func entities(for identifiers: [String]) async throws -> [DeviceEntity] {
        let postManager = PostManager()
        let peers = postManager.peers
        
        return peers.compactMap { peer in
            if identifiers.contains(peer.id) {
                return DeviceEntity(from: peer)
            }
            return nil
        }
    }
    
    func suggestedEntities() async throws -> [DeviceEntity] {
        let postManager = PostManager()
        await postManager.refreshPeers()
        
        return postManager.peers.map { DeviceEntity(from: $0) }
    }
}

@available(iOS 16.0, *)
struct ClipboardContentTypeEntity: AppEntity {
    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "Content Type")
    static var defaultQuery = ClipboardContentTypeQuery()
    
    let type: ClipboardContentType
    
    var id: String {
        type.rawValue
    }
    
    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(title: "\(type.displayName)")
    }
    
    init(type: ClipboardContentType) {
        self.type = type
    }
}

@available(iOS 16.0, *)
struct ClipboardContentTypeQuery: EntityQuery {
    func entities(for identifiers: [String]) async throws -> [ClipboardContentTypeEntity] {
        return ClipboardContentType.allCases.compactMap { type in
            if identifiers.contains(type.rawValue) {
                return ClipboardContentTypeEntity(type: type)
            }
            return nil
        }
    }
    
    func suggestedEntities() async throws -> [ClipboardContentTypeEntity] {
        return ClipboardContentType.allCases.map { ClipboardContentTypeEntity(type: $0) }
    }
}

@available(iOS 16.0, *)
struct PostAppShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: SyncClipboardIntent(),
            phrases: [
                "Sync my clipboard with \(.applicationName)",
                "Post my clipboard",
                "Sync clipboard across devices"
            ],
            shortTitle: "Sync Clipboard",
            systemImageName: "clipboard"
        )
        
        AppShortcut(
            intent: PullClipboardIntent(),
            phrases: [
                "Get my latest clipboard",
                "Pull clipboard from other devices",
                "Update my clipboard"
            ],
            shortTitle: "Pull Clipboard",
            systemImageName: "arrow.down.circle"
        )
        
        AppShortcut(
            intent: GetClipboardStatusIntent(),
            phrases: [
                "Check clipboard sync status",
                "How is my clipboard sync doing",
                "Post status"
            ],
            shortTitle: "Clipboard Status",
            systemImageName: "info.circle"
        )
    }
}

enum AppIntentError: Error, LocalizedError {
    case invalidParameter
    case networkError
    case authenticationRequired
    
    var errorDescription: String? {
        switch self {
        case .invalidParameter:
            return "Invalid parameter provided"
        case .networkError:
            return "Network connection failed"
        case .authenticationRequired:
            return "Authentication required"
        }
    }
}

@available(iOS 16.0, *)
extension SyncClipboardIntent {
    static var suggestedInvocationPhrase: String {
        "Sync my clipboard"
    }
}

@available(iOS 16.0, *)
extension PushClipboardIntent {
    static var suggestedInvocationPhrase: String {
        "Add to clipboard"
    }
}

@available(iOS 16.0, *)
extension PullClipboardIntent {
    static var suggestedInvocationPhrase: String {
        "Get latest clipboard"
    }
}