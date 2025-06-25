use crate::{
    derive_shared_secret, generate_keypair, generate_signing_keypair,
    sign_message_with_signing_key, verify_signature, ClipboardData, ClipboardManager,
    CryptoSession, KeyPair, MessageData, MessageType, NodeDiscoveryData, NodeInfo, NodeMap,
    PostMessage, Result, SigningKeyPair, SystemClipboard,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info};
use x25519_dalek;

pub struct SyncManager {
    clipboard: Arc<SystemClipboard>,
    nodes: Arc<RwLock<NodeMap>>,
    sequence_counter: Arc<Mutex<u64>>,
    node_id: Arc<Mutex<String>>,
    last_clipboard_hash: Arc<Mutex<u64>>,
    crypto_sessions: Arc<Mutex<HashMap<String, CryptoSession>>>,
    signing_keypair: SigningKeyPair,
    exchange_keypair: KeyPair,
    node_verifying_keys: Arc<Mutex<HashMap<String, [u8; 32]>>>,
}

impl SyncManager {
    pub fn new(clipboard: Arc<SystemClipboard>, node_id: String) -> Result<Self> {
        let signing_keypair = generate_signing_keypair()?;
        let exchange_keypair = generate_keypair()?;

        Ok(Self {
            clipboard,
            nodes: Arc::new(RwLock::new(HashMap::new())),
            sequence_counter: Arc::new(Mutex::new(0)),
            node_id: Arc::new(Mutex::new(node_id)),
            last_clipboard_hash: Arc::new(Mutex::new(0)),
            crypto_sessions: Arc::new(Mutex::new(HashMap::new())),
            signing_keypair,
            exchange_keypair,
            node_verifying_keys: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Update the node ID - useful when Tailscale becomes available after startup
    pub async fn update_node_id(&self, new_node_id: String) -> Result<()> {
        let mut node_id = self.node_id.lock().await;
        if *node_id != new_node_id {
            info!("Updating node ID from {} to {}", *node_id, new_node_id);
            *node_id = new_node_id;
        }
        Ok(())
    }

    /// Get the current node ID
    pub async fn get_node_id(&self) -> String {
        self.node_id.lock().await.clone()
    }

    pub async fn start_sync_loop<F>(&self, send_message: F) -> Result<()>
    where
        F: Fn(PostMessage) + Send + Sync + 'static + Clone,
    {
        let clipboard = Arc::clone(&self.clipboard);
        let sequence_counter = Arc::clone(&self.sequence_counter);
        let node_id = self.node_id.clone();
        let last_hash = Arc::clone(&self.last_clipboard_hash);
        let send_fn = send_message.clone();
        let signing_keypair = self.signing_keypair.clone();

        clipboard
            .watch_changes_generic(move |content| {
                let send_fn = send_fn.clone();
                let sequence_counter = Arc::clone(&sequence_counter);
                let node_id = node_id.clone();
                let last_hash = Arc::clone(&last_hash);
                let signing_keypair = signing_keypair.clone();

                tokio::spawn(async move {
                    let content_hash = calculate_hash(&content);
                    let mut last = last_hash.lock().await;

                    if content_hash == *last {
                        return;
                    }
                    *last = content_hash;
                    drop(last);

                    let mut seq = sequence_counter.lock().await;
                    *seq += 1;
                    let sequence = *seq;
                    drop(seq);

                    let timestamp = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();

                    let source_node = node_id.lock().await.clone();
                    let clipboard_data = ClipboardData {
                        content,
                        timestamp,
                        source_node,
                        sequence,
                    };

                    let mut message = PostMessage {
                        version: 1,
                        message_type: MessageType::ClipboardUpdate,
                        data: MessageData::ClipboardUpdate(clipboard_data),
                        signature: vec![],
                    };

                    // Sign the message
                    match Self::sign_post_message(&mut message, &signing_keypair) {
                        Ok(()) => {
                            debug!("Broadcasting clipboard update (seq: {})", sequence);
                            send_fn(message);
                        }
                        Err(e) => {
                            tracing::error!("Failed to sign clipboard update message: {}", e);
                        }
                    }
                });
            })
            .await?;

        Ok(())
    }

    fn sign_post_message(
        message: &mut PostMessage,
        signing_keypair: &SigningKeyPair,
    ) -> Result<()> {
        let mut message_for_signing = message.clone();
        message_for_signing.signature = Vec::new();

        let message_bytes = serde_json::to_vec(&message_for_signing).map_err(|e| {
            crate::PostError::Serialization(format!("Failed to serialize message: {}", e))
        })?;

        let signature = Self::sign_message_with_keypair(signing_keypair, &message_bytes)?;
        message.signature = signature;

        Ok(())
    }

    fn sign_message_with_keypair(
        signing_keypair: &SigningKeyPair,
        message: &[u8],
    ) -> Result<Vec<u8>> {
        sign_message_with_signing_key(signing_keypair, message)
    }

    async fn verify_message_signature(
        &self,
        message: &PostMessage,
        source_node: &str,
    ) -> Result<()> {
        // Create a message copy without the signature for verification
        let mut message_for_verification = message.clone();
        message_for_verification.signature = Vec::new();

        let message_bytes = serde_json::to_vec(&message_for_verification).map_err(|e| {
            crate::PostError::Serialization(format!(
                "Failed to serialize message for verification: {}",
                e
            ))
        })?;

        // Get the verifying key for this node
        let node_keys = self.node_verifying_keys.lock().await;
        let verifying_key = node_keys.get(source_node).ok_or_else(|| {
            crate::PostError::Crypto(format!("No verifying key found for node: {}", source_node))
        })?;

        // Verify the signature
        let signature_valid = verify_signature(verifying_key, &message_bytes, &message.signature)?;
        if !signature_valid {
            return Err(crate::PostError::Crypto(format!(
                "Invalid signature on message from node: {}",
                source_node
            )));
        }

        Ok(())
    }

    pub async fn handle_message(&self, message: PostMessage) -> Result<()> {
        match &message.data {
            MessageData::ClipboardUpdate(data) => {
                // Verify message signature
                self.verify_message_signature(&message, &data.source_node)
                    .await?;
                self.handle_clipboard_update(data.clone()).await?;
            }
            MessageData::Heartbeat(data) => {
                // Verify message signature
                self.verify_message_signature(&message, &data.source_node)
                    .await?;
                self.handle_heartbeat(&data.source_node).await?;
            }
            MessageData::NodeDiscovery(data) => {
                // Create a message copy without the signature for verification
                let mut message_for_verification = message.clone();
                message_for_verification.signature = Vec::new();

                let message_bytes = serde_json::to_vec(&message_for_verification).map_err(|e| {
                    crate::PostError::Serialization(format!(
                        "Failed to serialize message for verification: {}",
                        e
                    ))
                })?;

                // Verify the signature
                let signature_valid =
                    verify_signature(&data.signing_public_key, &message_bytes, &message.signature)?;
                if !signature_valid {
                    return Err(crate::PostError::Crypto(
                        "Invalid Ed25519 signature on node discovery message".to_string(),
                    ));
                }

                // Validate that the key is not all zeros (common security mistake)
                if data.public_key.iter().all(|&b| b == 0) {
                    return Err(crate::PostError::Crypto(
                        "Invalid X25519 public key: all zeros".to_string(),
                    ));
                }

                // Store the binding between source_node and verifying key
                let mut node_keys = self.node_verifying_keys.lock().await;
                if let Some(existing_key) = node_keys.get(&data.source_node) {
                    // Verify the node is still using the same verifying key
                    if existing_key != &data.signing_public_key {
                        return Err(crate::PostError::Crypto(format!(
                            "Node {} attempted to change verifying key",
                            data.source_node
                        )));
                    }
                } else {
                    // Store the new binding
                    node_keys.insert(data.source_node.clone(), data.signing_public_key);
                }
                drop(node_keys);

                // Only now proceed with session derivation after successful verification
                self.handle_node_discovery(&data.source_node, &data.public_key)
                    .await?;
            }
        }
        Ok(())
    }

    async fn handle_clipboard_update(&self, data: ClipboardData) -> Result<()> {
        let current_node_id = self.node_id.lock().await.clone();
        if data.source_node == current_node_id {
            debug!("Ignoring own clipboard update");
            return Ok(());
        }

        let content_hash = calculate_hash(&data.content);
        let mut last_hash = self.last_clipboard_hash.lock().await;

        if content_hash == *last_hash {
            debug!("Duplicate clipboard content, ignoring");
            return Ok(());
        }

        info!(
            "Received clipboard update from {}: {} chars",
            data.source_node,
            data.content.len()
        );

        match self.clipboard.set_contents(&data.content).await {
            Ok(()) => {
                info!("Successfully set clipboard contents on Linux");
                *last_hash = content_hash;
            }
            Err(e) => {
                error!("Failed to set clipboard contents on Linux: {}", e);
                return Err(e);
            }
        }

        Ok(())
    }

    async fn handle_heartbeat(&self, node_id: &str) -> Result<()> {
        let mut nodes = self.nodes.write().await;
        if let Some(node) = nodes.get_mut(node_id) {
            node.last_seen = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            debug!("Updated heartbeat for node: {}", node_id);
        }
        Ok(())
    }

    async fn handle_node_discovery(
        &self,
        node_id: &str,
        remote_public_key: &[u8; 32],
    ) -> Result<()> {
        let mut nodes = self.nodes.write().await;
        if !nodes.contains_key(node_id) {
            let node_info = NodeInfo {
                id: node_id.to_string(),
                name: node_id.to_string(), // TODO: Get actual node name
                last_seen: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                public_key: remote_public_key.to_vec(),
            };
            nodes.insert(node_id.to_string(), node_info.clone());
            drop(nodes);

            // Create crypto session for the new node
            self.create_crypto_session_for_node(node_id, &node_info.public_key)
                .await?;

            info!("Discovered new node: {}", node_id);
        }
        Ok(())
    }

    pub async fn get_nodes(&self) -> NodeMap {
        self.nodes.read().await.clone()
    }

    pub async fn cleanup_stale_nodes(&self, max_age_seconds: u64) -> Result<()> {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut nodes = self.nodes.write().await;
        let initial_count = nodes.len();

        nodes.retain(|id, node| {
            let is_stale = current_time.saturating_sub(node.last_seen) > max_age_seconds;
            if is_stale {
                debug!("Removing stale node: {}", id);
            }
            !is_stale
        });

        if nodes.len() != initial_count {
            info!("Cleaned up {} stale nodes", initial_count - nodes.len());
        }

        Ok(())
    }

    async fn create_crypto_session_for_node(&self, node_id: &str, public_key: &[u8]) -> Result<()> {
        // Validate public key by parsing into x25519_dalek::PublicKey
        let public_key_array: [u8; 32] = public_key
            .try_into()
            .map_err(|_| crate::PostError::Crypto("Invalid public key length".to_string()))?;

        // Check if public key is all zeros (invalid/weak key)
        if public_key_array.iter().all(|&b| b == 0) {
            return Err(crate::PostError::Crypto(
                "Invalid public key: all zeros".to_string(),
            ));
        }

        // Parse into PublicKey to validate it's a valid point
        let _parsed_public_key = x25519_dalek::PublicKey::from(public_key_array);

        let shared_secret =
            derive_shared_secret(&self.exchange_keypair.private_key, &public_key_array)?;
        let crypto_session = CryptoSession::new(&shared_secret)?;

        let mut sessions = self.crypto_sessions.lock().await;
        sessions.insert(node_id.to_string(), crypto_session);

        info!("Created crypto session for node: {}", node_id);
        Ok(())
    }

    pub async fn get_crypto_session(&self, node_id: &str) -> Option<CryptoSession> {
        let sessions = self.crypto_sessions.lock().await;
        sessions.get(node_id).cloned()
    }

    pub async fn create_node_discovery_message(&self) -> Result<PostMessage> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let discovery_data = NodeDiscoveryData {
            source_node: self.node_id.lock().await.clone(),
            timestamp,
            public_key: *<&[u8; 32]>::try_from(self.exchange_keypair.public_key.as_slice())
                .map_err(|_| {
                    crate::PostError::Crypto("Exchange public key must be 32 bytes".to_string())
                })?,
            signing_public_key: *<&[u8; 32]>::try_from(
                self.signing_keypair.verifying_key.as_slice(),
            )
            .map_err(|_| {
                crate::PostError::Crypto("Signing public key must be 32 bytes".to_string())
            })?,
        };

        let mut message = PostMessage {
            version: 1,
            message_type: MessageType::NodeDiscovery,
            data: MessageData::NodeDiscovery(discovery_data),
            signature: vec![],
        };

        // Sign the message
        Self::sign_post_message(&mut message, &self.signing_keypair)?;

        Ok(message)
    }
}

fn calculate_hash(content: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}
