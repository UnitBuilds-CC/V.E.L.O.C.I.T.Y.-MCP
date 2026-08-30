//! Token bucket rate limiter for MCP tool calls.
//!
//! Prevents a single client from overwhelming the server with rapid-fire tool
//! calls. Uses a token bucket algorithm: tokens accumulate at a fixed rate up
//! to a maximum burst size; each tool call consumes one token.

use std::sync::atomic::{AtomicU64, Ordering};

/// Maximum tokens that can accumulate (burst capacity).
const DEFAULT_BURST: u32 = 100;

/// Tokens added per second.
const DEFAULT_RATE: u32 = 20;

/// Token bucket rate limiter.
pub struct RateLimiter {
    /// Current token count (scaled by 1000 for sub-token precision).
    tokens_scaled: AtomicU64,
    /// Last refill timestamp (milliseconds since arbitrary epoch).
    last_refill_ms: AtomicU64,
    /// Burst capacity (scaled by 1000).
    burst_scaled: u64,
    /// Refill rate in tokens per second.
    rate: u32,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    /// Create a new rate limiter with default settings (20 tokens/sec, burst 100).
    pub fn new() -> Self {
        Self::with_limits(DEFAULT_RATE, DEFAULT_BURST)
    }

    /// Create a rate limiter with custom limits.
    pub fn with_limits(rate_per_sec: u32, burst: u32) -> Self {
        let burst_scaled = burst as u64 * 1000;
        RateLimiter {
            tokens_scaled: AtomicU64::new(burst_scaled),
            last_refill_ms: AtomicU64::new(current_time_ms()),
            burst_scaled,
            rate: rate_per_sec,
        }
    }

    /// Try to consume one token. Returns `true` if allowed, `false` if rate-limited.
    pub fn try_acquire(&self) -> bool {
        self.refill();

        // Atomic CAS loop to consume one token
        loop {
            let current = self.tokens_scaled.load(Ordering::Relaxed);
            if current < 1000 {
                return false; // Not enough tokens
            }
            let new_val = current - 1000;
            match self.tokens_scaled.compare_exchange_weak(
                current,
                new_val,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(_) => continue, // Retry
            }
        }
    }

    /// Refill tokens based on elapsed time.
    fn refill(&self) {
        let now = current_time_ms();
        let last = self.last_refill_ms.load(Ordering::Relaxed);

        if now <= last {
            return;
        }

        let elapsed_ms = now - last;

        // Calculate new tokens to add
        let new_tokens_scaled = elapsed_ms * (self.rate as u64); // tokens * 1000 / 1000ms

        // Try to update the refill timestamp (only one thread wins)
        if self
            .last_refill_ms
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            // We won the refill race; add tokens
            loop {
                let current = self.tokens_scaled.load(Ordering::Relaxed);
                let new_val = (current + new_tokens_scaled).min(self.burst_scaled);
                match self.tokens_scaled.compare_exchange_weak(
                    current,
                    new_val,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(_) => continue,
                }
            }
        }
    }

    /// Get the current approximate token count (for diagnostics).
    pub fn available_tokens(&self) -> u32 {
        self.refill();
        (self.tokens_scaled.load(Ordering::Relaxed) / 1000) as u32
    }
}

/// Get current time in milliseconds (monotonic, relative to process start).
fn current_time_ms() -> u64 {
    static EPOCH: std::sync::LazyLock<std::time::Instant> =
        std::sync::LazyLock::new(std::time::Instant::now);
    EPOCH.elapsed().as_millis() as u64
}

/// Global rate limiter instance.
static GLOBAL_RATE_LIMITER: std::sync::LazyLock<RateLimiter> =
    std::sync::LazyLock::new(RateLimiter::default);

/// Check if a tool call is allowed by the global rate limiter.
pub fn check_rate_limit() -> bool {
    GLOBAL_RATE_LIMITER.try_acquire()
}

/// Get the current available tokens from the global limiter.
pub fn available_tokens() -> u32 {
    GLOBAL_RATE_LIMITER.available_tokens()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_rate_limiter_allows_burst() {
        let limiter = RateLimiter::with_limits(10, 5);
        // Should allow 5 calls in a burst
        for _ in 0..5 {
            assert!(limiter.try_acquire());
        }
        // 6th should be rejected
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn test_rate_limiter_refills_over_time() {
        let limiter = RateLimiter::with_limits(1000, 10);
        // Consume all tokens
        for _ in 0..10 {
            assert!(limiter.try_acquire());
        }
        assert!(!limiter.try_acquire());

        // Wait a bit for refill (at 1000 tokens/sec, 50ms should give ~50 tokens)
        std::thread::sleep(Duration::from_millis(50));
        assert!(limiter.try_acquire());
    }

    #[test]
    fn test_rate_limiter_available_tokens() {
        let limiter = RateLimiter::with_limits(10, 10);
        let initial = limiter.available_tokens();
        assert!(initial <= 10);
        assert!(initial > 0);
    }

    #[test]
    fn test_global_rate_limiter_works() {
        // Just verify it doesn't panic
        let _ = check_rate_limit();
        let _ = available_tokens();
    }
}
