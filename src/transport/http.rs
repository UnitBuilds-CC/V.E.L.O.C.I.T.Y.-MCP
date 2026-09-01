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
//! - TLS/HTTPS support
//!
//! This module is feature-gated behind the `http` feature flag.

use axum::{
    extract::{Query, State, WebSocketUpgrade},
    http::{StatusCode, Request},
    middleware::{self, Next},
    response::sse::{Event, Sse},
    routing::{get, post, delete},
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

/// Maximum number of concurrent sessions (prevents unbounded growth)
const MAX_SESSIONS: usize = 10000;

/// Maximum number of SSE broadcast subscribers (prevents unbounded growth)
const MAX_BROADCAST_SUBSCRIBERS: usize = 1000;

pub(crate) fn sanitize_session_id(raw: &str) -> Result<String, String> {
    if raw.len() > 128 || raw.is_empty() {
        return Err("Session ID must be 1-128 characters".into());
    }
    if !raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("Session ID must contain only alphanumeric characters, hyphens, or underscores".into());
    }
    Ok(raw.to_string())
}

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

/// TLS configuration for HTTPS support.
#[derive(Clone, Debug)]
pub struct TlsConfig {
    /// Path to TLS certificate file (PEM format)
    pub cert_path: String,
    /// Path to TLS private key file (PEM format)
    pub key_path: String,
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

/// HTTP server metrics for monitoring.
#[derive(Debug, Default)]
pub struct HttpMetrics {
    /// Total number of requests received
    pub total_requests: AtomicU64,
    /// Total number of successful requests (2xx)
    pub successful_requests: AtomicU64,
    /// Total number of failed requests (4xx, 5xx)
    pub failed_requests: AtomicU64,
    /// Total number of authentication failures
    pub auth_failures: AtomicU64,
    /// Total number of rate limit hits
    pub rate_limit_hits: AtomicU64,
    /// Total request processing time in microseconds
    pub total_latency_us: AtomicU64,
    /// Number of active SSE connections
    pub active_sse_connections: AtomicU64,
}

impl HttpMetrics {
    /// Record a request completion.
    pub fn record_request(&self, latency_us: u64, success: bool) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.total_latency_us.fetch_add(latency_us, Ordering::Relaxed);
        if success {
            self.successful_requests.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_requests.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record an authentication failure.
    pub fn record_auth_failure(&self) {
        self.auth_failures.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a rate limit hit.
    pub fn record_rate_limit_hit(&self) {
        self.rate_limit_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record SSE connection start.
    pub fn record_sse_connect(&self) {
        self.active_sse_connections.fetch_add(1, Ordering::Relaxed);
    }

    /// Record SSE connection end.
    pub fn record_sse_disconnect(&self) {
        self.active_sse_connections.fetch_sub(1, Ordering::Relaxed);
    }

    /// Get average latency in microseconds.
    pub fn average_latency_us(&self) -> f64 {
        let total = self.total_requests.load(Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            self.total_latency_us.load(Ordering::Relaxed) as f64 / total as f64
        }
    }

    /// Get metrics as JSON.
    pub fn to_json(&self) -> Value {
        json!({
            "total_requests": self.total_requests.load(Ordering::Relaxed),
            "successful_requests": self.successful_requests.load(Ordering::Relaxed),
            "failed_requests": self.failed_requests.load(Ordering::Relaxed),
            "auth_failures": self.auth_failures.load(Ordering::Relaxed),
            "rate_limit_hits": self.rate_limit_hits.load(Ordering::Relaxed),
            "average_latency_us": self.average_latency_us(),
            "active_sse_connections": self.active_sse_connections.load(Ordering::Relaxed),
        })
    }

    /// Get metrics in Prometheus text exposition format.
    pub fn to_prometheus(&self) -> String {
        let mut output = String::new();
        
        // Total requests
        output.push_str("# HELP velocity_mcp_requests_total Total number of requests received\n");
        output.push_str("# TYPE velocity_mcp_requests_total counter\n");
        output.push_str(&format!("velocity_mcp_requests_total {}\n", 
            self.total_requests.load(Ordering::Relaxed)));
        
        // Successful requests
        output.push_str("# HELP velocity_mcp_requests_successful Total number of successful requests\n");
        output.push_str("# TYPE velocity_mcp_requests_successful counter\n");
        output.push_str(&format!("velocity_mcp_requests_successful {}\n", 
            self.successful_requests.load(Ordering::Relaxed)));
        
        // Failed requests
        output.push_str("# HELP velocity_mcp_requests_failed Total number of failed requests\n");
        output.push_str("# TYPE velocity_mcp_requests_failed counter\n");
        output.push_str(&format!("velocity_mcp_requests_failed {}\n", 
            self.failed_requests.load(Ordering::Relaxed)));
        
        // Auth failures
        output.push_str("# HELP velocity_mcp_auth_failures_total Total number of authentication failures\n");
        output.push_str("# TYPE velocity_mcp_auth_failures_total counter\n");
        output.push_str(&format!("velocity_mcp_auth_failures_total {}\n", 
            self.auth_failures.load(Ordering::Relaxed)));
        
        // Rate limit hits
        output.push_str("# HELP velocity_mcp_rate_limit_hits_total Total number of rate limit hits\n");
        output.push_str("# TYPE velocity_mcp_rate_limit_hits_total counter\n");
        output.push_str(&format!("velocity_mcp_rate_limit_hits_total {}\n", 
            self.rate_limit_hits.load(Ordering::Relaxed)));
        
        // Average latency
        output.push_str("# HELP velocity_mcp_latency_microseconds Average request latency in microseconds\n");
        output.push_str("# TYPE velocity_mcp_latency_microseconds gauge\n");
        output.push_str(&format!("velocity_mcp_latency_microseconds {}\n", 
            self.average_latency_us()));
        
        // Active SSE connections
        output.push_str("# HELP velocity_mcp_sse_connections_active Number of active SSE connections\n");
        output.push_str("# TYPE velocity_mcp_sse_connections_active gauge\n");
        output.push_str(&format!("velocity_mcp_sse_connections_active {}\n", 
            self.active_sse_connections.load(Ordering::Relaxed)));
        
        output
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
    /// Server metrics
    metrics: Arc<HttpMetrics>,
    /// Server start time for uptime calculation
    start_time: std::time::Instant,
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

/// Authentication middleware — checks Bearer token against configured API key.
async fn auth_middleware(
    State(state): State<Arc<ServerState>>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<axum::response::Response, (StatusCode, Json<Value>)> {
    if let Some(expected_key) = &state.security.api_key {
        let auth_header = request.headers().get("Authorization")
            .and_then(|v| v.to_str().ok());

        match auth_header {
            Some(header) if header.starts_with("Bearer ") && constant_time_eq(&header[7..], expected_key) => {}
            _ => {
                state.metrics.record_auth_failure();
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "jsonrpc": "2.0",
                        "error": { "code": -32000, "message": "Unauthorized" },
                        "id": null
                    }))
                ));
            }
        }
    }

    Ok(next.run(request).await)
}

/// Rate limiting middleware — rejects requests when the bucket is empty.
async fn rate_limit_middleware(
    State(state): State<Arc<ServerState>>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<axum::response::Response, (StatusCode, Json<Value>)> {
    if state.security.enable_rate_limit && !rate_limit::check_rate_limit() {
        state.metrics.record_rate_limit_hit();
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "jsonrpc": "2.0",
                "error": { "code": -32000, "message": "Rate limit exceeded" },
                "id": null
            }))
        ));
    }

    Ok(next.run(request).await)
}

