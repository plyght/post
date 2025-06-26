import Foundation

struct PostPeer: Identifiable, Codable {
    let id: String
    let name: String
    let ipAddress: String
    let publicKey: String
    let isOnline: Bool
    let lastSeen: Date
    
    init(id: String, name: String, ipAddress: String, publicKey: String, isOnline: Bool = false, lastSeen: Date = Date()) {
        self.id = id
        self.name = name
        self.ipAddress = ipAddress
        self.publicKey = publicKey
        self.isOnline = isOnline
        self.lastSeen = lastSeen
    }
}

struct ClipboardContent: Codable {
    let id: String
    let content: String
    let contentType: ClipboardContentType
    let timestamp: Date
    let nodeId: String
    let signature: String?
    
    init(content: String, contentType: ClipboardContentType, nodeId: String, signature: String? = nil) {
        self.id = UUID().uuidString
        self.content = content
        self.contentType = contentType
        self.timestamp = Date()
        self.nodeId = nodeId
        self.signature = signature
    }
}

enum ClipboardContentType: String, Codable, CaseIterable {
    case text = "text/plain"
    case url = "text/uri-list"
    case html = "text/html"
    case image = "image/png"
    case file = "application/octet-stream"
    
    var displayName: String {
        switch self {
        case .text: return "Text"
        case .url: return "URL"
        case .html: return "HTML"
        case .image: return "Image"
        case .file: return "File"
        }
    }
}

struct PostMessage: Codable {
    let nodeId: String
    let timestamp: Date
    let contentType: String
    let encryptedContent: String
    let signature: String
    
    init(nodeId: String, contentType: String, encryptedContent: String, signature: String) {
        self.nodeId = nodeId
        self.timestamp = Date()
        self.contentType = contentType
        self.encryptedContent = encryptedContent
        self.signature = signature
    }
}

struct PostConfiguration: Codable {
    let nodeId: String
    let syncInterval: TimeInterval
    let maxContentSize: Int
    let enableBackgroundSync: Bool
    let requireBiometricAuth: Bool
    let tailscaleSocketPath: String?
    let networkPort: Int
    
    static let `default` = PostConfiguration(
        nodeId: UIDevice.current.identifierForVendor?.uuidString ?? UUID().uuidString,
        syncInterval: 5.0,
        maxContentSize: 1_048_576, // 1MB
        enableBackgroundSync: true,
        requireBiometricAuth: false,
        tailscaleSocketPath: nil,
        networkPort: 8413  // HTTP API port (TCP P2P port + 1)
    )
}

struct TailscaleStatus: Codable {
    let backendState: String
    let authURL: String?
    let user: TailscaleUser?
    let self: TailscaleDevice
    let peers: [TailscaleDevice]
    
    struct TailscaleUser: Codable {
        let id: Int
        let loginName: String
        let displayName: String
    }
    
    struct TailscaleDevice: Codable {
        let id: String
        let publicKey: String
        let hostName: String
        let dnsName: String
        let os: String
        let tailscaleIPs: [String]
        let relay: String?
        let rxBytes: Int?
        let txBytes: Int?
        let created: String
        let lastSeen: String?
        let online: Bool?
        let exitNode: Bool?
        let exitNodeOption: Bool?
        let active: Bool?
        
        enum CodingKeys: String, CodingKey {
            case id = "ID"
            case publicKey = "PublicKey"
            case hostName = "HostName"
            case dnsName = "DNSName"
            case os = "OS"
            case tailscaleIPs = "TailscaleIPs"
            case relay = "Relay"
            case rxBytes = "RxBytes"
            case txBytes = "TxBytes"
            case created = "Created"
            case lastSeen = "LastSeen"
            case online = "Online"
            case exitNode = "ExitNode"
            case exitNodeOption = "ExitNodeOption"
            case active = "Active"
        }
    }
}

enum PostError: Error, LocalizedError {
    case networkError(String)
    case encryptionError(String)
    case clipboardError(String)
    case configurationError(String)
    case authenticationError(String)
    case tailscaleError(String)
    
    var errorDescription: String? {
        switch self {
        case .networkError(let message):
            return "Network error: \(message)"
        case .encryptionError(let message):
            return "Encryption error: \(message)"
        case .clipboardError(let message):
            return "Clipboard error: \(message)"
        case .configurationError(let message):
            return "Configuration error: \(message)"
        case .authenticationError(let message):
            return "Authentication error: \(message)"
        case .tailscaleError(let message):
            return "Tailscale error: \(message)"
        }
    }
}

struct NetworkRequest: Codable {
    let method: String
    let path: String
    let headers: [String: String]
    let body: Data?
    
    init(method: String, path: String, headers: [String: String] = [:], body: Data? = nil) {
        self.method = method
        self.path = path
        self.headers = headers
        self.body = body
    }
}

struct NetworkResponse: Codable {
    let statusCode: Int
    let headers: [String: String]
    let data: Data
    
    var isSuccess: Bool {
        return (200...299).contains(statusCode)
    }
}