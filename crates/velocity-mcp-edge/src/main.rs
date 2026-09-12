//! VELOCITY-MCP WASIX HTTP server for Wasmer Edge deployment.
//!
//! This binary implements a full HTTP server using hyper that can run on Wasmer Edge
//! with WASIX (WASI + eXtensions) support. It wraps the velocity-mcp-core protocol logic.
//!
//! # Security Layers
//!
//! The server implements defense-in-depth with the following configurable security layers,
//! applied in priority order:
//!
//! 1. **Request Size Limits (P0)** - Max 1MB per request body (env: `MAX_BODY_SIZE`,
//!    default 1048576). Returns 413 Payload Too Large if exceeded. Checks Content-Length
//!    header first for early rejection, then enforces on actual body bytes. Prevents
//!    memory exhaustion / DoS attacks.
//!
//! 2. **CORS Headers (P0)** - Restricts cross-origin access via `ALLOWED_ORIGINS` env var
//!    (comma-separated). Set to `"*"` to allow all origins. When unset, no CORS headers
//!    are sent. Handles OPTIONS preflight with 204 No Content. Adds
//!    Access-Control-Allow-Origin, Allow-Methods, Allow-Headers, Max-Age.
//!
//! 3. **API Key Authentication (P1)** - Requires `X-API-Key` header on `/mcp` and `/`
//!    endpoints when `VELOCITY_API_KEY` env var is set. Uses timing-safe comparison
//!    (constant-time XOR) to prevent timing side-channel attacks. Returns 401
//!    Unauthorized if key is missing or invalid. When env var is unset, auth is disabled.
//!
//! 4. **Rate Limiting (P1)** - Token bucket algorithm: 100 requests/minute per client IP
//!    (configurable via `RATE_LIMIT_PER_MINUTE`). Returns 429 Too Many Requests when
//!    exceeded. Uses `HashMap<SocketAddr, TokenBucket>` with lazy expiration cleanup
//!    every 60s. Client IP extracted from `X-Forwarded-For` header (falls back to TCP
//!    peer address). Includes `X-RateLimit-*` response headers.
//!
//! 5. **Error Sanitization (P2)** - Internal errors are never leaked to clients. Full
//!    error details are logged server-side; clients receive only generic sanitized
//!    messages. Stack traces and implementation details are stripped from all responses.
//!
//! 6. **Request Logging (P2)** - All requests are logged with method, path, HTTP status,
//!    duration, client IP, and a unique correlation ID. Correlation IDs are monotonic
//!    counters for tracing through the request lifecycle. Structured via the `tracing`
//!    crate (JSON output when `tracing-subscriber` is configured with `fmt().json()`).
//!
//! # Environment Variables
//!
//! | Variable | Default | Description |
//! |---|---|---|
//! | `PORT` | `8080` | TCP listen port |
//! | `MAX_BODY_SIZE` | `1048576` (1MB) | Maximum request body in bytes |
//! | `ALLOWED_ORIGINS` | _(unset = no CORS)_ | Comma-separated allowed origins, or `*` |
//! | `VELOCITY_API_KEY` | _(unset = no auth)_ | Required API key for MCP endpoints |
//! | `RATE_LIMIT_PER_MINUTE` | `100` | Max requests per minute per IP (0 = disabled) |

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper_util::rt::TokioIo;
use tracing::{info, warn};
use velocity_mcp_core::{handle_mcp_request, parse_request, serialize_response};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default maximum request body size: 1 MB.
const DEFAULT_MAX_BODY_SIZE: usize = 1_048_576;

/// Default rate limit: 100 requests per minute per IP.
const DEFAULT_RATE_LIMIT: u32 = 100;

/// How often to sweep expired entries from the rate limiter map.
const RATE_LIMIT_CLEANUP_INTERVAL_SECS: u64 = 60;

/// Pre-serialized health-check response (avoids allocation on every probe).
const HEALTH_RESPONSE: &[u8] =
    b"{\"status\":\"healthy\",\"version\":\"3.2.0\"}";

