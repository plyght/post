use crate::{
    derive_shared_secret, generate_keypair, generate_signing_keypair, sign_message, ClipboardData, ClipboardManager,
    CryptoSession, KeyPair, MessageData, MessageType, NodeDiscoveryData, NodeInfo, NodeMap, PostMessage, Result, SigningKeyPair,
    SystemClipboard,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info};

pub struct SyncManager {
    clipboard: Arc<SystemClipboard>,
    nodes: Arc<RwLock<NodeMap>>,
    sequence_counter: Arc<Mutex<u64>>,
    node_id: String,
    last_clipboard_hash: Arc<Mutex<u64>>,
    crypto_sessions: Arc<Mutex<HashMap<String, CryptoSession>>>,
    signing_keypair: SigningKeyPair,
    exchange_keypair: KeyPair,
}

impl SyncManager {
    pub fn new(clipboard: Arc<SystemClipboard>, node_id: String) -> Result<Self> {
        let signing_keypair = generate_signing_keypair()?;
        let exchange_keypair = generate_keypair()?;

        Ok(Self {
            clipboard,
            nodes: Arc::new(RwLock::new(HashMap::new())),
            sequence_counter: Arc::new(Mutex::new(0)),
            node_id,
            last_clipboard_hash: Arc::new(Mutex::new(0)),
            crypto_sessions: Arc::new(Mutex::new(HashMap::new())),
            signing_keypair,
            exchange_keypair,
        })
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

                    let clipboard_data = ClipboardData {
                        content,
                        timestamp,
                        source_node: node_id,
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

    fn sign_post_message(message: &mut PostMessage, signing_keypair: &SigningKeyPair) -> Result<()> {
        let message_bytes = serde_json::to_vec(&message)
            .map_err(|e| crate::PostError::Serialization(format!("Failed to serialize message: {}", e)))?;

        let signature = Self::sign_message_with_keypair(signing_keypair, &message_bytes)?;
        message.signature = signature;

        Ok(())
    }

    fn sign_message_with_keypair(signing_keypair: &SigningKeyPair, message: &[u8]) -> Result<Vec<u8>> {
        use secrecy::ExposeSecret;
        let signing_key_bytes = signing_keypair.signing_key.expose_secret();
        sign_message(signing_key_bytes, message)
    }

    pub async fn handle_message(&self, message: PostMessage) -> Result<()> {
        match message.data {
            MessageData::ClipboardUpdate(data) => {
                self.handle_clipboard_update(data).await?;
            }
            MessageData::Heartbeat(data) => {
                self.handle_heartbeat(&data.source_node).await?;
            }
            MessageData::NodeDiscovery(data) => {
                // Validate the public key
                if data.public_key.len() != 32 {
                    return Err(crate::PostError::Crypto(
                        "Invalid X25519 public key length, expected 32 bytes".to_string()
                    ));
                }
                
                // Validate that the key is not all zeros (common security mistake)
                if data.public_key.iter().all(|&b| b == 0) {
                    return Err(crate::PostError::Crypto(
                        "Invalid X25519 public key: all zeros".to_string()
                    ));
                }
                
                self.handle_node_discovery(&data.source_node, &data.public_key)
                    .await?;
            }
        }
        Ok(())
    }

    async fn handle_clipboard_update(&self, data: ClipboardData) -> Result<()> {
        if data.source_node == self.node_id {
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

        self.clipboard.set_contents(&data.content).await?;
        *last_hash = content_hash;

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

    async fn handle_node_discovery(&self, node_id: &str, remote_public_key: &[u8]) -> Result<()> {
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
        let shared_secret = derive_shared_secret(&self.exchange_keypair.private_key, public_key)?;
        let crypto_session = CryptoSession::new(&shared_secret)?;

        let mut sessions = self.crypto_sessions.lock().await;
        sessions.insert(node_id.to_string(), crypto_session);

        debug!("Created crypto session for node: {}", node_id);
        Ok(())
    }

    pub async fn get_crypto_session(&self, node_id: &str) -> Option<CryptoSession> {
        let sessions = self.crypto_sessions.lock().await;
        sessions.get(node_id).cloned()
    }

    pub fn create_node_discovery_message(&self) -> Result<PostMessage> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let discovery_data = NodeDiscoveryData {
            source_node: self.node_id.clone(),
            timestamp,
            public_key: self.exchange_keypair.public_key.clone(),
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
