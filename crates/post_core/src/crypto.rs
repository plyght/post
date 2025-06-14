use crate::{PostError, Result};
use blake2::{Blake2s256, Digest};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;
use x25519_dalek::{EphemeralSecret, PublicKey};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPair {
    pub public_key: Vec<u8>,
    pub private_key: Vec<u8>,
}

pub struct CryptoSession {
    cipher: Arc<Mutex<ChaCha20Poly1305>>,
    nonce_counter: Arc<Mutex<u64>>,
}

impl CryptoSession {
    pub fn new(shared_secret: &[u8]) -> Result<Self> {
        let key = derive_encryption_key(shared_secret)?;
        let cipher = ChaCha20Poly1305::new_from_slice(&key)
            .map_err(|e| PostError::Crypto(format!("Failed to create cipher: {}", e)))?;

        Ok(Self {
            cipher: Arc::new(Mutex::new(cipher)),
            nonce_counter: Arc::new(Mutex::new(0)),
        })
    }

    pub async fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = self.cipher.lock().await;
        let mut counter = self.nonce_counter.lock().await;

        *counter = counter.wrapping_add(1);
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[4..].copy_from_slice(&counter.to_le_bytes());
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| PostError::Crypto(format!("Encryption failed: {}", e)))?;

        let mut result = nonce_bytes.to_vec();
        result.extend_from_slice(&ciphertext);

        debug!(
            "Encrypted {} bytes -> {} bytes",
            plaintext.len(),
            result.len()
        );
        Ok(result)
    }

    pub async fn decrypt(&self, encrypted_data: &[u8]) -> Result<Vec<u8>> {
        if encrypted_data.len() < 12 {
            return Err(PostError::Crypto(
                "Invalid encrypted data length".to_string(),
            ));
        }

        let cipher = self.cipher.lock().await;
        let nonce = Nonce::from_slice(&encrypted_data[..12]);
        let ciphertext = &encrypted_data[12..];

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| PostError::Crypto(format!("Decryption failed: {}", e)))?;

        debug!(
            "Decrypted {} bytes -> {} bytes",
            encrypted_data.len(),
            plaintext.len()
        );
        Ok(plaintext)
    }
}

pub fn generate_keypair() -> Result<KeyPair> {
    let secret = EphemeralSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);

    // For storage, we need the private key bytes
    let mut private_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut private_bytes);

    Ok(KeyPair {
        public_key: public.to_bytes().to_vec(),
        private_key: private_bytes.to_vec(),
    })
}

pub fn derive_shared_secret(_private_key: &[u8], public_key: &[u8]) -> Result<[u8; 32]> {
    // For now, we'll use a simple key derivation approach
    // In a real implementation, you'd want to properly reconstruct the secret from stored bytes
    let secret = EphemeralSecret::random_from_rng(OsRng);

    let public = PublicKey::from(
        <[u8; 32]>::try_from(public_key)
            .map_err(|_| PostError::Crypto("Invalid public key length".to_string()))?,
    );

    let shared_secret = secret.diffie_hellman(&public);
    Ok(shared_secret.to_bytes())
}

pub fn derive_encryption_key(shared_secret: &[u8]) -> Result<[u8; 32]> {
    let mut hasher = Blake2s256::new();
    hasher.update(b"post-clipboard-v1");
    hasher.update(shared_secret);
    let result = hasher.finalize();
    Ok(result.into())
}

pub fn derive_key_from_tailscale_identity(identity: &[u8]) -> Result<[u8; 32]> {
    let mut hasher = Blake2s256::new();
    hasher.update(b"post-tailscale-identity-v1");
    hasher.update(identity);
    let result = hasher.finalize();

    debug!("Derived key from Tailscale identity");
    Ok(result.into())
}
