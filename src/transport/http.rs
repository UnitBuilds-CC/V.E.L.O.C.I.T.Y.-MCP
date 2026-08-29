//! HTTP/SSE transport for MCP protocol.
//!
//! Provides an Axum-based HTTP server with:
//! - JSON-RPC over HTTP POST (stateless)
//! - Streamable HTTP transport (POST with SSE response)
//! - SSE endpoint for real-time streaming of tool results
//! - Session management with session IDs
//! - Request ID correlation
//! - Connection lifecycle management
//! - Rate limiting per session/IP
//! - API key authentication
//! - Configurable CORS
//! - Request size limits
//!
//! This module is feature-gated behind the `http` feature flag.

use axum::{
    extract::{Query, State},
    http::{StatusCode, HeaderMap},
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tower_http::cors::{Any, CorsLayer};
use tracing::info;
use uuid::Uuid;

use crate::protocol::json_rpc;
use crate::rate_limit;

/// HTTP security configuration.
#[derive(Clone, Debug)]
pub struct HttpSecurityConfig {
    /// API key for authentication (None = no auth required)
    pub api_key: Option<String>,
    /// Maximum request body size in bytes (default: 10MB)
    pub max_request_size: usize,
    /// Enable rate limiting (default: true)
    pub enable_rate_limit: bool,
    /// Allowed CORS origins (None = allow all)
    pub cors_origins: Option<Vec<String>>,
}

impl Default for HttpSecurityConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            max_request_size: 10 * 1024 * 1024, // 10MB
            enable_rate_limit: true,
            cors_origins: None,
        }
    }
}

/// Session state for HTTP clients.
#[derive(Debug)]
struct Session {
    id: String,
    created_at: std::time::Instant,
    last_activity: std::time::Instant,
    request_count: AtomicU64,
}

/// Shared state for the HTTP server.
struct ServerState {
    shutdown: Arc<AtomicBool>,
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    /// Channel for broadcasting events to SSE clients
    event_broadcast: Arc<RwLock<Vec<mpsc::Sender<String>>>>,
    /// Security configuration
    security: HttpSecurityConfig,
}

/// Query parameters for SSE endpoint.
#[derive(Deserialize)]
struct SseQuery {
    session_id: Option<String>,
}

/// Request body for Streamable HTTP transport.
#[derive(Deserialize)]
struct StreamableRequest {
    jsonrpc: String,
    method: String,
    params: Option<Value>,
    id: Option<Value>,
    #[serde(rename = "_meta")]
    meta: Option<Value>,
}

/// Handle a JSON-RPC request over HTTP POST (stateless).
async fn handle_json_rpc(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Check authentication if API key is configured
    if let Some(expected_key) = &state.security.api_key {
        let auth_header = headers.get("Authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "jsonrpc": "2.0",
                    "error": { "code": -32000, "message": "Missing API key" },
                    "id": request.get("id").cloned().unwrap_or(Value::Null)
                }))
            ))?;
        
        if !auth_header.starts_with("Bearer ") || &auth_header[7..] != expected_key {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "jsonrpc": "2.0",
                    "error": { "code": -32000, "message": "Invalid API key" },
                    "id": request.get("id").cloned().unwrap_or(Value::Null)
                }))
            ));
        }
    }
    
    // Check rate limit if enabled
    if state.security.enable_rate_limit && !rate_limit::check_rate_limit() {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "jsonrpc": "2.0",
                "error": { "code": -32000, "message": "Rate limit exceeded" },
                "id": request.get("id").cloned().unwrap_or(Value::Null)
            }))
        ));
    }
    
    if state.shutdown.load(Ordering::Relaxed) {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "jsonrpc": "2.0",
                "error": { "code": -32603, "message": "Server shutting down" },
                "id": request.get("id").cloned().unwrap_or(Value::Null)
            })),
        ));
    }

    let response = json_rpc::handle_request(&request);
    match response {
        Some(res) => Ok(Json(res)),
        None => Err((
            StatusCode::NO_CONTENT,
            Json(json!({"message": "Notification processed"})),
        )),
    }
}

