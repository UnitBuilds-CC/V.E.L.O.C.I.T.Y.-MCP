//! Load benchmarks for velocity-mcp-edge.
//!
//! Measures:
//! - Direct protocol processing throughput (no HTTP overhead)
//! - HTTP round-trip latency under load
//! - Concurrent connection handling

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use std::convert::Infallible;
use std::net::SocketAddr;
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use http_body_util::Full;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use velocity_mcp_edge::{process_mcp_request, error_response};

// ---------------------------------------------------------------------------
// Direct protocol benchmarks (no HTTP)
// ---------------------------------------------------------------------------

fn bench_process_mcp_request(c: &mut Criterion) {
    let mut group = c.benchmark_group("protocol_processing");
    group.throughput(Throughput::Elements(1));

    let ping_request = br#"{"jsonrpc":"2.0","method":"ping","id":1}"#;
    let init_request = br#"{"jsonrpc":"2.0","method":"initialize","params":{},"id":1}"#;
    let tools_list = br#"{"jsonrpc":"2.0","method":"tools/list","id":1}"#;
    let tools_call = br#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"test","arguments":{}},"id":1}"#;
    let invalid_json = b"not valid json";

    group.bench_function("ping", |b| {
        b.iter(|| process_mcp_request(black_box(ping_request)))
    });

    group.bench_function("initialize", |b| {
        b.iter(|| process_mcp_request(black_box(init_request)))
    });

    group.bench_function("tools_list", |b| {
        b.iter(|| process_mcp_request(black_box(tools_list)))
    });

    group.bench_function("tools_call", |b| {
        b.iter(|| process_mcp_request(black_box(tools_call)))
    });

    group.bench_function("invalid_json", |b| {
        b.iter(|| process_mcp_request(black_box(invalid_json)))
    });

    group.finish();
}

fn bench_error_response_bytes(c: &mut Criterion) {
    let mut group = c.benchmark_group("error_formatting");
    group.bench_function("error_response_bytes", |b| {
        b.iter(|| {
            velocity_mcp_edge::error_response_bytes(black_box("test error message"))
        })
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// HTTP round-trip benchmarks (with server)
// ---------------------------------------------------------------------------

/// Same handler as main.rs for benchmark server.
async fn bench_handler(
    req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path();
    let method = req.method().clone();

    if path == "/health" {
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .body(Full::new(Bytes::from(r#"{"status":"healthy"}"#)))
            .unwrap());
    }

    if method == hyper::Method::POST && (path == "/mcp" || path == "/") {
        use http_body_util::BodyExt;
        let body_bytes = match req.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(_) => {
                return Ok(error_response(StatusCode::BAD_REQUEST, "read error"));
            }
        };
        let response_bytes = process_mcp_request(&body_bytes);
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(response_bytes)))
            .unwrap());
    }

    Ok(error_response(StatusCode::NOT_FOUND, "not found"))
}

async fn start_bench_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let base = format!("http://{}", addr);

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(c) => c,
                Err(_) => break,
            };
            let io = TokioIo::new(stream);
            tokio::task::spawn(async move {
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service_fn(bench_handler))
                    .await;
            });
        }
    });

    tokio::task::yield_now().await;
    base
}

fn bench_http_roundtrip(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let base = rt.block_on(start_bench_server());

    let mut group = c.benchmark_group("http_roundtrip");
    group.throughput(Throughput::Elements(1));
    group.measurement_time(std::time::Duration::from_secs(5));

    let ping_body = br#"{"jsonrpc":"2.0","method":"ping","id":1}"#;
    let init_body = br#"{"jsonrpc":"2.0","method":"initialize","params":{},"id":1}"#;

    group.bench_function("ping_http", |b| {
        b.iter(|| {
            rt.block_on(async {
                let client = reqwest::Client::new();
                let resp = client
                    .post(format!("{}/mcp", base))
                    .body(ping_body.as_slice())
                    .header("content-type", "application/json")
                    .send()
                    .await
                    .unwrap();
                let _ = resp.text().await.unwrap();
            });
        })
    });

    group.bench_function("initialize_http", |b| {
        b.iter(|| {
            rt.block_on(async {
                let client = reqwest::Client::new();
                let resp = client
                    .post(format!("{}/mcp", base))
                    .body(init_body.as_slice())
                    .header("content-type", "application/json")
                    .send()
                    .await
                    .unwrap();
                let _ = resp.text().await.unwrap();
            });
        })
    });

    group.bench_function("health_http", |b| {
        b.iter(|| {
            rt.block_on(async {
                let client = reqwest::Client::new();
                let resp = client
                    .get(format!("{}/health", base))
                    .send()
                    .await
                    .unwrap();
                let _ = resp.text().await.unwrap();
            });
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Concurrent connection benchmarks
// ---------------------------------------------------------------------------

fn bench_concurrent_connections(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let base = rt.block_on(start_bench_server());

    let mut group = c.benchmark_group("concurrent");

    for concurrency in [10, 50, 100] {
        group.bench_function(BenchmarkId::new("concurrent_ping", concurrency), |b| {
            b.iter(|| {
                rt.block_on(async {
                    let mut handles = Vec::with_capacity(concurrency);
                    for _ in 0..concurrency {
                        let b = base.clone();
                        handles.push(tokio::spawn(async move {
                            let client = reqwest::Client::new();
                            let resp = client
                                .post(format!("{}/mcp", b))
                                .body(r#"{"jsonrpc":"2.0","method":"ping","id":1}"#)
                                .header("content-type", "application/json")
                                .send()
                                .await
                                .unwrap();
                            assert_eq!(resp.status(), 200);
                        }));
                    }
                    for h in handles {
                        h.await.unwrap();
                    }
                });
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_process_mcp_request,
    bench_error_response_bytes,
    bench_http_roundtrip,
    bench_concurrent_connections,
);
criterion_main!(benches);
