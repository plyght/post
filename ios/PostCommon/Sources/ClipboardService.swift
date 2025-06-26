import Foundation
import UIKit
import Combine

class ClipboardService: ObservableObject {
    @Published private(set) var currentContent: String?
    @Published private(set) var lastChangeTime: Date?
    
    private let pasteboard = UIPasteboard.general
    private var monitoringTimer: Timer?
    private var lastChangeCount: Int = 0
    private let maxContentSize: Int = 1_048_576 // 1MB
    
    private let contentSubject = PassthroughSubject<String?, Never>()
    
    var contentPublisher: AnyPublisher<String?, Never> {
        contentSubject.eraseToAnyPublisher()
    }
    
    init() {
        lastChangeCount = pasteboard.changeCount
        updateCurrentContent()
    }
    
    deinit {
        stopMonitoring()
    }
    
    func startMonitoring() async {
        await MainActor.run {
            stopMonitoring()
            
            monitoringTimer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
                self?.checkForClipboardChanges()
            }
            
            print("Started clipboard monitoring")
        }
    }
    
    func stopMonitoring() {
        monitoringTimer?.invalidate()
        monitoringTimer = nil
        print("Stopped clipboard monitoring")
    }
    
    private func checkForClipboardChanges() {
        let currentChangeCount = pasteboard.changeCount
        
        if currentChangeCount != lastChangeCount {
            lastChangeCount = currentChangeCount
            updateCurrentContent()
            lastChangeTime = Date()
        }
    }
    
    private func updateCurrentContent() {
        guard pasteboard.hasStrings else {
            if currentContent != nil {
                currentContent = nil
                contentSubject.send(nil)
            }
            return
        }
        
        let newContent = pasteboard.string
        
        if let content = newContent, !content.isEmpty {
            if content.count > maxContentSize {
                print("Clipboard content too large: \(content.count) bytes, max: \(maxContentSize)")
                return
            }
            
            if content != currentContent {
                currentContent = content
                contentSubject.send(content)
                print("Clipboard updated: \(content.prefix(50))...")
            }
        } else {
            if currentContent != nil {
                currentContent = nil
                contentSubject.send(nil)
            }
        }
    }
    
    func setClipboard(_ content: String) async {
        await MainActor.run {
            guard content.count <= maxContentSize else {
                print("Content too large to set in clipboard: \(content.count) bytes")
                return
            }
            
            pasteboard.string = content
            currentContent = content
            lastChangeCount = pasteboard.changeCount
            lastChangeTime = Date()
            contentSubject.send(content)
            
            print("Clipboard set programmatically: \(content.prefix(50))...")
        }
    }
    
    func clearClipboard() async {
        await MainActor.run {
            pasteboard.string = ""
            currentContent = nil
            lastChangeCount = pasteboard.changeCount
            lastChangeTime = Date()
            contentSubject.send(nil)
            
            print("Clipboard cleared")
        }
    }
    
    func getClipboardContent() -> String? {
        guard pasteboard.hasStrings else {
            return nil
        }
        
        let content = pasteboard.string
        
        if let content = content, !content.isEmpty && content.count <= maxContentSize {
            return content
        }
        
        return nil
    }
    
    func getClipboardMetadata() -> ClipboardMetadata {
        let hasContent = pasteboard.hasStrings
        let contentSize = pasteboard.string?.count ?? 0
        let contentType = determineContentType()
        
        return ClipboardMetadata(
            hasContent: hasContent,
            contentSize: contentSize,
            contentType: contentType,
            changeCount: pasteboard.changeCount,
            lastChanged: lastChangeTime ?? Date()
        )
    }
    
    private func determineContentType() -> ClipboardContentType {
        guard let content = pasteboard.string else {
            return .text
        }
        
        if isValidURL(content) {
            return .url
        }
        
        if pasteboard.hasImages {
            return .image
        }
        
        if containsHTML(content) {
            return .html
        }
        
        return .text
    }
    
    private func isValidURL(_ string: String) -> Bool {
        guard let url = URL(string: string) else {
            return false
        }
        
        return url.scheme != nil && (url.scheme == "http" || url.scheme == "https" || url.scheme == "ftp")
    }
    
    private func containsHTML(_ string: String) -> Bool {
        return string.contains("<") && string.contains(">") && 
               (string.lowercased().contains("<html") || 
                string.lowercased().contains("<div") || 
                string.lowercased().contains("<p>") ||
                string.lowercased().contains("<span"))
    }
    
    func copyToClipboard(_ content: ClipboardContent) async throws {
        guard content.content.count <= maxContentSize else {
            throw PostError.clipboardError("Content too large: \(content.content.count) bytes")
        }
        
        await setClipboard(content.content)
    }
    
    func getCurrentClipboardContent() throws -> ClipboardContent? {
        guard let content = getClipboardContent() else {
            return nil
        }
        
        let contentType = determineContentType()
        
        return ClipboardContent(
            content: content,
            contentType: contentType,
            nodeId: UIDevice.current.identifierForVendor?.uuidString ?? "unknown"
        )
    }
    
    var isMonitoring: Bool {
        return monitoringTimer?.isValid ?? false
    }
    
    var clipboardSize: Int {
        return pasteboard.string?.count ?? 0
    }
    
    var hasClipboardPermission: Bool {
        return pasteboard.hasStrings || pasteboard.hasImages || pasteboard.hasURLs
    }
}

struct ClipboardMetadata {
    let hasContent: Bool
    let contentSize: Int
    let contentType: ClipboardContentType
    let changeCount: Int
    let lastChanged: Date
    
    var formattedSize: String {
        let formatter = ByteCountFormatter()
        formatter.allowedUnits = [.useBytes, .useKB, .useMB]
        formatter.countStyle = .file
        return formatter.string(fromByteCount: Int64(contentSize))
    }
    
    var ageDescription: String {
        let formatter = RelativeDateTimeFormatter()
        formatter.dateTimeStyle = .named
        return formatter.localizedString(for: lastChanged, relativeTo: Date())
    }
}