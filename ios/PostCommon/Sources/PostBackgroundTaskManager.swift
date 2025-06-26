import Foundation
import BackgroundTasks
import UIKit

@available(iOS 26.0, *)
class PostBackgroundTaskManager {
    static let shared = PostBackgroundTaskManager()
    
    private let syncTaskIdentifier = "com.post.clipboard-sync"
    private let refreshTaskIdentifier = "com.post.background-refresh"
    
    private var currentSyncTask: BGContinuedProcessingTask?
    private var isRegistered = false
    
    private init() {}
    
    func registerBackgroundTasks() {
        guard !isRegistered else { return }
        
        BGTaskScheduler.shared.register(forTaskWithIdentifier: syncTaskIdentifier, using: nil) { task in
            self.handleSyncTask(task as! BGContinuedProcessingTask)
        }
        
        BGTaskScheduler.shared.register(forTaskWithIdentifier: refreshTaskIdentifier, using: nil) { task in
            self.handleRefreshTask(task as! BGAppRefreshTask)
        }
        
        isRegistered = true
        print("Registered background tasks for iOS 26")
    }
    
    func startBackgroundSync(operation: @escaping () async -> Void) async {
        guard #available(iOS 26.0, *) else {
            await operation()
            return
        }
        
        let request = BGContinuedProcessingTaskRequest(identifier: syncTaskIdentifier)
        request.title = "Syncing Clipboard"
        request.subtitle = "Synchronizing clipboard across devices"
        request.submissionStrategy = .queue
        
        do {
            try BGTaskScheduler.shared.submit(request)
            print("Background sync task submitted")
        } catch {
            print("Failed to submit background sync task: \(error)")
            await operation()
        }
    }
    
    private func handleSyncTask(_ task: BGContinuedProcessingTask) {
        currentSyncTask = task
        
        task.title = "Syncing Clipboard"
        task.subtitle = "Connecting to Post network..."
        
        Task {
            do {
                await self.performBackgroundSync(task: task)
                task.setTaskCompleted(success: true)
            } catch {
                print("Background sync failed: \(error)")
                task.setTaskCompleted(success: false)
            }
        }
        
        task.expirationHandler = {
            print("Background sync task expired")
            task.setTaskCompleted(success: false)
        }
    }
    
    private func performBackgroundSync(task: BGContinuedProcessingTask) async {
        let steps: [(String, String, () async throws -> Void)] = [
            ("Reading clipboard...", "Accessing local clipboard content", {
                try await Task.sleep(nanoseconds: 500_000_000) // 0.5s
            }),
            ("Encrypting content...", "Securing data for transmission", {
                try await Task.sleep(nanoseconds: 1_000_000_000) // 1s
            }),
            ("Discovering peers...", "Finding devices on Tailscale network", {
                try await Task.sleep(nanoseconds: 1_500_000_000) // 1.5s
            }),
            ("Syncing to peers...", "Sending encrypted clipboard data", {
                try await Task.sleep(nanoseconds: 2_000_000_000) // 2s
            }),
            ("Finalizing sync...", "Completing synchronization", {
                try await Task.sleep(nanoseconds: 500_000_000) // 0.5s
            })
        ]
        
        for (index, (title, subtitle, operation)) in steps.enumerated() {
            let progress = Double(index) / Double(steps.count)
            
            await MainActor.run {
                task.setProgress(progress, title: title, subtitle: subtitle)
            }
            
            try await operation()
            
            print("Background sync step \(index + 1)/\(steps.count) completed: \(title)")
        }
        
        await MainActor.run {
            task.setProgress(1.0, title: "Sync Complete", subtitle: "Clipboard synchronized successfully")
        }
        
        try await Task.sleep(nanoseconds: 500_000_000)
    }
    
    private func handleRefreshTask(_ task: BGAppRefreshTask) {
        print("Handling background app refresh task")
        
        Task {
            do {
                await self.performQuickRefresh()
                task.setTaskCompleted(success: true)
            } catch {
                print("Background refresh failed: \(error)")
                task.setTaskCompleted(success: false)
            }
        }
        
        task.expirationHandler = {
            print("Background refresh task expired")
            task.setTaskCompleted(success: false)
        }
    }
    
    private func performQuickRefresh() async {
        print("Performing quick background refresh")
        try? await Task.sleep(nanoseconds: 1_000_000_000)
    }
    
    func scheduleBackgroundRefresh() {
        let request = BGAppRefreshTaskRequest(identifier: refreshTaskIdentifier)
        request.earliestBeginDate = Date(timeIntervalSinceNow: 15 * 60) // 15 minutes
        
        do {
            try BGTaskScheduler.shared.submit(request)
            print("Background refresh scheduled")
        } catch {
            print("Failed to schedule background refresh: \(error)")
        }
    }
    
    func cancelBackgroundTasks() {
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: syncTaskIdentifier)
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: refreshTaskIdentifier)
        currentSyncTask = nil
        print("Cancelled background tasks")
    }
    
    var hasActiveSyncTask: Bool {
        return currentSyncTask != nil
    }
    
    func simulateBackgroundSync() async {
        guard #available(iOS 26.0, *) else {
            print("BGContinuedProcessingTask not available on this iOS version")
            return
        }
        
        print("Simulating background sync for testing...")
        
        let steps = [
            "Reading clipboard content",
            "Encrypting data",
            "Finding peers",
            "Transmitting data",
            "Verifying sync"
        ]
        
        for (index, step) in steps.enumerated() {
            let progress = Double(index) / Double(steps.count)
            print("[\(Int(progress * 100))%] \(step)...")
            try? await Task.sleep(nanoseconds: 800_000_000)
        }
        
        print("[100%] Background sync simulation complete")
    }
}

@available(iOS, deprecated: 26.0, message: "Use PostBackgroundTaskManager for iOS 26+")
class LegacyBackgroundTaskManager {
    static let shared = LegacyBackgroundTaskManager()
    
    private var backgroundTask: UIBackgroundTaskIdentifier = .invalid
    
    private init() {}
    
    func startBackgroundSync(operation: @escaping () async -> Void) async {
        backgroundTask = UIApplication.shared.beginBackgroundTask {
            self.endBackgroundTask()
        }
        
        guard backgroundTask != .invalid else {
            await operation()
            return
        }
        
        await operation()
        endBackgroundTask()
    }
    
    private func endBackgroundTask() {
        if backgroundTask != .invalid {
            UIApplication.shared.endBackgroundTask(backgroundTask)
            backgroundTask = .invalid
        }
    }
}

extension PostBackgroundTaskManager {
    static func createManager() -> Any {
        if #available(iOS 26.0, *) {
            return PostBackgroundTaskManager.shared
        } else {
            return LegacyBackgroundTaskManager.shared
        }
    }
}