use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use post_core::{
    ClipboardData, ClipboardManager, MessageData, MessageType, PostConfig, PostMessage, 
    SyncManager, SystemClipboard, Transport,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct ApiState {
    pub sync_manager: Arc<Mutex<Option<Arc<SyncManager>>>>,
    pub clipboard: Arc<SystemClipboard>,
    pub transport: Arc<dyn Transport>,
    pub config: Arc<PostConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    pub timestamp: String,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn error(message: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct StatusResponse {
    pub status: String,
    pub node_id: String,
    pub connected_peers: Vec<PeerInfo>,
    pub tailscale_connected: bool,
    pub clipboard_content_size: usize,
    pub uptime_seconds: u64,
}

#[derive(Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: String,
    pub name: String,
    pub ip_address: String,
    pub online: bool,
    pub last_seen: String,
}

#[derive(Serialize, Deserialize)]
pub struct ClipboardSyncRequest {
    pub content: String,
    pub content_type: String,
    pub node_id: String,
    pub timestamp: String,
}

#[derive(Serialize, Deserialize)]
pub struct ClipboardResponse {
    pub content: String,
    pub content_type: String,
    pub timestamp: String,
    pub source_node: String,
}

#[derive(Serialize, Deserialize)]
pub struct HandshakeRequest {
    pub public_key: String,
    pub signing_key: String,
    pub node_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct HandshakeResponse {
    pub public_key: String,
    pub signing_key: String,
    pub node_id: String,
    pub session_id: String,
}

pub fn create_router(state: ApiState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/status", get(get_status))
        .route("/api/v1/clipboard/sync", post(sync_clipboard))
        .route("/api/v1/clipboard/pull", get(pull_clipboard))
        .route("/api/v1/auth/handshake", post(auth_handshake))
        .route("/api/v1/peers", get(get_peers))
        .route("/api/v1/peers/:peer_id/sync", post(sync_to_peer))
        .with_state(state)
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(CorsLayer::permissive()),
        )
}

// Health check endpoint
async fn health_check() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "service": "post-daemon",
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

// Get daemon status
async fn get_status(State(state): State<ApiState>) -> Result<Json<ApiResponse<StatusResponse>>, StatusCode> {
    info!("API: Status request received");

    // Get node ID from transport
    let node_id = match state.transport.get_node_id().await {
        Ok(id) => id,
        Err(e) => {
            error!("Failed to get node ID: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Check Tailscale connectivity
    let tailscale_connected = state.transport.is_connected().await.unwrap_or(false);

    // Get clipboard content size
    let clipboard_content = state.clipboard.get_contents().await.unwrap_or_default();
    let clipboard_content_size = clipboard_content.len();

    // Get peer information
    let tailnet_nodes = state.transport.get_tailnet_nodes().await.unwrap_or_default();
    let connected_peers = tailnet_nodes
        .into_iter()
        .enumerate()
        .map(|(i, node)| PeerInfo {
            id: format!("peer-{}", i),
            name: node,
            ip_address: "100.64.0.1".to_string(), // Placeholder - would get from Tailscale API
            online: true,
            last_seen: chrono::Utc::now().to_rfc3339(),
        })
        .collect();

    let response = StatusResponse {
        status: if tailscale_connected { "connected".to_string() } else { "disconnected".to_string() },
        node_id,
        connected_peers,
        tailscale_connected,
        clipboard_content_size,
        uptime_seconds: 0, // Would track actual uptime
    };

    Ok(Json(ApiResponse::success(response)))
}

// Sync clipboard content
async fn sync_clipboard(
    State(state): State<ApiState>,
    Json(request): Json<ClipboardSyncRequest>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    info!("API: Clipboard sync request from node {}", request.node_id);

    // Validate request
    if request.content.is_empty() {
        warn!("Empty clipboard content in sync request");
        return Ok(Json(ApiResponse::error("Empty content".to_string())));
    }

    // Update local clipboard
    if let Err(e) = state.clipboard.set_contents(&request.content).await {
        error!("Failed to set clipboard contents: {}", e);
        return Ok(Json(ApiResponse::error(format!("Failed to set clipboard: {}", e))));
    }

    // Create PostMessage for peer sync
    let clipboard_data = ClipboardData {
        content: request.content.clone(),
        timestamp: chrono::Utc::now().timestamp() as u64,
        source_node: request.node_id.clone(),
        sequence: 1, // Would be actual sequence number in production
    };

    let message = PostMessage {
        version: 1,
        message_type: MessageType::ClipboardUpdate,
        data: MessageData::ClipboardUpdate(clipboard_data),
        signature: vec![], // Would be actual signature in production
    };

    // Send to other peers via existing transport
    if let Err(e) = state.transport.send_message(message).await {
        error!("Failed to send message to peers: {}", e);
        return Ok(Json(ApiResponse::error(format!("Failed to sync to peers: {}", e))));
    }

    info!("Clipboard sync completed successfully");
    Ok(Json(ApiResponse::success("Clipboard synced successfully".to_string())))
}

// Pull latest clipboard content
async fn pull_clipboard(State(state): State<ApiState>) -> Result<Json<ApiResponse<ClipboardResponse>>, StatusCode> {
    info!("API: Clipboard pull request received");

    let content = match state.clipboard.get_contents().await {
        Ok(content) => content,
        Err(e) => {
            error!("Failed to get clipboard contents: {}", e);
            return Ok(Json(ApiResponse::error(format!("Failed to get clipboard: {}", e))));
        }
    };

    let node_id = state.transport.get_node_id().await.unwrap_or_else(|_| "unknown".to_string());

    let response = ClipboardResponse {
        content,
        content_type: "text/plain".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        source_node: node_id,
    };

    Ok(Json(ApiResponse::success(response)))
}

// Handle authentication handshake
async fn auth_handshake(
    State(state): State<ApiState>,
    Json(request): Json<HandshakeRequest>,
) -> Result<Json<ApiResponse<HandshakeResponse>>, StatusCode> {
    info!("API: Authentication handshake from node {}", request.node_id);

    // In a production system, you'd validate the keys and establish secure session
    // For now, we'll return a basic response

    let node_id = state.transport.get_node_id().await.unwrap_or_else(|_| "daemon".to_string());
    let session_id = Uuid::new_v4().to_string();

    let response = HandshakeResponse {
        public_key: "daemon-public-key".to_string(), // Would be actual public key
        signing_key: "daemon-signing-key".to_string(), // Would be actual signing key
        node_id,
        session_id,
    };

    info!("Handshake completed with session ID: {}", response.session_id);
    Ok(Json(ApiResponse::success(response)))
}

// Get connected peers
async fn get_peers(State(state): State<ApiState>) -> Result<Json<ApiResponse<Vec<PeerInfo>>>, StatusCode> {
    info!("API: Peers list request received");

    let tailnet_nodes = state.transport.get_tailnet_nodes().await.unwrap_or_default();
    let peers = tailnet_nodes
        .into_iter()
        .enumerate()
        .map(|(i, node)| PeerInfo {
            id: format!("peer-{}", i),
            name: node,
            ip_address: "100.64.0.1".to_string(), // Placeholder
            online: true,
            last_seen: chrono::Utc::now().to_rfc3339(),
        })
        .collect();

    Ok(Json(ApiResponse::success(peers)))
}

// Sync clipboard to specific peer
async fn sync_to_peer(
    State(state): State<ApiState>,
    Path(peer_id): Path<String>,
    Json(request): Json<ClipboardSyncRequest>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    info!("API: Sync to peer {} request received", peer_id);

    // Create PostMessage
    let clipboard_data = ClipboardData {
        content: request.content,
        timestamp: chrono::Utc::now().timestamp() as u64,
        source_node: request.node_id,
        sequence: 1, // Would be actual sequence number in production
    };

    let message = PostMessage {
        version: 1,
        message_type: MessageType::ClipboardUpdate,
        data: MessageData::ClipboardUpdate(clipboard_data),
        signature: vec![], // Would be actual signature in production
    };

    // Send to specific peer (in production, you'd target the specific peer)
    if let Err(e) = state.transport.send_message(message).await {
        error!("Failed to send message to peer {}: {}", peer_id, e);
        return Ok(Json(ApiResponse::error(format!("Failed to sync to peer: {}", e))));
    }

    Ok(Json(ApiResponse::success(format!("Synced to peer {}", peer_id))))
}

pub async fn start_api_server(
    config: Arc<PostConfig>,
    sync_manager: Arc<Mutex<Option<Arc<SyncManager>>>>,
    clipboard: Arc<SystemClipboard>,
    transport: Arc<dyn Transport>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state = ApiState {
        sync_manager,
        clipboard,
        transport,
        config: Arc::clone(&config),
    };

    let app = create_router(state);

    // Use a different port for HTTP API (8413) to avoid conflict with TCP P2P (8412)
    let http_port = config.network.port + 1;
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", http_port)).await?;
    
    info!("🚀 Post HTTP API server starting on port {}", http_port);
    info!("📋 API endpoints available:");
    info!("   GET  /health                    - Health check");
    info!("   GET  /api/v1/status             - Daemon status");
    info!("   POST /api/v1/clipboard/sync     - Sync clipboard");
    info!("   GET  /api/v1/clipboard/pull     - Pull clipboard");
    info!("   POST /api/v1/auth/handshake     - Authentication");
    info!("   GET  /api/v1/peers              - List peers");
    info!("   POST /api/v1/peers/:id/sync     - Sync to peer");

    axum::serve(listener, app).await?;
    
    Ok(())
}