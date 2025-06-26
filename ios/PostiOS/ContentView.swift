import SwiftUI

struct ContentView: View {
    @State private var selectedTab = 0
    
    var body: some View {
        TabView(selection: $selectedTab) {
            StatusView()
                .tabItem {
                    Image(systemName: "clipboard")
                    Text("Status")
                }
                .tag(0)
            
            PeersView()
                .tabItem {
                    Image(systemName: "network")
                    Text("Peers")
                }
                .tag(1)
            
            SettingsView()
                .tabItem {
                    Image(systemName: "gear")
                    Text("Settings")
                }
                .tag(2)
        }
        .tint(.blue)
    }
}

struct StatusView: View {
    @State private var isConnected = false
    @State private var clipboardContent = "Sample clipboard content..."
    
    var body: some View {
        NavigationView {
            VStack(spacing: 20) {
                ConnectionStatusCard(isConnected: isConnected)
                ClipboardPreviewCard(content: clipboardContent)
                
                Button("Sync Clipboard Now") {
                    // TODO: Implement clipboard sync
                }
                .buttonStyle(.borderedProminent)
                
                Spacer()
            }
            .padding()
            .navigationTitle("Post")
        }
    }
}

struct ConnectionStatusCard: View {
    let isConnected: Bool
    
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Image(systemName: isConnected ? "wifi" : "wifi.slash")
                    .foregroundColor(isConnected ? .green : .red)
                Text("Connection Status")
                    .font(.headline)
                Spacer()
            }
            
            Text(isConnected ? "Connected to Post network" : "Disconnected")
                .font(.subheadline)
                .foregroundColor(.secondary)
            
            Text("Last sync: Never")
                .font(.caption)
                .foregroundColor(.secondary)
        }
        .padding()
        .background(Color(.systemGray6))
        .cornerRadius(12)
    }
}

struct ClipboardPreviewCard: View {
    let content: String
    
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Image(systemName: "doc.on.clipboard")
                Text("Current Clipboard")
                    .font(.headline)
                Spacer()
            }
            
            Text(content.prefix(100) + (content.count > 100 ? "..." : ""))
                .font(.body)
                .lineLimit(3)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding()
        .background(Color(.systemGray6))
        .cornerRadius(12)
    }
}

struct PeersView: View {
    @State private var peers: [MockPeer] = [
        MockPeer(name: "MacBook Pro", ipAddress: "100.64.1.2", isOnline: true),
        MockPeer(name: "Desktop PC", ipAddress: "100.64.1.3", isOnline: false)
    ]
    
    var body: some View {
        NavigationView {
            List {
                ForEach(peers) { peer in
                    PeerRow(peer: peer)
                }
            }
            .navigationTitle("Peers")
            .refreshable {
                // TODO: Implement peer refresh
            }
        }
    }
}

struct MockPeer: Identifiable {
    let id = UUID()
    let name: String
    let ipAddress: String
    let isOnline: Bool
}

struct PeerRow: View {
    let peer: MockPeer
    
    var body: some View {
        HStack {
            VStack(alignment: .leading) {
                Text(peer.name)
                    .font(.headline)
                Text(peer.ipAddress)
                    .font(.caption)
                    .foregroundColor(.secondary)
            }
            
            Spacer()
            
            Circle()
                .fill(peer.isOnline ? Color.green : Color.red)
                .frame(width: 12, height: 12)
        }
        .padding(.vertical, 4)
    }
}

struct SettingsView: View {
    @State private var autoSyncEnabled = true
    @State private var biometricAuthEnabled = false
    
    var body: some View {
        NavigationView {
            Form {
                Section("Sync Settings") {
                    Toggle("Auto Sync", isOn: $autoSyncEnabled)
                    
                    HStack {
                        Text("Sync Interval")
                        Spacer()
                        Text("5 seconds")
                            .foregroundColor(.secondary)
                    }
                }
                
                Section("Security") {
                    Toggle("Biometric Authentication", isOn: $biometricAuthEnabled)
                    
                    Button("Reset Encryption Keys") {
                        // TODO: Implement key reset
                    }
                    .foregroundColor(.red)
                }
                
                Section("About") {
                    HStack {
                        Text("Version")
                        Spacer()
                        Text("1.0.0")
                            .foregroundColor(.secondary)
                    }
                    
                    Link("Privacy Policy", destination: URL(string: "https://example.com/privacy")!)
                    Link("Support", destination: URL(string: "https://example.com/support")!)
                }
            }
            .navigationTitle("Settings")
        }
    }
}

#Preview {
    ContentView()
}