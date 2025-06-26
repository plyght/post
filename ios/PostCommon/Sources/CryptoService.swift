import Foundation
import CryptoKit
import Security

class CryptoService {
    private let keychain = KeychainService()
    private var privateKey: Curve25519.KeyAgreement.PrivateKey?
    private var signingKey: Curve25519.Signing.PrivateKey?
    private var sharedSecrets: [String: SymmetricKey] = [:]
    
    private let keyRotationInterval: TimeInterval = 24 * 60 * 60 // 24 hours
    private let pbkdf2Rounds = 100_000
    
    private struct KeychainKeys {
        static let privateKey = "com.post.privatekey"
        static let signingKey = "com.post.signingkey"
        static let lastKeyRotation = "com.post.lastkeyrotation"
    }
    
    init() {}
    
    func initializeKeys() async {
        do {
            if let existingPrivateKey = try keychain.loadKey(KeychainKeys.privateKey),
               let existingSigningKey = try keychain.loadKey(KeychainKeys.signingKey) {
                
                self.privateKey = try Curve25519.KeyAgreement.PrivateKey(rawRepresentation: existingPrivateKey)
                self.signingKey = try Curve25519.Signing.PrivateKey(rawRepresentation: existingSigningKey)
                
                await checkKeyRotation()
            } else {
                await generateNewKeys()
            }
        } catch {
            print("Failed to initialize keys: \(error)")
            await generateNewKeys()
        }
    }
    
    private func generateNewKeys() async {
        do {
            let newPrivateKey = Curve25519.KeyAgreement.PrivateKey()
            let newSigningKey = Curve25519.Signing.PrivateKey()
            
            try keychain.saveKey(newPrivateKey.rawRepresentation, forKey: KeychainKeys.privateKey)
            try keychain.saveKey(newSigningKey.rawRepresentation, forKey: KeychainKeys.signingKey)
            try keychain.saveData(Date().timeIntervalSince1970.description.data(using: .utf8)!, forKey: KeychainKeys.lastKeyRotation)
            
            self.privateKey = newPrivateKey
            self.signingKey = newSigningKey
            
            sharedSecrets.removeAll()
            
            print("Generated new encryption keys")
        } catch {
            print("Failed to generate new keys: \(error)")
        }
    }
    
    private func checkKeyRotation() async {
        do {
            if let lastRotationData = try keychain.loadData(KeychainKeys.lastKeyRotation),
               let lastRotationString = String(data: lastRotationData, encoding: .utf8),
               let lastRotationTime = TimeInterval(lastRotationString) {
                
                let timeSinceRotation = Date().timeIntervalSince1970 - lastRotationTime
                if timeSinceRotation > keyRotationInterval {
                    await generateNewKeys()
                }
            }
        } catch {
            print("Failed to check key rotation: \(error)")
        }
    }
    
    func getPublicKey() throws -> Data {
        guard let privateKey = privateKey else {
            throw PostError.encryptionError("Private key not initialized")
        }
        return privateKey.publicKey.rawRepresentation
    }
    
    func getSigningPublicKey() throws -> Data {
        guard let signingKey = signingKey else {
            throw PostError.encryptionError("Signing key not initialized")
        }
        return signingKey.publicKey.rawRepresentation
    }
    
    func establishSharedSecret(with peerPublicKey: Data, peerId: String) throws {
        guard let privateKey = privateKey else {
            throw PostError.encryptionError("Private key not initialized")
        }
        
        let peerKey = try Curve25519.KeyAgreement.PublicKey(rawRepresentation: peerPublicKey)
        let sharedSecret = try privateKey.sharedSecretFromKeyAgreement(with: peerKey)
        
        let symmetricKey = sharedSecret.hkdfDerivedSymmetricKey(
            using: SHA256.self,
            salt: "post-clipboard-sync".data(using: .utf8)!,
            sharedInfo: peerId.data(using: .utf8)!,
            outputByteCount: 32
        )
        
        sharedSecrets[peerId] = symmetricKey
    }
    
