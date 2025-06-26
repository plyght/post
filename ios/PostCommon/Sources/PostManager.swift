import Foundation
import Combine
import SwiftUI

@MainActor
class PostManager: ObservableObject {
    @Published var isConnected = false
    @Published var peers: [PostPeer] = []
    @Published var clipboardContent: String?
    @Published var lastSyncTime: Date?
    @Published var currentError: PostError?
    
    private let postClient: PostClient
    private let clipboardService: ClipboardService
    private let cryptoService: CryptoService
    private let tailscaleService: TailscaleService
    private let backgroundTaskManager: PostBackgroundTaskManager
    
    private var syncTimer: Timer?
    private var cancellables = Set<AnyCancellable>()
    private let configuration: PostConfiguration
    
    init() {
        self.configuration = PostConfiguration.default
        self.postClient = PostClient(configuration: configuration)
        self.clipboardService = ClipboardService()
        self.cryptoService = CryptoService()
        self.tailscaleService = TailscaleService()
        self.backgroundTaskManager = PostBackgroundTaskManager.shared
        
        setupBindings()
        startServices()
    }
    
    deinit {
        stopSyncTimer()
        cancellables.removeAll()
    }
    
    private func setupBindings() {
        postClient.isConnectedPublisher
            .receive(on: DispatchQueue.main)
            .assign(to: \.isConnected, on: self)
            .store(in: &cancellables)
        
        postClient.peersPublisher
            .receive(on: DispatchQueue.main)
            .assign(to: \.peers, on: self)
            .store(in: &cancellables)
        
        postClient.errorPublisher
            .receive(on: DispatchQueue.main)
            .assign(to: \.currentError, on: self)
            .store(in: &cancellables)
        
        clipboardService.contentPublisher
            .receive(on: DispatchQueue.main)
            .assign(to: \.clipboardContent, on: self)
            .store(in: &cancellables)
    }
    
    private func startServices() {
        Task {
            do {
                await cryptoService.initializeKeys()
                await tailscaleService.startMonitoring()
                await postClient.connect()
                
                if configuration.enableBackgroundSync {
                    startSyncTimer()
                }
                
                await clipboardService.startMonitoring()
                
            } catch {
                currentError = error as? PostError ?? .networkError(error.localizedDescription)
            }
        }
    }
    
    private func startSyncTimer() {
        stopSyncTimer()
        syncTimer = Timer.scheduledTimer(withTimeInterval: configuration.syncInterval, repeats: true) { _ in
            Task { @MainActor in
                await self.syncClipboard()
            }
        }
    }
    
    private func stopSyncTimer() {
        syncTimer?.invalidate()
        syncTimer = nil
    }
    
    func syncClipboard() async {
        do {
            guard let content = clipboardContent else { return }
            
            let clipboardData = ClipboardContent(
                content: content,
                contentType: .text,
                nodeId: configuration.nodeId
            )
            
            await postClient.syncClipboard(clipboardData)
            lastSyncTime = Date()
            
        } catch {
            currentError = error as? PostError ?? .networkError(error.localizedDescription)
        }
    }
    
    func syncClipboardWithBackground() async {
        await backgroundTaskManager.startBackgroundSync { [weak self] in
            await self?.syncClipboard()
        }
    }
    
    func refreshPeers() async {
        await postClient.refreshPeers()
    }
    
    func sendClipboardToPeer(_ peer: PostPeer) async {
        guard let content = clipboardContent else { return }
        
        let clipboardData = ClipboardContent(
            content: content,
            contentType: .text,
            nodeId: configuration.nodeId
        )
        
        await postClient.sendClipboardToPeer(peer, content: clipboardData)
    }
    
    func handleIncomingClipboard(_ content: ClipboardContent) async {
        do {
            let decryptedContent = try await cryptoService.decrypt(content.content)
            await clipboardService.setClipboard(decryptedContent)
            clipboardContent = decryptedContent
            lastSyncTime = Date()
        } catch {
            currentError = .encryptionError("Failed to decrypt incoming clipboard: \(error.localizedDescription)")
        }
    }
    
    func resetConfiguration() {
        stopSyncTimer()
        
        Task {
            await cryptoService.resetKeys()
            await postClient.disconnect()
            
            if configuration.enableBackgroundSync {
                startSyncTimer()
            }
            
            await postClient.connect()
        }
    }
    
    func enableBackgroundSync(_ enabled: Bool) {
        if enabled {
            startSyncTimer()
        } else {
            stopSyncTimer()
        }
    }
    
    var connectionStatusText: String {
        if isConnected {
            return "Connected (\(peers.count) peers)"
        } else {
            return "Disconnected"
        }
    }
    
    var lastSyncText: String {
        guard let lastSyncTime = lastSyncTime else {
            return "Never synced"
        }
        
        let formatter = RelativeDateTimeFormatter()
        formatter.dateTimeStyle = .named
        return formatter.localizedString(for: lastSyncTime, relativeTo: Date())
    }
}