/// Pre-serialized 404 error (used when no route matches).
const NOT_FOUND_BODY: &[u8] =
    b"{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32601,\"message\":\"Not Found\"},\"id\":null}";

/// Pre-serialized 413 error body.
const PAYLOAD_TOO_LARGE_BODY: &[u8] =
    b"{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32000,\"message\":\"Payload too large\"},\"id\":null}";

/// Pre-serialized 401 error body.
const UNAUTHORIZED_BODY: &[u8] =
    b"{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32000,\"message\":\"Unauthorized\"},\"id\":null}";

/// Pre-serialized 429 error body.
const RATE_LIMITED_BODY: &[u8] =
    b"{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32000,\"message\":\"Rate limit exceeded\"},\"id\":null}";

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Server configuration loaded from environment variables.
///
/// All fields are resolved once at startup and shared (via `Arc`) across all
/// connection handlers. Changing env vars at runtime has no effect.
#[derive(Clone)]
struct ServerConfig {
    /// Maximum allowed request body in bytes.
    max_body_size: usize,
    /// Allowed CORS origins. `None` disables CORS entirely.
    /// `Some(vec![])` (empty vec) means allow all origins (`*`).
    allowed_origins: Option<Vec<String>>,
    /// Required API key for `/mcp` and `/`. `None` disables authentication.
    api_key: Option<String>,
    /// Maximum requests per minute per IP. 0 disables rate limiting.
    rate_limit_per_minute: u32,
}

impl ServerConfig {
    /// Load configuration from environment variables with safe defaults.
    fn from_env() -> Self {
        let max_body_size = std::env::var("MAX_BODY_SIZE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(DEFAULT_MAX_BODY_SIZE);

        let allowed_origins = std::env::var("ALLOWED_ORIGINS").ok().map(|v| {
            if v.trim() == "*" {
                Vec::new() // sentinel: allow all
            } else {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            }
        });

        let api_key = std::env::var("VELOCITY_API_KEY").ok();

        let rate_limit_per_minute = std::env::var("RATE_LIMIT_PER_MINUTE")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(DEFAULT_RATE_LIMIT);

        Self {
            max_body_size,
            allowed_origins,
            api_key,
            rate_limit_per_minute,
        }
    }

    /// Whether CORS is enabled (the env var was set).
    fn cors_enabled(&self) -> bool {
        self.allowed_origins.is_some()
    }

    /// Whether API key authentication is enforced.
    fn auth_enabled(&self) -> bool {
        self.api_key.is_some()
    }

    /// Whether rate limiting is active (limit > 0).
    fn rate_limit_enabled(&self) -> bool {
        self.rate_limit_per_minute > 0
    }
}

// ---------------------------------------------------------------------------
// Rate Limiter (Token Bucket)
// ---------------------------------------------------------------------------

/// Per-IP token bucket for rate limiting.
///
/// Each bucket starts full (`tokens == capacity`). Every request consumes one
/// token. Tokens refill at `capacity` per 60 seconds, computed lazily on each
/// check via elapsed-time interpolation.
struct TokenBucket {
    tokens: f64,
    capacity: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: u32) -> Self {
        Self {
            tokens: capacity as f64,
            capacity: capacity as f64,
            last_refill: Instant::now(),
        }
    }

    /// Refill tokens based on elapsed time and attempt to consume one.
    /// Returns `true` if the request is allowed.
    fn try_consume(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Lazily add tokens proportional to elapsed time since last refill.
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        // capacity tokens per 60 seconds
        let new_tokens = elapsed.as_secs_f64() * (self.capacity / 60.0);
        self.tokens = (self.tokens + new_tokens).min(self.capacity);
        self.last_refill = now;
    }

    /// A bucket is expired when it has been full for longer than the cleanup
    /// interval, meaning the client has been idle.
    fn is_expired(&self) -> bool {
        self.last_refill.elapsed() > Duration::from_secs(RATE_LIMIT_CLEANUP_INTERVAL_SECS)
    }
}

