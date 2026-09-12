//! VELOCITY-MCP WASIX HTTP server for Wasmer Edge deployment.
//!
//! This binary implements a full HTTP server using hyper that can run on Wasmer Edge
//! with WASIX (WASI + eXtensions) support. It wraps the velocity-mcp-core protocol logic.

use std::convert::Infallible;
use std::net::SocketAddr;
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use tracing::{info, error, warn};
use velocity_mcp_core::{handle_mcp_request, parse_request, serialize_response};

/// Handle incoming HTTP requests
async fn handle_request(
    req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Extract request path and method for logging
    let path = req.uri().path();
    let method = req.method().clone();
    
    info!(method = %method, path = path, "Received HTTP request");
    
    // Health check endpoint
    if path == "/health" || path == "/healthz" {
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(r#"{"status":"healthy","version":"3.2.0"}"#)))
            .unwrap());
    }
    
    // MCP endpoint - accept POST requests to /mcp or root
    if method == hyper::Method::POST && (path == "/mcp" || path == "/") {
        // Read request body
        let body_bytes = match req.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(e) => {
                error!(error = %e, "Failed to read request body");
                return Ok(error_response(StatusCode::BAD_REQUEST, "Failed to read request body"));
            }
        };
        
        // Process MCP request
        let response_bytes = process_mcp_request(&body_bytes);
        
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(response_bytes)))
            .unwrap());
    }
    
    // Method not allowed or not found
    Ok(error_response(StatusCode::NOT_FOUND, "Endpoint not found. Use POST /mcp"))
}

/// Process MCP JSON-RPC request and return response bytes
fn process_mcp_request(request_body: &[u8]) -> Vec<u8> {
    match parse_request(request_body) {
        Ok(request) => {
            let response = handle_mcp_request(&request);
            serialize_response(&response)
        }
        Err(e) => error_response_bytes(&format!("Parse error: {}", e)),
    }
}

/// Create error response as Response object
fn error_response(status: StatusCode, message: &str) -> Response<Full<Bytes>> {
    let error_json = serde_json::json!({
        "jsonrpc": "2.0",
        "error": {
            "code": status.as_u16() as i64,
            "message": message
        },
        "id": null
    });
    
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(serde_json::to_vec(&error_json).unwrap_or_default())))
        .unwrap()
}

/// Create error response as raw bytes
fn error_response_bytes(message: &str) -> Vec<u8> {
    let error_json = serde_json::json!({
        "jsonrpc": "2.0",
        "error": {
            "code": -32700,
            "message": message
        },
        "id": null
    });
    serde_json::to_vec(&error_json).unwrap_or_default()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    // Get port from environment or use default
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);
    
    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    
    info!(address = %addr, "Starting VELOCITY-MCP Edge server");
    info!(version = env!("CARGO_PKG_VERSION"), "Core protocol: velocity-mcp-core");
    
    // Create TCP listener
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(address = %addr, "Listening for connections");
    
    // Accept connections and process them
    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        
        // Spawn a task to handle the connection
        tokio::task::spawn(async move {
            if let Err(err) = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, service_fn(handle_request))
                .await
            {
                warn!(error = %err, "Error serving connection");
            }
        });
    }
}
