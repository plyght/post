use crate::{
    ClipboardData, ClipboardManager, CryptoSession, MessageType, NodeInfo, NodeMap, PostMessage,
    Result, SystemClipboard,
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
    #[allow(dead_code)]
    crypto_sessions: Arc<Mutex<HashMap<String, CryptoSession>>>,
}

impl SyncManager {
    pub fn new(clipboard: Arc<SystemClipboard>, node_id: String) -> Self {
        Self {
            clipboard,
            nodes: Arc::new(RwLock::new(HashMap::new())),
            sequence_counter: Arc::new(Mutex::new(0)),
            node_id,
            last_clipboard_hash: Arc::new(Mutex::new(0)),
            crypto_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
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

        clipboard
            .watch_changes_generic(move |content| {
                let send_fn = send_fn.clone();
                let sequence_counter = Arc::clone(&sequence_counter);
                let node_id = node_id.clone();
                let last_hash = Arc::clone(&last_hash);

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

                    let message = PostMessage {
                        version: 1,
                        message_type: MessageType::ClipboardUpdate,
                        data: clipboard_data,
                        signature: vec![], // TODO: Add signing
                    };

                    debug!("Broadcasting clipboard update (seq: {})", sequence);
                    send_fn(message);
                });
            })
            .await?;

        Ok(())
    }

    pub async fn handle_message(&self, message: PostMessage) -> Result<()> {
        match message.message_type {
            MessageType::ClipboardUpdate => {
                self.handle_clipboard_update(message.data).await?;
            }
            MessageType::Heartbeat => {
                self.handle_heartbeat(&message.data.source_node).await?;
            }
            MessageType::NodeDiscovery => {
                self.handle_node_discovery(&message.data.source_node)
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

    async fn handle_node_discovery(&self, node_id: &str) -> Result<()> {
        let mut nodes = self.nodes.write().await;
        if !nodes.contains_key(node_id) {
            let node_info = NodeInfo {
                id: node_id.to_string(),
                name: node_id.to_string(), // TODO: Get actual node name
                last_seen: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                public_key: vec![], // TODO: Exchange public keys
            };
            nodes.insert(node_id.to_string(), node_info);
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
}

fn calculate_hash(content: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}