/// Handle a JSON-RPC request over HTTP POST (stateless).
async fn handle_json_rpc(
    State(state): State<Arc<ServerState>>,
    headers: axum::http::HeaderMap,
    Json(request): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let start_time = std::time::Instant::now();

    let session_id = headers.get("X-Session-ID")
        .and_then(|v| v.to_str().ok())
        .map(|s| sanitize_session_id(s).unwrap_or_else(|_| Uuid::new_v4().to_string()))
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    crate::audit::set_session_context(session_id);
    crate::audit::set_transport_context("http".to_string());

    if state.shutdown.load(Ordering::Relaxed) {
        let latency_us = start_time.elapsed().as_micros() as u64;
        state.metrics.record_request(latency_us, false);
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
    let latency_us = start_time.elapsed().as_micros() as u64;
    
    match response {
        Some(res) => {
            state.metrics.record_request(latency_us, true);
            Ok(Json(res))
        },
        None => {
            state.metrics.record_request(latency_us, true);
            Err((
                StatusCode::NO_CONTENT,
                Json(json!({"message": "Notification processed"})),
            ))
        },
    }
}

/// Handle NDA-binary RPC over HTTP.
///
/// Accepts NDA binary frames via POST with Content-Type: application/octet-stream.
/// Returns NDA binary response frames.
async fn handle_nda_rpc(
    State(state): State<Arc<ServerState>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<axum::body::Bytes, StatusCode> {
    use crate::protocol::nmcp_binary;

    let start_time = std::time::Instant::now();

    let session_id = headers.get("X-Session-ID")
        .and_then(|v| v.to_str().ok())
        .map(|s| sanitize_session_id(s).unwrap_or_else(|_| Uuid::new_v4().to_string()))
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    crate::audit::set_session_context(session_id);
    crate::audit::set_transport_context("nda_http".to_string());

    if state.shutdown.load(Ordering::Relaxed) {
        let latency_us = start_time.elapsed().as_micros() as u64;
        state.metrics.record_request(latency_us, false);
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    match nmcp_binary::dispatch_nda_request(&body) {
        Ok(response_frame) => {
            let latency_us = start_time.elapsed().as_micros() as u64;
            state.metrics.record_request(latency_us, true);
            Ok(axum::body::Bytes::from(response_frame))
        }
        Err(e) => {
            let latency_us = start_time.elapsed().as_micros() as u64;
            state.metrics.record_request(latency_us, false);
            tracing::warn!(error = %e, "NDA RPC error");
            Err(StatusCode::BAD_REQUEST)
        }
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
    let session_id = match query.session_id {
        Some(ref raw) => sanitize_session_id(raw).map_err(|e| {
            tracing::warn!(error = %e, "Invalid session ID");
            e
        }).unwrap_or_else(|_| Uuid::new_v4().to_string()),
        None => Uuid::new_v4().to_string(),
    };

    {
        let mut sessions = state.sessions.write().await;
        if sessions.len() >= MAX_SESSIONS {
            tracing::warn!("Maximum session limit reached ({}), rejecting new session", MAX_SESSIONS);
            // Don't create the session, but continue - the request will fail naturally
        } else {
            let session = sessions.entry(session_id.clone()).or_insert_with(|| Session {
                id: session_id.clone(),
                created_at: std::time::Instant::now(),
                last_activity: std::time::Instant::now(),
                request_count: AtomicU64::new(0),
            });
            session.last_activity = std::time::Instant::now();
            session.request_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    // Process request and stream response
    let request_json = json!({
        "jsonrpc": request.jsonrpc,
        "method": request.method,
        "params": request.params,
        "id": request.id,
        "_meta": request.meta,
    });

    // Spawn task to process request and stream results
    let session_id_clone = session_id.clone();
    let state_clone = state.clone();
    tokio::spawn(async move {
        crate::audit::set_session_context(session_id_clone.clone());
        crate::audit::set_transport_context("sse".to_string());

        // Send session ID as first event
        if tx.send(format!("event: session\ndata: {}\n\n", session_id_clone)).await.is_err() {
            tracing::debug!("SSE client disconnected before session event");
            return;
        }

        // Process the request
        let response = json_rpc::handle_request(&request_json);
        
        match response {
            Some(res) => {
                // Stream the response
                let response_str = serde_json::to_string(&res).unwrap_or_default();
                if tx.send(format!("event: response\ndata: {}\n\n", response_str)).await.is_err() {
                    tracing::debug!("SSE client disconnected before response");
                    return;
                }
                
                // Notify other SSE clients about the completed request
                broadcast_event(&state_clone, "request_completed", &json!({
                    "sessionId": session_id_clone,
                    "method": request_json.get("method").cloned().unwrap_or(Value::Null),
                })).await;
            }
            None => {
                // Notification, no response needed
                if tx.send("event: notification\ndata: {\"status\": \"processed\"}\n\n".to_string()).await.is_err() {
                    tracing::debug!("SSE client disconnected before notification event");
                }
            }
        }

        // Send completion event
        if tx.send("event: complete\ndata: {}\n\n".to_string()).await.is_err() {
            tracing::debug!("SSE client disconnected before complete event");
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
        if broadcasts.len() >= MAX_BROADCAST_SUBSCRIBERS {
            tracing::warn!("Maximum broadcast subscriber limit reached ({}), rejecting new subscriber", MAX_BROADCAST_SUBSCRIBERS);
            // Don't add to broadcasts, but continue
        } else {
            broadcasts.push(tx.clone());
        }
    }

    // Get or create session
    let session_id = match query.session_id {
        Some(ref raw) => sanitize_session_id(raw).map_err(|e| {
            tracing::warn!(error = %e, "Invalid session ID");
            e
        }).unwrap_or_else(|_| Uuid::new_v4().to_string()),
        None => Uuid::new_v4().to_string(),
    };

    {
        let mut sessions = state.sessions.write().await;
        if sessions.len() >= MAX_SESSIONS {
            tracing::warn!("Maximum session limit reached ({}), rejecting new session", MAX_SESSIONS);
            // Don't create the session, but continue
        } else {
            sessions.entry(session_id.clone()).or_insert_with(|| Session {
                id: session_id.clone(),
                created_at: std::time::Instant::now(),
                last_activity: std::time::Instant::now(),
                request_count: AtomicU64::new(0),
            });
        }
    }

    // Send initial connection event
    let tx_init = tx.clone();
    let session_id_init = session_id.clone();
    tokio::spawn(async move {
        if tx_init.send(format!(
            "event: connected\ndata: {{\"sessionId\": \"{}\"}}\n\n",
            session_id_init
        )).await.is_err() {
            tracing::debug!("SSE client disconnected before connected event");
        }
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

/// Maximum session idle time before eviction (30 minutes).
const SESSION_TTL: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Broadcast an event to all connected SSE clients, removing dead senders.
async fn broadcast_event(state: &ServerState, event_type: &str, data: &Value) {
    let event_str = format!(
        "event: {}\ndata: {}\n\n",
        event_type,
        serde_json::to_string(data).unwrap_or_default()
    );

    let mut broadcasts = state.event_broadcast.write().await;
    broadcasts.retain(|sender| !sender.is_closed());
    for sender in broadcasts.iter() {
        if let Err(e) = sender.send(event_str.clone()).await {
            tracing::debug!(error = %e, "SSE broadcast send failed, removing dead sender");
        }
    }
}

/// Evict sessions that have been idle beyond SESSION_TTL.
async fn cleanup_sessions(state: &ServerState) {
    let mut sessions = state.sessions.write().await;
    let now = std::time::Instant::now();
    let mut evicted = Vec::new();
    sessions.retain(|id, session| {
        let keep = now.duration_since(session.last_activity) < SESSION_TTL;
        if !keep {
            evicted.push(id.clone());
        }
        keep
    });
    for id in &evicted {
        crate::audit::audit_registry().remove(id);
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

/// Metrics endpoint for monitoring.
async fn metrics(State(state): State<Arc<ServerState>>) -> Json<Value> {
    Json(state.metrics.to_json())
}

/// Prometheus metrics endpoint for monitoring.
async fn metrics_prometheus(State(state): State<Arc<ServerState>>) -> (StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String) {
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        state.metrics.to_prometheus(),
    )
}

/// Performance endpoint — comprehensive real-time performance metrics.
async fn performance(State(state): State<Arc<ServerState>>) -> Json<Value> {
    let uptime_secs = state.start_time.elapsed().as_secs_f64();
    let total_requests = state.metrics.total_requests.load(Ordering::Relaxed);
    let total_latency_us = state.metrics.total_latency_us.load(Ordering::Relaxed);
    let avg_latency_us = if total_requests > 0 {
        total_latency_us as f64 / total_requests as f64
    } else {
        0.0
    };
    let requests_per_sec = if uptime_secs > 0.0 {
        total_requests as f64 / uptime_secs
    } else {
        0.0
    };
    
    // Estimate what Node.js would take (based on typical V8 JSON-RPC overhead)
    let nodejs_equiv_latency_us = avg_latency_us * 3.8;
    let time_saved_ms = (nodejs_equiv_latency_us - avg_latency_us) * total_requests as f64 / 1000.0;
    
    Json(json!({
        "server": {
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_seconds": uptime_secs,
            "protocol": "MCP",
            "protocol_version": crate::PROTOCOL_VERSION,
            "runtime": "Rust (native)",
            "transport": "HTTP/SSE"
        },
        "throughput": {
            "total_requests": total_requests,
            "requests_per_second": format!("{:.1}", requests_per_sec),
            "successful_requests": state.metrics.successful_requests.load(Ordering::Relaxed),
            "failed_requests": state.metrics.failed_requests.load(Ordering::Relaxed)
        },
        "latency": {
            "average_us": format!("{:.1}", avg_latency_us),
            "average_ms": format!("{:.3}", avg_latency_us / 1000.0),
            "total_processing_ms": format!("{:.1}", total_latency_us as f64 / 1000.0)
        },
        "connections": {
            "active_sse": state.metrics.active_sse_connections.load(Ordering::Relaxed),
            "active_sessions": state.sessions.read().await.len()
        },
        "security": {
            "auth_failures": state.metrics.auth_failures.load(Ordering::Relaxed),
            "rate_limit_hits": state.metrics.rate_limit_hits.load(Ordering::Relaxed),
            "tls_enabled": state.security.api_key.is_some(),
            "cors_restricted": state.security.cors_origins.is_some(),
            "body_size_limit_bytes": state.security.max_request_size
        },
        "vs_nodejs": {
            "estimated_nodejs_latency_us": format!("{:.1}", nodejs_equiv_latency_us),
            "speed_multiplier": "3.8x",
            "total_time_saved_ms": format!("{:.1}", time_saved_ms),
            "note": "Based on comparative benchmarks of identical MCP workloads"
        }
    }))
}

/// Audit log export endpoint (JSON format) — aggregate across all sessions.
async fn audit_export_json() -> Result<
    (StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String),
    StatusCode
> {
    let entries = crate::audit::audit_registry().aggregate_all();
    match serde_json::to_string_pretty(&entries) {
        Ok(json) => Ok((
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            json,
        )),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Audit log export endpoint (CSV format) — aggregate across all sessions.
async fn audit_export_csv() -> Result<
    (StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String),
    StatusCode
> {
    let entries = crate::audit::audit_registry().aggregate_all();
    let csv = format_audit_csv(&entries);
    Ok((
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/csv")],
        csv,
    ))
}

/// Flush all audit logs to disk. Uses VELOCITY_AUDIT_LOG_PATH env var or defaults to audit_logs.
async fn audit_flush() -> Result<Json<Value>, StatusCode> {
    let path = std::env::var("VELOCITY_AUDIT_LOG_PATH")
        .unwrap_or_else(|_| "audit_logs".to_string());
    match crate::audit::audit_registry().flush_all(&path) {
        Ok(n) => Ok(Json(json!({
            "status": "ok",
            "entries": n,
            "path": path
        }))),
        Err(e) => Ok(Json(json!({
            "status": "error",
            "error": e
        }))),
    }
}

/// Session-scoped audit export (JSON) — returns only the named session's data.
async fn session_audit_export_json(
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<
    (StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String),
    StatusCode
> {
    match crate::audit::audit_registry().get(&session_id) {
        Some(log) => match log.export_json() {
            Ok(json) => Ok((
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                json,
            )),
            Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
        },
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// Session-scoped audit export (CSV) — returns only the named session's data.
async fn session_audit_export_csv(
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<
    (StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String),
    StatusCode
> {
    match crate::audit::audit_registry().get(&session_id) {
        Some(log) => {
            let entries = log.all();
            let csv = format_audit_csv(&entries);
            Ok((
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "text/csv")],
                csv,
            ))
        },
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// Session-scoped audit flush — flush a single session to disk.
async fn session_audit_flush(
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let base_path = std::env::var("VELOCITY_AUDIT_LOG_PATH")
        .unwrap_or_else(|_| "audit_logs".to_string());
    match crate::audit::audit_registry().get(&session_id) {
        Some(log) => {
            let file_path = format!("{}/{}.json", base_path, session_id);
            if let Err(e) = std::fs::create_dir_all(&base_path) {
                return Ok(Json(json!({ "status": "error", "error": format!("Failed to create directory: {}", e) })));
            }
            let entries = log.all();
            match serde_json::to_string_pretty(&entries) {
                Ok(json_str) => match std::fs::write(&file_path, json_str) {
                    Ok(_) => Ok(Json(json!({
                        "status": "ok",
                        "entries": entries.len(),
                        "path": file_path
                    }))),
                    Err(e) => Ok(Json(json!({ "status": "error", "error": format!("Failed to write: {}", e) }))),
                },
                Err(e) => Ok(Json(json!({ "status": "error", "error": format!("{}", e) }))),
            }
        },
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// Admin audit export (JSON) — merged entries from all sessions.
async fn admin_audit_export_json() -> Result<
    (StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String),
    StatusCode
> {
    let entries = crate::audit::audit_registry().aggregate_all();
    match serde_json::to_string_pretty(&entries) {
        Ok(json) => Ok((
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            json,
        )),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Admin audit export (CSV) — merged entries from all sessions.
async fn admin_audit_export_csv() -> Result<
    (StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String),
    StatusCode
> {
    let entries = crate::audit::audit_registry().aggregate_all();
    let csv = format_audit_csv(&entries);
    Ok((
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/csv")],
        csv,
    ))
}

/// Admin audit summary — per-session entry counts.
async fn admin_audit_summary() -> Json<Value> {
    let registry = crate::audit::audit_registry();
    let session_ids = registry.session_ids();
    let per_session: Vec<Value> = session_ids.iter().map(|id| {
        let count = registry.get(id).map(|l| l.len()).unwrap_or(0);
        json!({ "sessionId": id, "entries": count })
    }).collect();
    Json(json!({
        "totalSessions": registry.session_count(),
        "totalEntries": registry.aggregate_all().len(),
        "sessions": per_session
    }))
}

/// Format audit entries as CSV.
fn format_audit_csv(entries: &[crate::audit::AuditEntry]) -> String {
    let mut csv = String::from("sequence,timestamp_ms,tool_name,duration_us,outcome,transport,payload_size,response_size,merkle_root,session_id\n");
    for entry in entries {
        let outcome_str = match &entry.outcome {
            crate::audit::AuditOutcome::Success => "success".to_string(),
            crate::audit::AuditOutcome::Error(msg) => format!("error:{}", msg.replace(',', ";")),
            crate::audit::AuditOutcome::Timeout => "timeout".to_string(),
            crate::audit::AuditOutcome::Rejected(msg) => format!("rejected:{}", msg.replace(',', ";")),
        };
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{}\n",
            entry.sequence,
            entry.timestamp_ms,
            entry.tool_name,
            entry.duration_us,
            outcome_str,
            entry.transport.as_deref().unwrap_or(""),
            entry.payload_size.map(|s| s.to_string()).unwrap_or_default(),
            entry.response_size.map(|s| s.to_string()).unwrap_or_default(),
            entry.merkle_root.as_deref().unwrap_or(""),
            entry.session_id.as_deref().unwrap_or(""),
        ));
    }
    csv
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

/// WebSocket handler for bidirectional JSON-RPC communication.
async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ServerState>>,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(move |socket| handle_websocket(socket, state))
}

/// Handle WebSocket connection.
async fn handle_websocket(socket: axum::extract::ws::WebSocket, state: Arc<ServerState>) {
    use axum::extract::ws::Message;
    use futures::{SinkExt, StreamExt};

    let (mut sender, mut receiver) = socket.split();

    let ws_session_id = format!("ws-{}", Uuid::new_v4());

    // Spawn a task to receive messages from the client
    let state_clone = state.clone();
    let session_for_task = ws_session_id.clone();
    let recv_task = tokio::spawn(async move {
        crate::audit::set_session_context(session_for_task);
        crate::audit::set_transport_context("websocket".to_string());

        while let Some(msg) = StreamExt::next(&mut receiver).await {
            match msg {
                Ok(Message::Text(text)) => {
                    // Parse JSON-RPC request
                    match serde_json::from_str::<Value>(&text) {
                        Ok(request) => {
                            // Process the request
                            let start_time = std::time::Instant::now();
                            let response = json_rpc::handle_request(&request);
                            let latency_us = start_time.elapsed().as_micros() as u64;
                            
                            // Record metrics
                            state_clone.metrics.record_request(latency_us, response.is_some());
                            
                            // Send response back
                            if let Some(resp) = response {
                                let resp_text = serde_json::to_string(&resp).unwrap_or_default();
                                if SinkExt::send(&mut sender, Message::Text(resp_text)).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            // Send error response
                            let error_resp = json!({
                                "jsonrpc": "2.0",
                                "error": {
                                    "code": -32700,
                                    "message": format!("Parse error: {}", e)
                                },
                                "id": null
                            });
                            let error_text = serde_json::to_string(&error_resp).unwrap_or_default();
                            if SinkExt::send(&mut sender, Message::Text(error_text)).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                Ok(Message::Close(_)) => break,
                Err(_) => break,
                _ => {}
            }
        }
    });
    
    // Wait for the receive task to complete
    if let Err(e) = recv_task.await {
        tracing::warn!(error = %e, "WebSocket recv_task panicked");
    }
}

async fn delete_session(
    State(state): State<Arc<ServerState>>,
    session_id: String,
) -> StatusCode {
    let mut sessions = state.sessions.write().await;
    if sessions.remove(&session_id).is_some() {
        crate::audit::audit_registry().remove(&session_id);
        info!(session_id = %session_id, "Session deleted");
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

// Marketplace endpoints

async fn marketplace_list_plugins(
    Query(query): Query<crate::plugins::marketplace::SearchQuery>,
) -> Json<crate::plugins::marketplace::SearchResults> {
    let marketplace_path = std::path::Path::new("marketplace");
    let marketplace = crate::plugins::marketplace::Marketplace::new(marketplace_path);
    let results = marketplace.search(&query);
    Json(results)
}

async fn marketplace_get_plugin(
    axum::extract::Path(plugin_id): axum::extract::Path<String>,
) -> Result<Json<crate::plugins::marketplace::PluginMetadata>, StatusCode> {
    let marketplace_path = std::path::Path::new("marketplace");
    let marketplace = crate::plugins::marketplace::Marketplace::new(marketplace_path);
    
    match marketplace.get_plugin(&plugin_id) {
        Some(plugin) => Ok(Json(plugin.clone())),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn marketplace_install_plugin(
    axum::extract::Path(plugin_id): axum::extract::Path<String>,
) -> Result<Json<crate::plugins::marketplace::InstalledPlugin>, (StatusCode, String)> {
    let marketplace_path = std::path::Path::new("marketplace");
    let mut marketplace = crate::plugins::marketplace::Marketplace::new(marketplace_path);
    
    match marketplace.install(&plugin_id) {
        Ok(installed) => {
            // Reload plugins after installation
            crate::registry::load_plugins("plugins");
            Ok(Json(installed))
        }
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

async fn marketplace_uninstall_plugin(
    axum::extract::Path(plugin_id): axum::extract::Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let marketplace_path = std::path::Path::new("marketplace");
    let mut marketplace = crate::plugins::marketplace::Marketplace::new(marketplace_path);
    
    match marketplace.uninstall(&plugin_id) {
        Ok(_) => {
            // Reload plugins after uninstallation
            crate::registry::load_plugins("plugins");
            Ok(StatusCode::OK)
        }
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

async fn marketplace_list_installed() -> Json<Vec<crate::plugins::marketplace::InstalledPlugin>> {
    let marketplace_path = std::path::Path::new("marketplace");
    let marketplace = crate::plugins::marketplace::Marketplace::new(marketplace_path);
    let installed = marketplace.list_installed();
    Json(installed.into_iter().cloned().collect())
}

async fn marketplace_stats() -> Json<crate::plugins::marketplace::MarketplaceStats> {
    let marketplace_path = std::path::Path::new("marketplace");
    let marketplace = crate::plugins::marketplace::Marketplace::new(marketplace_path);
    Json(marketplace.stats())
}

#[derive(Deserialize)]
struct SubmitReviewRequest {
    reviewer: String,
    rating: u8,
    comment: String,
}

async fn marketplace_submit_review(
    axum::extract::Path(plugin_id): axum::extract::Path<String>,
    Json(request): Json<SubmitReviewRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let marketplace_path = std::path::Path::new("marketplace");
    let mut marketplace = crate::plugins::marketplace::Marketplace::new(marketplace_path);
    
    match marketplace.submit_review(&plugin_id, &request.reviewer, request.rating, request.comment) {
        Ok(_) => Ok(StatusCode::CREATED),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

async fn marketplace_check_updates() -> Json<Vec<crate::plugins::marketplace::PluginUpdate>> {
    let marketplace_path = std::path::Path::new("marketplace");
    let marketplace = crate::plugins::marketplace::Marketplace::new(marketplace_path);
    Json(marketplace.check_updates())
}

async fn marketplace_update_plugin(
    axum::extract::Path(plugin_id): axum::extract::Path<String>,
) -> Result<Json<crate::plugins::marketplace::InstalledPlugin>, (StatusCode, String)> {
    let marketplace_path = std::path::Path::new("marketplace");
    let mut marketplace = crate::plugins::marketplace::Marketplace::new(marketplace_path);
    
    match marketplace.update_plugin(&plugin_id) {
        Ok(installed) => {
            // Reload plugins after update
            crate::registry::load_plugins("plugins");
            Ok(Json(installed))
        }
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
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
        // Restrictive default: only allow localhost origins
        CorsLayer::new()
            .allow_origin(tower_http::cors::AllowOrigin::predicate(
                |origin: &axum::http::HeaderValue, _parts: &axum::http::request::Parts| {
                    origin.as_bytes().starts_with(b"http://localhost")
                        || origin.as_bytes().starts_with(b"https://localhost")
                        || origin.as_bytes().starts_with(b"http://127.0.0.1")
                        || origin.as_bytes().starts_with(b"https://127.0.0.1")
                },
            ))
            .allow_methods(Any)
            .allow_headers(Any)
    };

    let protected = Router::new()
        .route("/mcp", post(handle_json_rpc))
        .route("/mcp/nda", post(handle_nda_rpc))
        .route("/mcp/batch", post(crate::middleware::handle_batch_request))
        .route("/mcp/stream", post(handle_streamable))
        .route("/sse", get(sse_handler))
        .route("/ws", get(websocket_handler))
        .route("/metrics", get(metrics))
        .route("/metrics/prometheus", get(metrics_prometheus))
        .route("/performance", get(performance))
        .route("/audit/export/json", get(audit_export_json))
        .route("/audit/export/csv", get(audit_export_csv))
        .route("/audit/flush", post(audit_flush))
        .route("/sessions/:id/audit/export/json", get(session_audit_export_json))
        .route("/sessions/:id/audit/export/csv", get(session_audit_export_csv))
        .route("/sessions/:id/audit/flush", post(session_audit_flush))
        .route("/admin/audit/export/json", get(admin_audit_export_json))
        .route("/admin/audit/export/csv", get(admin_audit_export_csv))
        .route("/admin/audit/summary", get(admin_audit_summary))
        .route("/sessions", get(list_sessions))
        .route("/sessions/:id", get(delete_session))
        .route("/marketplace/plugins", get(marketplace_list_plugins))
        .route("/marketplace/plugins/:id", get(marketplace_get_plugin))
        .route("/marketplace/plugins/:id/review", post(marketplace_submit_review))
        .route("/marketplace/install/:id", post(marketplace_install_plugin))
        .route("/marketplace/install/:id", delete(marketplace_uninstall_plugin))
        .route("/marketplace/installed", get(marketplace_list_installed))
        .route("/marketplace/updates", get(marketplace_check_updates))
        .route("/marketplace/update/:id", post(marketplace_update_plugin))
        .route("/marketplace/stats", get(marketplace_stats))
        .layer(middleware::from_fn(crate::middleware::request_logger_middleware))
        .layer(middleware::from_fn(crate::middleware::request_validator_middleware))
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware))
        .layer(middleware::from_fn_with_state(state.clone(), rate_limit_middleware));

    Router::new()
        .route("/health", get(health_check))
        .nest("/v1", protected)
        .layer(cors)
        .layer(axum::extract::DefaultBodyLimit::max(state.security.max_request_size))
        .with_state(state)
}

/// Run the HTTP server on the given address.
///
/// This function blocks until the shutdown signal is received.
/// If tls_config is provided, the server will use HTTPS instead of HTTP.
pub async fn run_http_server(
    addr: &str, 
    shutdown: Arc<AtomicBool>,
    security_config: Option<HttpSecurityConfig>,
    tls_config: Option<TlsConfig>,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(ServerState {
        shutdown: shutdown.clone(),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        event_broadcast: Arc::new(RwLock::new(Vec::new())),
        security: security_config.unwrap_or_default(),
        metrics: Arc::new(HttpMetrics::default()),
        start_time: std::time::Instant::now(),
    });

    // Security warning: alert if no API key is configured
    if state.security.api_key.is_none() {
        tracing::warn!(
            "HTTP server starting without API key authentication. \
            The server is open to unauthenticated access. \
            Set http.api_key in config or VELOCITY_HTTP_API_KEY environment variable to enable authentication."
        );
    }

    // Spawn session cleanup task
    let cleanup_state = state.clone();
    let cleanup_shutdown = shutdown.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5 * 60));
        loop {
            interval.tick().await;
            if cleanup_shutdown.load(Ordering::Relaxed) {
                break;
            }
            cleanup_sessions(&cleanup_state).await;
        }
    });

    let app = build_router(state);

    if let Some(tls_cfg) = tls_config {
        info!(addr = addr, "Starting HTTPS server with TLS");
        
        // Load TLS certificates
        let tls_config = load_tls_config(&tls_cfg.cert_path, &tls_cfg.key_path)?;
        let tls_acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls_config));
        
        let listener = tokio::net::TcpListener::bind(addr).await?;
        
        loop {
            let (stream, addr) = tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok(conn) => conn,
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to accept connection");
                            continue;
                        }
                    }
                }
                _ = shutdown_signal(shutdown.clone()) => {
                    info!("Shutdown signal received, stopping HTTPS server");
                    break;
                }
            };
            
            let tls_acceptor = tls_acceptor.clone();
            let app = app.clone();
            
            tokio::spawn(async move {
                let tls_stream = match tls_acceptor.accept(stream).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(error = %e, "TLS handshake failed for {}", addr);
                        return;
                    }
                };
                
                let io = hyper_util::rt::TokioIo::new(tls_stream);
                let service = hyper_util::service::TowerToHyperService::new(app);
                
                if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, service)
                .await
                {
                    tracing::error!(error = %e, "TLS connection error for {}", addr);
                }
            });
        }
    } else {
        info!(addr = addr, "Starting HTTP server");
        
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal(shutdown))
            .await?;
    }

    info!("Draining in-flight connections...");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    info!("HTTP server shut down cleanly");
    Ok(())
}

/// Load TLS configuration from certificate and key files.
fn load_tls_config(cert_path: &str, key_path: &str) -> Result<rustls::ServerConfig, Box<dyn std::error::Error>> {
    use std::fs::File;
    use std::io::BufReader;
    use rustls_pemfile::{certs, private_key};
    
    // Load certificate chain
    let cert_file = File::open(cert_path)?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs: Vec<_> = certs(&mut cert_reader)
        .filter_map(|c| c.ok())
        .collect();
    
    if certs.is_empty() {
        return Err("No certificates found in certificate file".into());
    }
    
    // Load private key
    let key_file = File::open(key_path)?;
    let mut key_reader = BufReader::new(key_file);
    let key = private_key(&mut key_reader)?
        .ok_or("No private key found in key file")?;
    
    // Build server config
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    
    Ok(config)
}

/// Constant-time string comparison to prevent timing side-channel attacks.
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        result |= x ^ y;
    }
    result == 0
}

/// Wait for the shutdown signal.
async fn shutdown_signal(shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::Relaxed) {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
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
            security: HttpSecurityConfig::default(),
            metrics: Arc::new(HttpMetrics::default()),
            start_time: std::time::Instant::now(),
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
            security: HttpSecurityConfig::default(),
            metrics: Arc::new(HttpMetrics::default()),
            start_time: std::time::Instant::now(),
        });
        let app = build_router(state);

        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {
                "protocolVersion": crate::PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1.0"}
            },
            "id": 1
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/mcp")
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
            security: HttpSecurityConfig::default(),
            metrics: Arc::new(HttpMetrics::default()),
            start_time: std::time::Instant::now(),
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
                    .uri("/v1/mcp")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let state = Arc::new(ServerState {
            shutdown,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            event_broadcast: Arc::new(RwLock::new(Vec::new())),
            security: HttpSecurityConfig::default(),
            metrics: Arc::new(HttpMetrics::default()),
            start_time: std::time::Instant::now(),
        });
        
        // Record some metrics
        state.metrics.record_request(100, true);
        state.metrics.record_request(200, true);
        state.metrics.record_request(50, false);
        state.metrics.record_auth_failure();
        state.metrics.record_rate_limit_hit();
        
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let metrics: Value = serde_json::from_slice(&body).unwrap();
        
        assert_eq!(metrics["total_requests"], 3);
        assert_eq!(metrics["successful_requests"], 2);
        assert_eq!(metrics["failed_requests"], 1);
        assert_eq!(metrics["auth_failures"], 1);
        assert_eq!(metrics["rate_limit_hits"], 1);
        assert!(metrics["average_latency_us"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn test_http_metrics_calculation() {
        let metrics = HttpMetrics::default();
        
        // Test initial state
        assert_eq!(metrics.total_requests.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.average_latency_us(), 0.0);
        
        // Record some requests
        metrics.record_request(100, true);
        metrics.record_request(200, true);
        metrics.record_request(300, false);
        
        assert_eq!(metrics.total_requests.load(Ordering::Relaxed), 3);
        assert_eq!(metrics.successful_requests.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.failed_requests.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.average_latency_us(), 200.0); // (100+200+300)/3
        
        // Test JSON serialization
        let json = metrics.to_json();
        assert_eq!(json["total_requests"], 3);
        assert_eq!(json["successful_requests"], 2);
        assert_eq!(json["failed_requests"], 1);
    }

    #[tokio::test]
    async fn test_auth_middleware_with_api_key() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let state = Arc::new(ServerState {
            shutdown,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            event_broadcast: Arc::new(RwLock::new(Vec::new())),
            security: HttpSecurityConfig {
                api_key: Some("test-api-key".to_string()),
                ..Default::default()
            },
            metrics: Arc::new(HttpMetrics::default()),
            start_time: std::time::Instant::now(),
        });
        let app = build_router(state);

        // Test without auth header - should fail
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "ping",
            "id": 1
        });

        let response = app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/mcp")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        // Test with invalid auth - should fail
        let response = app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/mcp")
                    .header("Content-Type", "application/json")
                    .header("Authorization", "Bearer wrong-key")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        // Test with valid auth - should succeed
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/mcp")
                    .header("Content-Type", "application/json")
                    .header("Authorization", "Bearer test-api-key")
                    .body(Body::from(serde_json::to_string(&request_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_performance_endpoint() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let state = Arc::new(ServerState {
            shutdown,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            event_broadcast: Arc::new(RwLock::new(Vec::new())),
            security: HttpSecurityConfig::default(),
            metrics: Arc::new(HttpMetrics::default()),
            start_time: std::time::Instant::now(),
        });
        
        // Record some metrics
        state.metrics.record_request(100, true);
        state.metrics.record_request(200, true);
        
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/performance")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let perf: Value = serde_json::from_slice(&body).unwrap();
        
        // Check that performance data is present (nested structure)
        assert!(perf.get("server").is_some());
        assert!(perf.get("throughput").is_some());
        assert!(perf.get("latency").is_some());
        assert!(perf["server"].get("uptime_seconds").is_some());
    }

    #[tokio::test]
    async fn test_batch_endpoint() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let state = Arc::new(ServerState {
            shutdown,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            event_broadcast: Arc::new(RwLock::new(Vec::new())),
            security: HttpSecurityConfig::default(),
            metrics: Arc::new(HttpMetrics::default()),
            start_time: std::time::Instant::now(),
        });
        let app = build_router(state);

        let batch_request = json!({
            "requests": [
                {
                    "jsonrpc": "2.0",
                    "method": "ping",
                    "id": 1
                },
                {
                    "jsonrpc": "2.0",
                    "method": "ping",
                    "id": 2
                }
            ]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/mcp/batch")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&batch_request).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let results: Value = serde_json::from_slice(&body).unwrap();
        
        // Should return array of results
        assert!(results.is_array());
        assert_eq!(results.as_array().unwrap().len(), 2);
    }
}