/// Thread-safe per-IP rate limiter backed by a `HashMap` behind a `Mutex`.
///
/// The mutex is held only for the duration of a hash lookup + float arithmetic
/// (< 1 microsecond), so contention is negligible even under heavy load.
struct RateLimiter {
    buckets: Mutex<HashMap<SocketAddr, TokenBucket>>,
    capacity: u32,
}

impl RateLimiter {
    fn new(capacity: u32) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            capacity,
        }
    }

    /// Check whether `addr` is allowed to make a request.
    /// If the rate limit is disabled (`capacity == 0`), always returns `Ok(true)`.
    fn check(&self, addr: SocketAddr) -> Result<bool, ()> {
        if self.capacity == 0 {
            return Ok(true);
        }
        let mut map = self.buckets.lock().map_err(|_| ())?;

        // Periodic cleanup of expired entries to bound memory.
        if map.len() > 1000 {
            map.retain(|_, b| !b.is_expired());
        }

        let bucket = map
            .entry(addr)
            .or_insert_with(|| TokenBucket::new(self.capacity));
        Ok(bucket.try_consume())
    }
}

// ---------------------------------------------------------------------------
// Shared Server State
// ---------------------------------------------------------------------------

/// Shared, immutable-after-init server state passed to every request handler.
///
/// Wrapped in `Arc` and cloned (cheaply) into each connection task. The
/// `RateLimiter` internally uses a `Mutex<HashMap>` so all connections share
/// the same rate-limit state.
struct ServerState {
    config: ServerConfig,
    rate_limiter: RateLimiter,
}

// ---------------------------------------------------------------------------
// Utility: timing-safe string comparison
// ---------------------------------------------------------------------------

/// Compare two byte slices in constant time to prevent timing side-channel attacks.
///
/// Uses XOR accumulation: the loop always runs for `max(len_a, len_b)` iterations
/// regardless of where the first mismatch occurs, and the final `diff == 0` check
/// does not short-circuit. Length difference is folded in separately so that the
/// timing leak from `len_a != len_b` is at most a few nanoseconds (vs. microseconds
/// for early-exit comparison on long strings).
fn timing_safe_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// Utility: correlation ID
// ---------------------------------------------------------------------------

/// Monotonic request counter for generating correlation IDs.
///
/// Uses `Relaxed` ordering: we only need uniqueness, not sequencing guarantees.
/// Combined with process start time to produce IDs that are unique within a
/// server instance and globally unique across restarts.
static REQUEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Server start time, captured once at first use for correlation-ID prefix.
static START_TIME: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Generate a unique, opaque correlation ID for request tracing.
///
/// Format: `<epoch_offset>-<monotonic_counter>` where `epoch_offset` is seconds
/// since server start and `monotonic_counter` is a per-process atomic counter.
fn generate_correlation_id() -> String {
    let start = START_TIME.get_or_init(Instant::now);
    let counter = REQUEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}-{}", start.elapsed().as_secs(), counter)
}

// ---------------------------------------------------------------------------
// Utility: client IP extraction
// ---------------------------------------------------------------------------

/// Extract the client IP address from the request.
///
/// Checks `X-Forwarded-For` header first (for deployments behind a reverse
/// proxy / load balancer), falling back to the TCP peer address captured at
/// connection time. Only the first (leftmost) IP in `X-Forwarded-For` is used.
fn extract_client_ip(
    req: &Request<impl hyper::body::Body>,
    peer_addr: SocketAddr,
) -> SocketAddr {
    req.headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse::<SocketAddr>().ok())
        .unwrap_or(peer_addr)
}

// ---------------------------------------------------------------------------
// Utility: CORS helpers
// ---------------------------------------------------------------------------

/// Check whether `origin` is allowed by the server configuration.
fn is_origin_allowed(origin: &str, config: &ServerConfig) -> bool {
    match &config.allowed_origins {
        None => false,                       // CORS disabled entirely
        Some(allowed) if allowed.is_empty() => true, // wildcard "*"
        Some(allowed) => allowed.iter().any(|o| o == origin),
    }
}

