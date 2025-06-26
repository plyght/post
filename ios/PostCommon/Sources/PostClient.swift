import Foundation
import Combine
import Network

class PostClient: ObservableObject {
    @Published private(set) var isConnected = false
    @Published private(set) var peers: [PostPeer] = []
    @Published private(set) var lastError: PostError?
    
    private let configuration: PostConfiguration
    private let cryptoService: CryptoService
    private let tailscaleService: TailscaleService
    private let urlSession: URLSession
    private let monitor = NWPathMonitor()
    
    private var cancellables = Set<AnyCancellable>()
    private var activeDaemons: [String: URL] = [:]
    
    private let isConnectedSubject = CurrentValueSubject<Bool, Never>(false)
    private let peersSubject = CurrentValueSubject<[PostPeer], Never>([])
    private let errorSubject = PassthroughSubject<PostError?, Never>()
    
    var isConnectedPublisher: AnyPublisher<Bool, Never> {
        isConnectedSubject.eraseToAnyPublisher()
    }
    
    var peersPublisher: AnyPublisher<[PostPeer], Never> {
        peersSubject.eraseToAnyPublisher()
    }
    
    var errorPublisher: AnyPublisher<PostError?, Never> {
        errorSubject.eraseToAnyPublisher()
    }
    
    init(configuration: PostConfiguration) {
        self.configuration = configuration
        self.cryptoService = CryptoService()
        self.tailscaleService = TailscaleService()
        
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 10.0
        config.timeoutIntervalForResource = 30.0
        self.urlSession = URLSession(configuration: config)
        
        setupNetworkMonitoring()
    }
    
    deinit {
        disconnect()
        monitor.cancel()
        cancellables.removeAll()
    }
    
    private func setupNetworkMonitoring() {
        monitor.pathUpdateHandler = { [weak self] path in
            DispatchQueue.main.async {
                if path.status == .satisfied {
                    self?.handleNetworkAvailable()
                } else {
                    self?.handleNetworkUnavailable()
                }
            }
        }
        
        let queue = DispatchQueue(label: "NetworkMonitor")
        monitor.start(queue: queue)
    }
    
    private func handleNetworkAvailable() {
        print("Network became available")
        Task {
            await connect()
        }
    }
    
    private func handleNetworkUnavailable() {
        print("Network became unavailable")
        disconnect()
    }
    
    func connect() async {
        do {
            await cryptoService.initializeKeys()
            let status = try await tailscaleService.getStatus()
            
            await discoverPostDaemons(from: status)
            
            if !activeDaemons.isEmpty {
                isConnected = true
                isConnectedSubject.send(true)
                await refreshPeers()
                print("Connected to Post network with \(activeDaemons.count) daemons")
            } else {
                throw PostError.networkError("No Post daemons found on Tailscale network")
            }
            
        } catch {
            let postError = error as? PostError ?? .networkError(error.localizedDescription)
            lastError = postError
            errorSubject.send(postError)
            isConnected = false
            isConnectedSubject.send(false)
            print("Failed to connect: \(error)")
        }
    }
    
    private func discoverPostDaemons(from status: TailscaleStatus) async {
        activeDaemons.removeAll()
        
        var candidateDevices = status.peers
        candidateDevices.append(status.self)
        
        await withTaskGroup(of: (String, URL?).self) { group in
            for device in candidateDevices {
                guard let ipAddress = device.tailscaleIPs.first else { continue }
                
                group.addTask {
                    let url = URL(string: "http://\(ipAddress):\(self.configuration.networkPort)")!
                    let isReachable = await self.checkPostDaemon(at: url)
                    return (device.id, isReachable ? url : nil)
                }
            }
            
            for await (deviceId, url) in group {
                if let url = url {
                    activeDaemons[deviceId] = url
                }
            }
        }
    }
    
    private func checkPostDaemon(at url: URL) async -> Bool {
        do {
            let statusURL = url.appendingPathComponent("api/v1/status")
            let (_, response) = try await urlSession.data(from: statusURL)
            
            if let httpResponse = response as? HTTPURLResponse {
                return httpResponse.statusCode == 200
            }
            
            return false
        } catch {
            return false
        }
    }
    
    func syncClipboard(_ content: ClipboardContent) async {
        guard isConnected, !activeDaemons.isEmpty else {
            errorSubject.send(.networkError("Not connected to Post network"))
            return
        }
        
        do {
            let encryptedContent = try await encryptContent(content)
            await broadcastToAllDaemons(encryptedContent, endpoint: "api/v1/clipboard/sync")
            print("Clipboard synced to \(activeDaemons.count) daemons")
        } catch {
            let postError = error as? PostError ?? .networkError(error.localizedDescription)
            errorSubject.send(postError)
        }
    }
    
    func sendClipboardToPeer(_ peer: PostPeer, content: ClipboardContent) async {
        guard let daemonURL = activeDaemons[peer.id] else {
            errorSubject.send(.networkError("Daemon not found for peer \(peer.name)"))
            return
        }
        
        do {
            let encryptedContent = try await encryptContent(content)
            await sendToDaemon(encryptedContent, at: daemonURL, endpoint: "api/v1/clipboard/sync")
            print("Clipboard sent to peer: \(peer.name)")
        } catch {
            let postError = error as? PostError ?? .networkError(error.localizedDescription)
            errorSubject.send(postError)
        }
    }
    