    func encrypt(_ data: String, for peerId: String) throws -> String {
        guard let sharedSecret = sharedSecrets[peerId] else {
            throw PostError.encryptionError("No shared secret established with peer \(peerId)")
        }
        
        let plaintext = data.data(using: .utf8)!
        let nonce = ChaChaPoly.Nonce()
        
        let sealedBox = try ChaChaPoly.seal(plaintext, using: sharedSecret, nonce: nonce)
        let encryptedData = sealedBox.combined
        
        return encryptedData.base64EncodedString()
    }
    
    func decrypt(_ encryptedData: String, from peerId: String? = nil) throws -> String {
        guard let encryptedBytes = Data(base64Encoded: encryptedData) else {
            throw PostError.encryptionError("Invalid base64 encrypted data")
        }
        
        var decryptionError: Error?
        
        if let peerId = peerId, let sharedSecret = sharedSecrets[peerId] {
            do {
                let sealedBox = try ChaChaPoly.SealedBox(combined: encryptedBytes)
                let plaintext = try ChaChaPoly.open(sealedBox, using: sharedSecret)
                return String(data: plaintext, encoding: .utf8) ?? ""
            } catch {
                decryptionError = error
            }
        } else {
            for (_, sharedSecret) in sharedSecrets {
                do {
                    let sealedBox = try ChaChaPoly.SealedBox(combined: encryptedBytes)
                    let plaintext = try ChaChaPoly.open(sealedBox, using: sharedSecret)
                    return String(data: plaintext, encoding: .utf8) ?? ""
                } catch {
                    decryptionError = error
                    continue
                }
            }
        }
        
        throw PostError.encryptionError("Failed to decrypt data: \(decryptionError?.localizedDescription ?? "Unknown error")")
    }
    
    func sign(_ data: Data) throws -> Data {
        guard let signingKey = signingKey else {
            throw PostError.encryptionError("Signing key not initialized")
        }
        
        return try signingKey.signature(for: data)
    }
    
    func verify(signature: Data, for data: Data, from peerPublicKey: Data) throws -> Bool {
        let peerSigningKey = try Curve25519.Signing.PublicKey(rawRepresentation: peerPublicKey)
        return peerSigningKey.isValidSignature(signature, for: data)
    }
    
    func resetKeys() async {
        do {
            try keychain.deleteKey(KeychainKeys.privateKey)
            try keychain.deleteKey(KeychainKeys.signingKey)
            try keychain.deleteKey(KeychainKeys.lastKeyRotation)
            
            privateKey = nil
            signingKey = nil
            sharedSecrets.removeAll()
            
            await generateNewKeys()
        } catch {
            print("Failed to reset keys: \(error)")
        }
    }
    
    func deriveKeyFromPassword(_ password: String, salt: Data) throws -> SymmetricKey {
        guard let passwordData = password.data(using: .utf8) else {
            throw PostError.encryptionError("Invalid password encoding")
        }
        
        let derivedKey = try PBKDF2.deriveKey(
            from: passwordData,
            salt: salt,
            using: .sha256,
            rounds: pbkdf2Rounds,
            outputByteCount: 32
        )
        
        return SymmetricKey(data: derivedKey)
    }
}

extension PBKDF2 {
    static func deriveKey(from password: Data, salt: Data, using hashFunction: HashFunction.Type, rounds: Int, outputByteCount: Int) throws -> Data {
        var derivedKeyData = Data(repeating: 0, count: outputByteCount)
        let result = derivedKeyData.withUnsafeMutableBytes { derivedKeyBytes in
            salt.withUnsafeBytes { saltBytes in
                password.withUnsafeBytes { passwordBytes in
                    CCKeyDerivationPBKDF(
                        CCPBKDFAlgorithm(kCCPBKDF2),
                        passwordBytes.bindMemory(to: Int8.self).baseAddress!,
                        password.count,
                        saltBytes.bindMemory(to: UInt8.self).baseAddress!,
                        salt.count,
                        CCPseudoRandomAlgorithm(kCCPRFHmacAlgSHA256),
                        UInt32(rounds),
                        derivedKeyBytes.bindMemory(to: UInt8.self).baseAddress!,
                        outputByteCount
                    )
                }
            }
        }
        
        guard result == kCCSuccess else {
            throw PostError.encryptionError("Key derivation failed")
        }
        
        return derivedKeyData
    }
}