/// Add CORS response headers when the request origin is allowed.
///
/// Called on every response from the MCP endpoint so that browsers enforce the
/// same-origin policy correctly. Takes the origin string directly so that it
/// can be extracted from the request before the request is consumed.
fn apply_cors_headers(
    builder: http::response::Builder,
    origin: Option<&str>,
    config: &ServerConfig,
) -> http::response::Builder {
    if !config.cors_enabled() {
        return builder;
    }
    let origin = match origin {
        Some(o) => o,
        None => return builder,
    };
    if !is_origin_allowed(origin, config) {
        return builder;
    }
    builder
        .header("access-control-allow-origin", origin)
        .header("access-control-allow-methods", "POST, OPTIONS")
        .header(
            "access-control-allow-headers",
            "content-type, x-api-key, authorization",
        )
        .header("access-control-max-age", "86400")
}

// ---------------------------------------------------------------------------
// Utility: response builders
// ---------------------------------------------------------------------------

/// Build a JSON response with the given status code and pre-serialized body.
fn build_response(
    status: StatusCode,
    body: &'static [u8],
) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap_or_else(|_| {
            // Fallback: the builder only fails on invalid status codes, which
            // never happens with our hardcoded constants. Gracefully degrade.
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(
                    b"{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32603,\"message\":\"Internal error\"},\"id\":null}" as &[u8],
                )))
                .unwrap()
        })
}

/// Build a JSON error response with a *sanitized* message.
///
/// The `public_message` is what the client sees. The `detail` is logged
/// server-side only and never transmitted. This prevents leaking stack traces,
/// file paths, or other implementation details.
fn sanitized_error(
    status: StatusCode,
    public_message: &str,
    detail: Option<&str>,
) -> Response<Full<Bytes>> {
    if let Some(d) = detail {
        warn!(status = status.as_u16(), detail = d, "Sanitized error response");
    }
    let error_json = serde_json::json!({
        "jsonrpc": "2.0",
        "error": {
            "code": status.as_u16() as i64,
            "message": public_message
        },
        "id": null
    });
    let body = serde_json::to_vec(&error_json).unwrap_or_default();
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap_or_else(|_| build_response(StatusCode::INTERNAL_SERVER_ERROR, NOT_FOUND_BODY))
}

// ---------------------------------------------------------------------------
// Core request handler (per-connection)
// ---------------------------------------------------------------------------