    private func encryptContent(_ content: ClipboardContent) async throws -> PostMessage {
        let jsonData = try JSONEncoder().encode(content)
        let jsonString = String(data: jsonData, encoding: .utf8)!
        
        let encryptedContent = try cryptoService.encrypt(jsonString, for: configuration.nodeId)
        let signature = try cryptoService.sign(jsonData)
        
        return PostMessage(
            nodeId: configuration.nodeId,
            contentType: content.contentType.rawValue,
            encryptedContent: encryptedContent,
            signature: signature.base64EncodedString()
        )
    }
    
    private func broadcastToAllDaemons(_ message: PostMessage, endpoint: String) async {
        await withTaskGroup(of: Void.self) { group in
            for (_, daemonURL) in activeDaemons {
                group.addTask {
                    await self.sendToDaemon(message, at: daemonURL, endpoint: endpoint)
                }
            }
        }
    }
    
    private func sendToDaemon(_ message: PostMessage, at daemonURL: URL, endpoint: String) async {
        do {
            let url = daemonURL.appendingPathComponent(endpoint)
            var request = URLRequest(url: url)
            request.httpMethod = "POST"
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            
            let jsonData = try JSONEncoder().encode(message)
            request.httpBody = jsonData
            
            let (_, response) = try await urlSession.data(for: request)
            
            if let httpResponse = response as? HTTPURLResponse,
               httpResponse.statusCode != 200 {
                print("Daemon request failed with status: \(httpResponse.statusCode)")
            }
            
        } catch {
            print("Failed to send to daemon at \(daemonURL): \(error)")
        }
    }
    
    func refreshPeers() async {
        do {
            let status = try await tailscaleService.getStatus()
            let discoveredPeers = status.peers.map { device in
                PostPeer(
                    id: device.id,
                    name: device.hostName,
                    ipAddress: device.tailscaleIPs.first ?? "",
                    publicKey: device.publicKey,
                    isOnline: device.online ?? false
                )
            }
            
            peers = discoveredPeers
            peersSubject.send(discoveredPeers)
            
        } catch {
            let postError = error as? PostError ?? .tailscaleError(error.localizedDescription)
            errorSubject.send(postError)
        }
    }
    
    func pullClipboard() async -> ClipboardContent? {
        guard isConnected, let firstDaemon = activeDaemons.values.first else {
            return nil
        }
        
        do {
            let url = firstDaemon.appendingPathComponent("api/v1/clipboard/pull")
            let (data, response) = try await urlSession.data(from: url)
            
            if let httpResponse = response as? HTTPURLResponse,
               httpResponse.statusCode == 200 {
                
                let message = try JSONDecoder().decode(PostMessage.self, from: data)
                let decryptedContent = try await cryptoService.decrypt(message.encryptedContent)
                let clipboardData = try JSONDecoder().decode(ClipboardContent.self, from: decryptedContent.data(using: .utf8)!)
                
                return clipboardData
            }
            
        } catch {
            print("Failed to pull clipboard: \(error)")
        }
        
        return nil
    }
    
    func disconnect() {
        isConnected = false
        isConnectedSubject.send(false)
        activeDaemons.removeAll()
        peers.removeAll()
        peersSubject.send([])
        print("Disconnected from Post network")
    }
    
    func performHandshake(with peer: PostPeer) async throws {
        guard let daemonURL = activeDaemons[peer.id] else {
            throw PostError.networkError("Daemon not found for peer")
        }
        
        let publicKey = try cryptoService.getPublicKey()
        let signingPublicKey = try cryptoService.getSigningPublicKey()
        
        let handshakeData = [
            "public_key": publicKey.base64EncodedString(),
            "signing_key": signingPublicKey.base64EncodedString(),
            "node_id": configuration.nodeId
        ]
        
        let url = daemonURL.appendingPathComponent("api/v1/auth/handshake")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        
        let jsonData = try JSONSerialization.data(withJSONObject: handshakeData)
        request.httpBody = jsonData
        
        let (responseData, response) = try await urlSession.data(for: request)
        
        if let httpResponse = response as? HTTPURLResponse,
           httpResponse.statusCode == 200 {
            
            if let responseJson = try JSONSerialization.jsonObject(with: responseData) as? [String: String],
               let peerPublicKeyString = responseJson["public_key"],
               let peerPublicKeyData = Data(base64Encoded: peerPublicKeyString) {
                
                try cryptoService.establishSharedSecret(with: peerPublicKeyData, peerId: peer.id)
                print("Handshake completed with peer: \(peer.name)")
            }
        } else {
            throw PostError.authenticationError("Handshake failed")
        }
    }
    
    var connectionInfo: String {
        if isConnected {
            return "Connected to \(activeDaemons.count) Post daemons (\(peers.count) peers)"
        } else {
            return "Disconnected"
        }
    }
}