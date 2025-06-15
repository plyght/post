pub mod clipboard;
pub mod config;
pub mod crypto;
pub mod error;
pub mod sync;
pub mod transport;

pub use clipboard::*;
pub use config::*;
pub use crypto::*;
pub use error::*;
pub use sync::*;
pub use transport::*;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardData {
    pub content: String,
    pub timestamp: u64,
    pub source_node: String,
    pub sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDiscoveryData {
    pub source_node: String,
    pub timestamp: u64,
    pub public_key: [u8; 32],
    pub signing_public_key: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatData {
    pub source_node: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageData {
    ClipboardUpdate(ClipboardData),
    NodeDiscovery(NodeDiscoveryData),
    Heartbeat(HeartbeatData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostMessage {
    pub version: u8,
    pub message_type: MessageType,
    pub data: MessageData,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    ClipboardUpdate,
    Heartbeat,
    NodeDiscovery,
}

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub id: String,
    pub name: String,
    pub last_seen: u64,
    pub public_key: Vec<u8>,
}

pub type NodeMap = HashMap<String, NodeInfo>;