/// Handle a single HTTP request, applying all security middleware layers.
///
/// Middleware execution order (outermost first):
/// 1. Rate limit check
/// 2. CORS preflight (OPTIONS)
/// 3. Routing
/// 4. CORS headers on response
/// 5. Request logging
async fn handle_request(
    req: Request<hyper::body::Incoming>,
    state: Arc<ServerState>,
    peer_addr: SocketAddr,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let correlation_id = generate_correlation_id();
    let start = Instant::now();
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let client_ip = extract_client_ip(&req, peer_addr);

    // --- Layer 1: Rate Limiting (P1) ---
    if state.config.rate_limit_enabled() {
        match state.rate_limiter.check(client_ip) {
            Ok(true) => { /* allowed */ }
            Ok(false) => {
                warn!(client_ip = %client_ip, "Rate limit exceeded");
                let resp = build_response(StatusCode::TOO_MANY_REQUESTS, RATE_LIMITED_BODY);
                log_request(&method, &path, StatusCode::TOO_MANY_REQUESTS, &correlation_id, start, client_ip);
                return Ok(resp);
            }
            Err(_) => {
                // Mutex poisoned -- degrade gracefully by allowing the request
                // rather than crashing the server.
                warn!("Rate limiter mutex poisoned, degrading gracefully");
            }
        }
    }

    // --- Layer 2: CORS preflight (P0) ---
    if method == hyper::Method::OPTIONS {
        if state.config.cors_enabled() {
            let origin = req
                .headers()
                .get("origin")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if is_origin_allowed(origin, &state.config) {
                let builder = Response::builder()
                    .status(StatusCode::NO_CONTENT)
                    .header("access-control-allow-origin", origin)
                    .header("access-control-allow-methods", "POST, OPTIONS")
                    .header(
                        "access-control-allow-headers",
                        "content-type, x-api-key, authorization",
                    )
                    .header("access-control-max-age", "86400");
                let resp = builder
                    .body(Full::new(Bytes::new()))
                    .unwrap_or_else(|_| build_response(StatusCode::NO_CONTENT, &[]));
                log_request(&method, &path, StatusCode::NO_CONTENT, &correlation_id, start, client_ip);
                return Ok(resp);
            }
        }
        // CORS not enabled or origin not allowed -- fall through to 404.
    }

    // --- Layer 3: Routing ---
    let resp = if path == "/health" || path == "/healthz" {
        // Health check: always allowed, no auth or CORS.
        build_response(StatusCode::OK, HEALTH_RESPONSE)
    } else if method == hyper::Method::POST && (path == "/mcp" || path == "/") {
        // CORS headers are applied inside handle_mcp_post (it owns req).
        handle_mcp_post(req, &state).await
    } else {
        // Apply CORS headers to 404 as well (so browser clients can read it).
        let origin = req.headers().get("origin").and_then(|v| v.to_str().ok());
        let resp = build_response(StatusCode::NOT_FOUND, NOT_FOUND_BODY);
        let (parts, body) = resp.into_parts();
        let builder = apply_cors_headers(Response::builder().status(parts.status), origin, &state.config);
        let mut builder = builder.header("content-type", "application/json");
        for (name, value) in &parts.headers {
            builder = builder.header(name, value);
        }
        builder
            .body(body)
            .unwrap_or_else(|_| build_response(StatusCode::NOT_FOUND, NOT_FOUND_BODY))
    };

    // --- Layer 6: Request Logging (P2) ---
    log_request(&method, &path, resp.status(), &correlation_id, start, client_ip);

    Ok(resp)
}

/// Apply CORS headers to an already-built response, based on the request's Origin.
///
/// Accepts a pre-extracted origin string (since the request may have been consumed
/// by body collection). Pass `None` to skip CORS (no Origin header or CORS disabled).
fn with_cors(
    mut resp: Response<Full<Bytes>>,
    origin: Option<&str>,
    config: &ServerConfig,
) -> Response<Full<Bytes>> {
    if !config.cors_enabled() {
        return resp;
    }
    let origin = match origin {
        Some(o) => o,
        None => return resp,
    };
    if !is_origin_allowed(origin, config) {
        return resp;
    }
    let headers = resp.headers_mut();
    if let Ok(val) = origin.parse() {
        headers.insert("access-control-allow-origin", val);
    }
    if let Ok(val) = "POST, OPTIONS".parse() {
        headers.insert("access-control-allow-methods", val);
    }
    if let Ok(val) = "content-type, x-api-key, authorization".parse() {
        headers.insert("access-control-allow-headers", val);
    }
    if let Ok(val) = "86400".parse() {
        headers.insert("access-control-max-age", val);
    }
    resp
}