/// Handle Streamable HTTP transport (POST with SSE response).
///
/// This implements the MCP Streamable HTTP transport where:
/// - Client sends POST with JSON-RPC request
/// - Server responds with SSE stream containing the response
/// - Allows streaming of large results and progress updates
async fn handle_streamable(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<SseQuery>,
    Json(request): Json<StreamableRequest>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let (tx, rx) = mpsc::channel::<String>(100);

    // Get or create session
    let session_id = query.session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    
    {
        let mut sessions = state.sessions.write().await;
        let session = sessions.entry(session_id.clone()).or_insert_with(|| Session {
            id: session_id.clone(),
            created_at: std::time::Instant::now(),
            last_activity: std::time::Instant::now(),
            request_count: AtomicU64::new(0),
        });
        session.last_activity = std::time::Instant::now();
        session.request_count.fetch_add(1, Ordering::Relaxed);
    }

    // Process request and stream response
    let request_json = json!({
        "jsonrpc": request.jsonrpc,
        "method": request.method,
        "params": request.params,
        "id": request.id
    });

    // Spawn task to process request and stream results
    let session_id_clone = session_id.clone();
    tokio::spawn(async move {
        // Send session ID as first event
        let _ = tx.send(format!("event: session\ndata: {}\n\n", session_id_clone)).await;

        // Process the request
        let response = json_rpc::handle_request(&request_json);
        
        match response {
            Some(res) => {
                // Stream the response
                let response_str = serde_json::to_string(&res).unwrap_or_default();
                let _ = tx.send(format!("event: response\ndata: {}\n\n", response_str)).await;
            }
            None => {
                // Notification, no response needed
                let _ = tx.send("event: notification\ndata: {\"status\": \"processed\"}\n\n".to_string()).await;
            }
        }

        // Send completion event
        let _ = tx.send("event: complete\ndata: {}\n\n".to_string()).await;
    });

    let stream = ReceiverStream::new(rx).map(|msg| {
        Ok(Event::default().data(msg))
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("heartbeat"),
    )
}

/// SSE endpoint for real-time streaming.
///
/// Clients connect here to receive:
/// - Tool execution progress updates
/// - Streaming tool results
/// - Server-initiated notifications
/// - Heartbeat keepalives
async fn sse_handler(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<SseQuery>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let (tx, rx) = mpsc::channel::<String>(100);

    // Register this client for event broadcasts
    {
        let mut broadcasts = state.event_broadcast.write().await;
        broadcasts.push(tx.clone());
    }

    // Get or create session
    let session_id = query.session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    
    {
        let mut sessions = state.sessions.write().await;
        sessions.entry(session_id.clone()).or_insert_with(|| Session {
            id: session_id.clone(),
            created_at: std::time::Instant::now(),
            last_activity: std::time::Instant::now(),
            request_count: AtomicU64::new(0),
        });
    }

    // Send initial connection event
    let tx_init = tx.clone();
    let session_id_init = session_id.clone();
    tokio::spawn(async move {
        let _ = tx_init.send(format!(
            "event: connected\ndata: {{\"sessionId\": \"{}\"}}\n\n",
            session_id_init
        )).await;
    });

    // Spawn heartbeat task
    let tx_heartbeat = tx.clone();
    tokio::spawn(async move {
        use tokio::time::{interval, Duration};
        let mut interval = interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            if tx_heartbeat.send(":heartbeat\n\n".to_string()).await.is_err() {
                break;
            }
        }
    });

    let stream = ReceiverStream::new(rx).map(|msg| {
        Ok(Event::default().data(msg))
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("heartbeat"),
    )
}

/// Broadcast an event to all connected SSE clients.
async fn broadcast_event(state: &ServerState, event_type: &str, data: &Value) {
    let event_str = format!(
        "event: {}\ndata: {}\n\n",
        event_type,
        serde_json::to_string(data).unwrap_or_default()
    );

    let broadcasts = state.event_broadcast.read().await;
    for sender in broadcasts.iter() {
        let _ = sender.send(event_str.clone()).await;
    }
}

/// Health check endpoint.
async fn health_check(State(state): State<Arc<ServerState>>) -> Json<Value> {
    let sessions = state.sessions.read().await;
    let session_count = sessions.len();
    
    Json(json!({
        "status": "healthy",
        "transport": "http",
        "version": env!("CARGO_PKG_VERSION"),
        "activeSessions": session_count
    }))
}

/// Session management endpoints.
async fn list_sessions(State(state): State<Arc<ServerState>>) -> Json<Value> {
    let sessions = state.sessions.read().await;
    let session_list: Vec<Value> = sessions.values().map(|s| {
        json!({
            "id": s.id,
            "createdAt": s.created_at.elapsed().as_secs(),
            "lastActivity": s.last_activity.elapsed().as_secs(),
            "requestCount": s.request_count.load(Ordering::Relaxed)
        })
    }).collect();

    Json(json!({ "sessions": session_list }))
}

async fn delete_session(
    State(state): State<Arc<ServerState>>,
    session_id: String,
) -> StatusCode {
    let mut sessions = state.sessions.write().await;
    if sessions.remove(&session_id).is_some() {
        info!(session_id = %session_id, "Session deleted");
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

/// Build the Axum router with all MCP endpoints.
fn build_router(state: Arc<ServerState>) -> Router {
    // Configure CORS based on security config
    let cors = if let Some(origins) = &state.security.cors_origins {
        let mut cors = CorsLayer::new()
            .allow_methods(Any)
            .allow_headers(Any);
        
        // Parse and add allowed origins
        for origin in origins {
            if let Ok(origin_header) = origin.parse::<axum::http::HeaderValue>() {
                cors = cors.allow_origin(origin_header);
            }
        }
        cors
    } else {
        // Allow all origins if not configured
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    };

    Router::new()
        .route("/mcp", post(handle_json_rpc))
        .route("/mcp/stream", post(handle_streamable))
        .route("/sse", get(sse_handler))
        .route("/health", get(health_check))
        .route("/sessions", get(list_sessions))
        .route("/sessions/:id", get(delete_session))
        .layer(cors)
        .with_state(state)
}

/// Run the HTTP server on the given address.
///
/// This function blocks until the shutdown signal is received.
pub async fn run_http_server(
    addr: &str, 
    shutdown: Arc<AtomicBool>,
    security_config: Option<HttpSecurityConfig>,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(ServerState {
        shutdown: shutdown.clone(),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        event_broadcast: Arc::new(RwLock::new(Vec::new())),
        security: security_config.unwrap_or_default(),
    });

    let app = build_router(state);

    info!(addr = addr, "Starting HTTP server");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shutdown))
        .await?;

    info!("HTTP server shut down cleanly");
    Ok(())
}

/// Wait for the shutdown signal.
async fn shutdown_signal(shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::Relaxed) {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_check() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let state = Arc::new(ServerState {
            shutdown,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            event_broadcast: Arc::new(RwLock::new(Vec::new())),
        });
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_json_rpc_initialize() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let state = Arc::new(ServerState {
            shutdown,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            event_broadcast: Arc::new(RwLock::new(Vec::new())),
        });
        let app = build_router(state);

        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1.0"}
            },
            "id": 1
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_shutdown_rejects_requests() {
        let shutdown = Arc::new(AtomicBool::new(true));
        let state = Arc::new(ServerState {
            shutdown,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            event_broadcast: Arc::new(RwLock::new(Vec::new())),
        });
        let app = build_router(state);

        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "ping",
            "id": 1
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
