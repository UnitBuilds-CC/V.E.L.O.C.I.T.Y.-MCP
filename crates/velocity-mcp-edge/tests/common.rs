//! Shared test helpers for integration and E2E tests.
//!
//! Provides a function to start the velocity-mcp-edge HTTP server on a random
//! available port and return a reqwest client + base URL.

use std::convert::Infallible;
use std::net::SocketAddr;
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use velocity_mcp_edge::{process_mcp_request, error_response};

/// The same request handler as main.rs, extracted for testing.
async fn handle_request(
    req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path();
    let method = req.method().clone();

    if path == "/health" || path == "/healthz" {
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(r#"{"status":"healthy","version":"3.2.0"}"#)))
            .unwrap());
    }

    if method == hyper::Method::POST && (path == "/mcp" || path == "/") {
        let body_bytes = match BodyExt::collect(req).await {
            Ok(collected) => collected.to_bytes(),
            Err(_) => {
                return Ok(error_response(
                    StatusCode::BAD_REQUEST,
                    "Failed to read request body",
                ));
            }
        };
        let response_bytes = process_mcp_request(&body_bytes);
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(response_bytes)))
            .unwrap());
    }

    Ok(error_response(
        StatusCode::NOT_FOUND,
        "Endpoint not found. Use POST /mcp",
    ))
}

/// Start the test server on a random port and return the base URL.
/// The server runs in a background task.
pub async fn start_test_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind test server");
    let addr: SocketAddr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(_) => break,
            };
            let io = TokioIo::new(stream);
            tokio::task::spawn(async move {
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service_fn(handle_request))
                    .await;
            });
        }
    });

    // Small yield to let the server task start
    tokio::task::yield_now().await;

    base_url
}