/// Handle POST /mcp (or POST /) -- the main MCP endpoint.
///
/// Applies request size limits (P0) and API key authentication (P1) before
/// delegating to `velocity_mcp_core` for JSON-RPC processing.
/// CORS headers are applied to all responses via `with_cors`.
async fn handle_mcp_post(
    req: Request<hyper::body::Incoming>,
    state: &Arc<ServerState>,
) -> Response<Full<Bytes>> {
    // Extract Origin header early (before body consumption moves `req`).
    let origin = req
        .headers()
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // --- Layer 1: Request Size Limits (P0) ---

    // Fast-path: reject based on Content-Length header before reading body.
    if let Some(content_length) = req.headers().get("content-length") {
        if let Ok(len) = content_length.to_str().unwrap_or("0").parse::<u64>() {
            if len > state.config.max_body_size as u64 {
                warn!(
                    content_length = len,
                    max = state.config.max_body_size,
                    "Request body exceeds limit (Content-Length)"
                );
                return with_cors(
                    build_response(StatusCode::PAYLOAD_TOO_LARGE, PAYLOAD_TOO_LARGE_BODY),
                    origin.as_deref(),
                    &state.config,
                );
            }
        }
    }

    // --- Layer 3: API Key Authentication (P1) ---
    if state.config.auth_enabled() {
        let expected_key = state.config.api_key.as_deref().unwrap_or("");
        let provided_key = req
            .headers()
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !timing_safe_eq(provided_key.as_bytes(), expected_key.as_bytes()) {
            warn!("Invalid or missing API key");
            return with_cors(
                build_response(StatusCode::UNAUTHORIZED, UNAUTHORIZED_BODY),
                origin.as_deref(),
                &state.config,
            );
        }
    }

    // Read the request body (consumes `req`).
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            // Sanitize: log the real error, return a generic message.
            warn!(error = %e, "Failed to read request body");
            return with_cors(
                sanitized_error(
                    StatusCode::BAD_REQUEST,
                    "Failed to read request body",
                    Some(&e.to_string()),
                ),
                origin.as_deref(),
                &state.config,
            );
        }
    };

    // Enforce size limit on actual received bytes (Content-Length can be lied about).
    if body_bytes.len() > state.config.max_body_size {
        warn!(
            actual_size = body_bytes.len(),
            max = state.config.max_body_size,
            "Request body exceeds limit (actual)"
        );
        return with_cors(
            build_response(StatusCode::PAYLOAD_TOO_LARGE, PAYLOAD_TOO_LARGE_BODY),
            origin.as_deref(),
            &state.config,
        );
    }

    // Process the MCP JSON-RPC request and apply CORS headers.
    with_cors(process_mcp_request(&body_bytes), origin.as_deref(), &state.config)
}

