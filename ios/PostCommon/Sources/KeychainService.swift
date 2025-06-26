import Foundation
import Security

class KeychainService {
    private let service = "com.post.clipboard"
    
    func saveKey(_ keyData: Data, forKey key: String) throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
            kSecValueData as String: keyData,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        ]
        
        let status = SecItemAdd(query as CFDictionary, nil)
        
        if status == errSecDuplicateItem {
            try updateKey(keyData, forKey: key)
        } else if status != errSecSuccess {
            throw PostError.encryptionError("Failed to save key to keychain: \(status)")
        }
    }
    
    func loadKey(_ key: String) throws -> Data? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]
        
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        
        if status == errSecItemNotFound {
            return nil
        } else if status == errSecSuccess {
            return result as? Data
        } else {
            throw PostError.encryptionError("Failed to load key from keychain: \(status)")
        }
    }
    
    private func updateKey(_ keyData: Data, forKey key: String) throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key
        ]
        
        let updateAttributes: [String: Any] = [
            kSecValueData as String: keyData
        ]
        
        let status = SecItemUpdate(query as CFDictionary, updateAttributes as CFDictionary)
        
        if status != errSecSuccess {
            throw PostError.encryptionError("Failed to update key in keychain: \(status)")
        }
    }
    
    func deleteKey(_ key: String) throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key
        ]
        
        let status = SecItemDelete(query as CFDictionary)
        
        if status != errSecSuccess && status != errSecItemNotFound {
            throw PostError.encryptionError("Failed to delete key from keychain: \(status)")
        }
    }
    
    func saveData(_ data: Data, forKey key: String) throws {
        try saveKey(data, forKey: key)
    }
    
    func loadData(_ key: String) throws -> Data? {
        return try loadKey(key)
    }
    
    func deleteData(_ key: String) throws {
        try deleteKey(key)
    }
    
    func keyExists(_ key: String) -> Bool {
        do {
            return try loadKey(key) != nil
        } catch {
            return false
        }
    }
    
    func generateAndSaveRandomKey(forKey key: String, size: Int = 32) throws -> Data {
        var keyData = Data(count: size)
        let result = keyData.withUnsafeMutableBytes { bytes in
            SecRandomCopyBytes(kSecRandomDefault, size, bytes.bindMemory(to: UInt8.self).baseAddress!)
        }
        
        guard result == errSecSuccess else {
            throw PostError.encryptionError("Failed to generate random key")
        }
        
        try saveKey(keyData, forKey: key)
        return keyData
    }
    
    func saveSecureNote(_ note: String, forKey key: String) throws {
        guard let noteData = note.data(using: .utf8) else {
            throw PostError.encryptionError("Failed to encode note as UTF-8")
        }
        
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
            kSecValueData as String: noteData,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        ]
        
        let status = SecItemAdd(query as CFDictionary, nil)
        
        if status == errSecDuplicateItem {
            let updateQuery: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: service,
                kSecAttrAccount as String: key
            ]
            
            let updateAttributes: [String: Any] = [
                kSecValueData as String: noteData
            ]
            
            let updateStatus = SecItemUpdate(updateQuery as CFDictionary, updateAttributes as CFDictionary)
            if updateStatus != errSecSuccess {
                throw PostError.encryptionError("Failed to update secure note: \(updateStatus)")
            }
        } else if status != errSecSuccess {
            throw PostError.encryptionError("Failed to save secure note: \(status)")
        }
    }
    
    func loadSecureNote(_ key: String) throws -> String? {
        guard let data = try loadKey(key) else {
            return nil
        }
        
        return String(data: data, encoding: .utf8)
    }
}