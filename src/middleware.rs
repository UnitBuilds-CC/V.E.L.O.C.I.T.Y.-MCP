//! Advanced middleware and features for HTTP transport.
//!
//! Provides middleware for:
//! - Request/response transformation
//! - Response caching
//! - Request batching
//! - Request logging
//! - Rate limiting (enhanced)
//! - Request validation

use axum::{
    body::Body,
    extract::Request,
    http::{StatusCode, HeaderMap},
    middleware::Next,
    response::{Response, IntoResponse},
};
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Cache entry with expiration.
#[derive(Clone)]
struct CacheEntry {
    response: Vec<u8>,
    status: StatusCode,
    headers: HeaderMap,
    expires_at: Instant,
}

/// Response cache with TTL support and LRU eviction.
#[derive(Clone)]
pub struct ResponseCache {
    entries: Arc<RwLock<LruCache<String, CacheEntry>>>,
    default_ttl: Duration,
}

impl ResponseCache {
    /// Create a new response cache with default TTL and max size.
    pub fn new(default_ttl: Duration) -> Self {
        Self::with_max_size(default_ttl, 10000)
    }
    
    /// Create a new response cache with custom max size.
    pub fn with_max_size(default_ttl: Duration, max_size: usize) -> Self {
        let max_size = NonZeroUsize::new(max_size).unwrap_or(NonZeroUsize::new(10000).unwrap());
        Self {
            entries: Arc::new(RwLock::new(LruCache::new(max_size))),
            default_ttl,
        }
    }
    
    /// Get a cached response by key.
    pub fn get(&self, key: &str) -> Option<Response> {
        let mut entries = self.entries.write().ok()?;
        let entry = entries.get(key)?;
        
        if entry.expires_at > Instant::now() {
            let mut response = Response::builder()
                .status(entry.status)
                .body(Body::from(entry.response.clone()))
                .ok()?;
            
            *response.headers_mut() = entry.headers.clone();
            response.headers_mut().insert("X-Cache", axum::http::HeaderValue::from_static("HIT"));
            
            Some(response)
        } else {
            // Entry expired, remove it
            entries.pop(key);
            None
        }
    }
    
    /// Store a response in the cache. Returns the response (cloned body) so the caller can still use it.
    pub async fn set(&self, key: String, response: Response, ttl: Option<Duration>) -> Response {
        let ttl = ttl.unwrap_or(self.default_ttl);
        let expires_at = Instant::now() + ttl;
        
        let (parts, body) = response.into_parts();
        let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %e, "Cache: response body exceeded 10MB limit, skipping cache");
                return Response::from_parts(parts, Body::empty());
            }
        };
        
        let entry = CacheEntry {
            response: body_bytes.to_vec(),
            status: parts.status,
            headers: parts.headers.clone(),
            expires_at,
        };
        
        if let Ok(mut entries) = self.entries.write() {
            entries.put(key, entry);
        }
        
        Response::from_parts(parts, Body::from(body_bytes))
    }
    
    /// Clear expired entries from the cache.
    pub fn cleanup(&self) {
        if let Ok(mut entries) = self.entries.write() {
            let now = Instant::now();
            let keys_to_remove: Vec<String> = entries
                .iter()
                .filter(|(_, entry)| entry.expires_at <= now)
                .map(|(key, _)| key.clone())
                .collect();
            
            for key in keys_to_remove {
                entries.pop(&key);
            }
        }
    }
    
    /// Clear all entries from the cache.
    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.write() {
            entries.clear();
        }
    }
}

/// Request logger middleware.
pub async fn request_logger_middleware(
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let start = Instant::now();
    
    let response = next.run(request).await;
    
    let duration = start.elapsed();
    let status = response.status();
    
    tracing::info!(
        method = %method,
        uri = %uri,
        status = %status,
        duration_ms = duration.as_millis(),
        "Request completed"
    );
    
    response
}

/// Request validator middleware.
pub async fn request_validator_middleware(
    request: Request,
    next: Next,
) -> Response {
    // Validate content-type for POST requests
    if request.method() == axum::http::Method::POST {
        if let Some(content_type) = request.headers().get("content-type") {
            let ct = content_type.to_str().unwrap_or("");
            if !ct.contains("application/json") && !ct.contains("application/octet-stream") {
                return (
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    "Content-Type must be application/json or application/octet-stream"
                ).into_response();
            }
        }
    }
    
    next.run(request).await
}

/// Batch request handler.
#[derive(serde::Deserialize)]
pub struct BatchRequest {
    pub requests: Vec<serde_json::Value>,
}

/// Process a batch of requests.
pub async fn handle_batch_request(
    axum::Json(batch): axum::Json<BatchRequest>,
) -> Response {
    let mut responses = Vec::new();
    
    for request in batch.requests {
        let result = crate::protocol::json_rpc::handle_request(&request);
        match result {
            Some(response) => responses.push(response),
            None => responses.push(serde_json::json!({"status": "notification processed"})),
        }
    }
    
    axum::Json(responses).into_response()
}

/// Cache middleware factory.
pub fn cache_middleware(cache: ResponseCache) -> impl Fn(Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> + Clone {
    move |request: Request, next: Next| {
        let cache = cache.clone();
        
        Box::pin(async move {
            let cache_key = format!("{}:{}", request.method(), request.uri());
            
            if let Some(cached_response) = cache.get(&cache_key) {
                return cached_response;
            }
            
            let response = next.run(request).await;
            
            if response.status().is_success() {
                cache.set(cache_key, response, None).await
            } else {
                response
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cache_creation() {
        let cache = ResponseCache::new(Duration::from_secs(60));
        assert_eq!(cache.default_ttl, Duration::from_secs(60));
    }
    
    #[test]
    fn test_cache_clear() {
        let cache = ResponseCache::new(Duration::from_secs(60));
        cache.clear();
        // Should not panic
    }
    
    #[test]
    fn test_cache_cleanup() {
        let cache = ResponseCache::new(Duration::from_secs(60));
        cache.cleanup();
        // Should not panic
    }
}