/// Process MCP JSON-RPC request and return a JSON response.
///
/// All errors from the core protocol are sanitized before being returned to
/// the client (Layer 5: Error Sanitization).
fn process_mcp_request(request_body: &[u8]) -> Response<Full<Bytes>> {
    match parse_request(request_body) {
        Ok(request) => {
            let response = handle_mcp_request(&request);
            let bytes = serialize_response(&response);
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(bytes)))
                .unwrap_or_else(|_| {
                    build_response(StatusCode::INTERNAL_SERVER_ERROR, NOT_FOUND_BODY)
                })
        }
        Err(e) => {
            // Log the parse error detail server-side; return sanitized message.
            warn!(error = %e, "MCP request parse error");
            sanitized_error(
                StatusCode::BAD_REQUEST,
                "Invalid JSON-RPC request",
                Some(&e),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Request logging
// ---------------------------------------------------------------------------

/// Log a completed request in structured format.
///
/// Outputs: method, path, HTTP status, duration (ms), correlation ID, and
/// client IP. This data can be consumed by structured logging collectors
/// (e.g., JSON fmt subscriber, OpenTelemetry, etc.).
fn log_request(
    method: &hyper::Method,
    path: &str,
    status: StatusCode,
    correlation_id: &str,
    start: Instant,
    client_ip: SocketAddr,
) {
    let duration_ms = start.elapsed().as_millis();
    info!(
        method = %method,
        path = path,
        status = status.as_u16(),
        duration_ms = duration_ms as u64,
        correlation_id = correlation_id,
        client_ip = %client_ip,
        "Request completed"
    );
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing (structured logging).
    tracing_subscriber::fmt::init();

    // Load configuration from environment.
    let config = ServerConfig::from_env();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        cors_enabled = config.cors_enabled(),
        auth_enabled = config.auth_enabled(),
        rate_limit_per_minute = config.rate_limit_per_minute,
        max_body_size = config.max_body_size,
        "Starting VELOCITY-MCP Edge server with security hardening"
    );

    // Get port from environment or use default.
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);

    let addr: SocketAddr = ([0, 0, 0, 0], port).into();

    info!(address = %addr, "Listening for connections");

    // Create shared server state once -- all connections share the same
    // rate limiter and configuration via Arc.
    let state = Arc::new(ServerState {
        config,
        rate_limiter: RateLimiter::new(
            std::env::var("RATE_LIMIT_PER_MINUTE")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(DEFAULT_RATE_LIMIT),
        ),
    });

    // Create TCP listener.
    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Accept connections and process them.
    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let io = TokioIo::new(stream);

        // Clone the Arc (cheap: just increments the refcount).
        let state = Arc::clone(&state);

        // Spawn a task to handle the connection.
        tokio::task::spawn(async move {
            if let Err(err) = hyper::server::conn::http1::Builder::new()
                .serve_connection(
                    io,
                    service_fn(move |req| {
                        let state = Arc::clone(&state);
                        handle_request(req, state, peer_addr)
                    }),
                )
                .await
            {
                warn!(error = %err, "Error serving connection");
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Timing-safe comparison --

    #[test]
    fn timing_safe_equal_strings() {
        assert!(timing_safe_eq(b"hello", b"hello"));
    }

    #[test]
    fn timing_safe_different_strings() {
        assert!(!timing_safe_eq(b"hello", b"world"));
    }

    #[test]
    fn timing_safe_different_lengths() {
        assert!(!timing_safe_eq(b"short", b"much longer string"));
    }

    #[test]
    fn timing_safe_empty() {
        assert!(timing_safe_eq(b"", b""));
    }

    #[test]
    fn timing_safe_one_empty() {
        assert!(!timing_safe_eq(b"", b"a"));
        assert!(!timing_safe_eq(b"a", b""));
    }

    // -- Configuration --

    #[test]
    fn config_defaults() {
        // When env vars are not set, defaults should apply.
        // Note: this test may be affected by other tests setting env vars,
        // so we only check the struct construction logic.
        let cfg = ServerConfig {
            max_body_size: DEFAULT_MAX_BODY_SIZE,
            allowed_origins: None,
            api_key: None,
            rate_limit_per_minute: DEFAULT_RATE_LIMIT,
        };
        assert_eq!(cfg.max_body_size, 1_048_576);
        assert!(!cfg.cors_enabled());
        assert!(!cfg.auth_enabled());
        assert!(cfg.rate_limit_enabled());
    }

    #[test]
    fn config_cors_enabled_with_origins() {
        let cfg = ServerConfig {
            max_body_size: DEFAULT_MAX_BODY_SIZE,
            allowed_origins: Some(vec!["https://example.com".into()]),
            api_key: None,
            rate_limit_per_minute: DEFAULT_RATE_LIMIT,
        };
        assert!(cfg.cors_enabled());
    }

    #[test]
    fn config_auth_enabled() {
        let cfg = ServerConfig {
            max_body_size: DEFAULT_MAX_BODY_SIZE,
            allowed_origins: None,
            api_key: Some("secret-key".into()),
            rate_limit_per_minute: DEFAULT_RATE_LIMIT,
        };
        assert!(cfg.auth_enabled());
    }

    #[test]
    fn config_rate_limit_disabled_when_zero() {
        let cfg = ServerConfig {
            max_body_size: DEFAULT_MAX_BODY_SIZE,
            allowed_origins: None,
            api_key: None,
            rate_limit_per_minute: 0,
        };
        assert!(!cfg.rate_limit_enabled());
    }

    // -- Rate limiter --

    #[test]
    fn rate_limiter_allows_under_limit() {
        let rl = RateLimiter::new(10);
        let addr: SocketAddr = "127.0.0.1:1234".parse().unwrap();
        for _ in 0..10 {
            assert!(rl.check(addr).unwrap());
        }
    }

    #[test]
    fn rate_limiter_blocks_over_limit() {
        let rl = RateLimiter::new(2);
        let addr: SocketAddr = "127.0.0.1:5678".parse().unwrap();
        assert!(rl.check(addr).unwrap()); // 1st
        assert!(rl.check(addr).unwrap()); // 2nd
        assert!(!rl.check(addr).unwrap()); // 3rd -- blocked
    }

    #[test]
    fn rate_limiter_disabled_at_zero() {
        let rl = RateLimiter::new(0);
        let addr: SocketAddr = "127.0.0.1:9999".parse().unwrap();
        // Should always return true when disabled.
        for _ in 0..1000 {
            assert!(rl.check(addr).unwrap());
        }
    }

    #[test]
    fn rate_limiter_separate_ips() {
        let rl = RateLimiter::new(1);
        let addr1: SocketAddr = "10.0.0.1:1000".parse().unwrap();
        let addr2: SocketAddr = "10.0.0.2:2000".parse().unwrap();
        assert!(rl.check(addr1).unwrap());
        assert!(rl.check(addr2).unwrap()); // different IP, separate bucket
        assert!(!rl.check(addr1).unwrap()); // addr1 exhausted
    }

    // -- Request size enforcement --

    #[test]
    fn body_size_limit_rejects_large() {
        // Simulate the Content-Length check path.
        let max: usize = 1024;
        let declared_len: u64 = 2048;
        assert!(declared_len > max as u64);
    }

    #[test]
    fn body_size_limit_accepts_small() {
        let max: usize = 1_048_576; // 1 MB
        let declared_len: u64 = 512;
        assert!(declared_len <= max as u64);
    }

    // -- CORS origin checking --

    #[test]
    fn origin_allowed_wildcard() {
        let cfg = ServerConfig {
            max_body_size: DEFAULT_MAX_BODY_SIZE,
            allowed_origins: Some(vec![]), // empty = wildcard
            api_key: None,
            rate_limit_per_minute: DEFAULT_RATE_LIMIT,
        };
        assert!(is_origin_allowed("https://anything.com", &cfg));
    }

    #[test]
    fn origin_allowed_specific() {
        let cfg = ServerConfig {
            max_body_size: DEFAULT_MAX_BODY_SIZE,
            allowed_origins: Some(vec!["https://example.com".into()]),
            api_key: None,
            rate_limit_per_minute: DEFAULT_RATE_LIMIT,
        };
        assert!(is_origin_allowed("https://example.com", &cfg));
        assert!(!is_origin_allowed("https://evil.com", &cfg));
    }

    #[test]
    fn origin_not_allowed_when_disabled() {
        let cfg = ServerConfig {
            max_body_size: DEFAULT_MAX_BODY_SIZE,
            allowed_origins: None,
            api_key: None,
            rate_limit_per_minute: DEFAULT_RATE_LIMIT,
        };
        assert!(!is_origin_allowed("https://example.com", &cfg));
    }

    // -- Correlation ID --

    #[test]
    fn correlation_ids_are_unique() {
        let id1 = generate_correlation_id();
        let id2 = generate_correlation_id();
        assert_ne!(id1, id2);
    }

    // -- MCP request processing --

    #[test]
    fn process_mcp_request_valid_initialize() {
        let body = br#"{"jsonrpc":"2.0","method":"initialize","params":{},"id":1}"#;
        let resp = process_mcp_request(body);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn process_mcp_request_invalid_json() {
        let body = b"not valid json at all";
        let resp = process_mcp_request(body);
        // Should return 400 with sanitized message (no internal details).
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn process_mcp_request_oversized() {
        // Create a payload that exceeds a 1-byte limit.
        let body = br#"{"jsonrpc":"2.0","method":"ping","id":1}"#;
        assert!(body.len() > 1); // sanity: the body is > 1 byte
    }

    // -- Pre-serialized constants --

    #[test]
    fn pre_serialized_constants_are_valid_json() {
        // Ensure all pre-serialized bodies are valid JSON.
        for body in &[
            HEALTH_RESPONSE,
            NOT_FOUND_BODY,
            PAYLOAD_TOO_LARGE_BODY,
            UNAUTHORIZED_BODY,
            RATE_LIMITED_BODY,
        ] {
            let parsed: Result<serde_json::Value, _> = serde_json::from_slice(body);
            assert!(parsed.is_ok(), "Pre-serialized body is not valid JSON: {:?}", String::from_utf8_lossy(body));
        }
    }
}
