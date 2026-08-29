//! HTTP/SSE transport for MCP protocol.
//!
//! Provides an Axum-based HTTP server that speaks JSON-RPC over HTTP POST
//! and Server-Sent Events (SSE) for streaming responses.
//!
//! This module is feature-gated behind the `http` feature flag. When not
//! enabled, no HTTP dependencies are compiled and there is zero overhead.

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

use crate::protocol::json_rpc;

/// Shared state for the HTTP server.
struct ServerState {
    shutdown: Arc<AtomicBool>,
}

/// Handle a JSON-RPC request over HTTP POST.
async fn handle_json_rpc(
    State(state): State<Arc<ServerState>>,
    Json(request): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
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

/// SSE endpoint for streaming responses.
/// Clients connect here to receive real-time updates from long-running operations.
async fn sse_handler(
    State(_state): State<Arc<ServerState>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let (tx, rx) = mpsc::channel::<String>(100);

    // Spawn a task that sends heartbeat events to keep the connection alive
    tokio::spawn(async move {
        use tokio::time::{interval, Duration};
        let mut interval = interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            if tx.send(":heartbeat\n".to_string()).await.is_err() {
                break;
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(|msg| {
        Ok(Event::default().data(msg))
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("heartbeat"),
    )
}

/// Health check endpoint.
async fn health_check() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "transport": "http",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

/// Build the Axum router with all MCP endpoints.
fn build_router(state: Arc<ServerState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/mcp", post(handle_json_rpc))
        .route("/sse", get(sse_handler))
        .route("/health", get(health_check))
        .layer(cors)
        .with_state(state)
}

/// Run the HTTP server on the given address.
///
/// This function blocks until the shutdown signal is received.
pub async fn run_http_server(addr: &str, shutdown: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(ServerState {
        shutdown: shutdown.clone(),
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
        let state = Arc::new(ServerState { shutdown });
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
        let state = Arc::new(ServerState { shutdown });
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
        let state = Arc::new(ServerState { shutdown });
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
