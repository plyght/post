import Foundation
import Network

class TailscaleService {
    private let localAPIURL = URL(string: "http://100.115.92.1:41641/localapi/v0")!
    private let urlSession: URLSession
    private var isMonitoring = false
    
    init() {
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 5.0
        config.timeoutIntervalForResource = 10.0
        self.urlSession = URLSession(configuration: config)
    }
    
    func startMonitoring() async {
        guard !isMonitoring else { return }
        isMonitoring = true
        print("Started Tailscale monitoring")
    }
    
    func stopMonitoring() {
        isMonitoring = false
        print("Stopped Tailscale monitoring")
    }
    
    func getStatus() async throws -> TailscaleStatus {
        let statusURL = localAPIURL.appendingPathComponent("status")
        
        do {
            let (data, response) = try await urlSession.data(from: statusURL)
            
            guard let httpResponse = response as? HTTPURLResponse else {
                throw PostError.tailscaleError("Invalid response from Tailscale API")
            }
            
            guard httpResponse.statusCode == 200 else {
                throw PostError.tailscaleError("Tailscale API returned status \(httpResponse.statusCode)")
            }
            
            let decoder = JSONDecoder()
            decoder.dateDecodingStrategy = .iso8601
            
            let status = try decoder.decode(TailscaleStatus.self, from: data)
            print("Retrieved Tailscale status: \(status.peers.count) peers, backend state: \(status.backendState)")
            
            return status
            
        } catch {
            if error is PostError {
                throw error
            } else {
                throw PostError.tailscaleError("Failed to get Tailscale status: \(error.localizedDescription)")
            }
        }
    }
    
    func isConnected() async -> Bool {
        do {
            let status = try await getStatus()
            return status.backendState == "Running" || status.backendState == "Connected"
        } catch {
            print("Failed to check Tailscale connection: \(error)")
            return false
        }
    }
    
    func getCurrentDevice() async throws -> TailscaleStatus.TailscaleDevice {
        let status = try await getStatus()
        return status.self
    }
    
    func getPeers() async throws -> [TailscaleStatus.TailscaleDevice] {
        let status = try await getStatus()
        return status.peers
    }
    
    func getOnlinePeers() async throws -> [TailscaleStatus.TailscaleDevice] {
        let peers = try await getPeers()
        return peers.filter { $0.online == true }
    }
    
    func findPeerByHostname(_ hostname: String) async throws -> TailscaleStatus.TailscaleDevice? {
        let peers = try await getPeers()
        return peers.first { $0.hostName.lowercased() == hostname.lowercased() }
    }
    
    func findPeerByIP(_ ipAddress: String) async throws -> TailscaleStatus.TailscaleDevice? {
        let peers = try await getPeers()
        return peers.first { $0.tailscaleIPs.contains(ipAddress) }
    }
    
    func validateTailscaleAccess() async -> TailscaleValidationResult {
        do {
            let status = try await getStatus()
            
            if status.backendState == "NeedsLogin" {
                return .needsLogin(authURL: status.authURL)
            }
            
            if status.backendState == "NoState" || status.backendState == "Stopped" {
                return .notRunning
            }
            
            if status.backendState == "Running" || status.backendState == "Connected" {
                return .connected(peerCount: status.peers.count)
            }
            
            return .unknown(state: status.backendState)
            
        } catch let error as PostError {
            return .error(error)
        } catch {
            return .error(.tailscaleError(error.localizedDescription))
        }
    }
    
    func getNetworkInfo() async throws -> TailscaleNetworkInfo {
        let status = try await getStatus()
        let currentDevice = status.self
        
        let onlinePeers = status.peers.filter { $0.online == true }
        let totalPeers = status.peers.count
        
        let networkMap = status.peers.reduce(into: [String: [String]]()) { result, peer in
            let region = extractRegion(from: peer.relay ?? "unknown")
            if result[region] == nil {
                result[region] = []
            }
            result[region]?.append(peer.hostName)
        }
        
        return TailscaleNetworkInfo(
            currentDevice: currentDevice,
            totalPeers: totalPeers,
            onlinePeers: onlinePeers.count,
            networkMap: networkMap,
            backendState: status.backendState
        )
    }
    
    private func extractRegion(from relay: String) -> String {
        let components = relay.components(separatedBy: "-")
        return components.first?.capitalized ?? "Unknown"
    }
    
    func pingPeer(_ peer: TailscaleStatus.TailscaleDevice) async -> PingResult {
        guard let ipAddress = peer.tailscaleIPs.first else {
            return .failed("No IP address available")
        }
        
        let url = URL(string: "http://\(ipAddress):41641/localapi/v0/ping")!
        
        do {
            let startTime = Date()
            let (_, response) = try await urlSession.data(from: url)
            let endTime = Date()
            
            if let httpResponse = response as? HTTPURLResponse,
               httpResponse.statusCode == 200 {
                let latency = endTime.timeIntervalSince(startTime) * 1000
                return .success(latencyMs: latency)
            } else {
                return .failed("HTTP error")
            }
        } catch {
            return .failed(error.localizedDescription)
        }
    }
    
    var isAvailable: Bool {
        return isMonitoring
    }
}

enum TailscaleValidationResult {
    case connected(peerCount: Int)
    case needsLogin(authURL: String?)
    case notRunning
    case unknown(state: String)
    case error(PostError)
    
    var isValid: Bool {
        switch self {
        case .connected:
            return true
        default:
            return false
        }
    }
    
    var description: String {
        switch self {
        case .connected(let peerCount):
            return "Connected with \(peerCount) peers"
        case .needsLogin:
            return "Authentication required"
        case .notRunning:
            return "Tailscale not running"
        case .unknown(let state):
            return "Unknown state: \(state)"
        case .error(let error):
            return "Error: \(error.localizedDescription)"
        }
    }
}

struct TailscaleNetworkInfo {
    let currentDevice: TailscaleStatus.TailscaleDevice
    let totalPeers: Int
    let onlinePeers: Int
    let networkMap: [String: [String]]
    let backendState: String
    
    var healthStatus: String {
        if onlinePeers == 0 {
            return "No peers online"
        } else if onlinePeers < totalPeers / 2 {
            return "Limited connectivity"
        } else {
            return "Good connectivity"
        }
    }
}

enum PingResult {
    case success(latencyMs: Double)
    case failed(String)
    
    var description: String {
        switch self {
        case .success(let latency):
            return String(format: "%.1f ms", latency)
        case .failed(let reason):
            return "Failed: \(reason)"
        }
    }
}