//! Token bucket rate limiter for MCP tool calls.
//!
//! Two kinds of bucket live here:
//!
//! - [`check_rate_limit`] draws from one process-wide bucket. That is the right
//!   shape for the stdio and shared-memory transports, which each serve exactly
//!   one peer for the life of the process.
//! - [`check_rate_limit_for_peer`] gives every HTTP peer its own bucket, so one
//!   client exhausting its allowance cannot throttle the others.
//!
//! Tokens accumulate at a fixed rate up to a maximum burst size; each call
//! consumes one token.

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

        // Calculate new tokens to add (saturating to prevent overflow)
        let new_tokens_scaled = elapsed_ms.saturating_mul(self.rate as u64);

        // Try to update the refill timestamp (only one thread wins)
        if self
            .last_refill_ms
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            // We won the refill race; add tokens
            loop {
                let current = self.tokens_scaled.load(Ordering::Relaxed);
                let new_val = current
                    .saturating_add(new_tokens_scaled)
                    .min(self.burst_scaled);
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

/// Resolve the configured (rate per second, burst) pair.
///
/// Limits can be raised via VELOCITY_RATE_LIMIT / VELOCITY_RATE_BURST env vars
/// (e.g. for benchmark runs); defaults are 20 tokens/sec, burst 100.
fn configured_limits() -> (u32, u32) {
    match (
        std::env::var("VELOCITY_RATE_LIMIT")
            .ok()
            .and_then(|v| v.parse::<u32>().ok()),
        std::env::var("VELOCITY_RATE_BURST")
            .ok()
            .and_then(|v| v.parse::<u32>().ok()),
    ) {
        (Some(rate), Some(burst)) => (rate, burst),
        (Some(rate), None) => (rate, DEFAULT_BURST),
        (None, Some(burst)) => (DEFAULT_RATE, burst),
        (None, None) => (DEFAULT_RATE, DEFAULT_BURST),
    }
}

/// Global rate limiter instance, shared by the single-peer transports.
static GLOBAL_RATE_LIMITER: std::sync::LazyLock<RateLimiter> = std::sync::LazyLock::new(|| {
    let (rate, burst) = configured_limits();
    RateLimiter::with_limits(rate, burst)
});

/// Check if a call is allowed by the process-wide limiter.
pub fn check_rate_limit() -> bool {
    GLOBAL_RATE_LIMITER.try_acquire()
}

/// Get the current available tokens from the global limiter.
pub fn available_tokens() -> u32 {
    GLOBAL_RATE_LIMITER.available_tokens()
}

/// Refill the process-wide bucket to capacity. Test-only by design: production
/// has no reset because one bucket per process is the intended semantics for the
/// single-peer transports. A unit test that drains the shared bucket to prove the
/// refusal path otherwise leaves every later `tools/call` in the same binary
/// racing the 20 tokens/sec refill.
#[cfg(test)]
pub(crate) fn refill_global_bucket() {
    GLOBAL_RATE_LIMITER
        .last_refill_ms
        .store(current_time_ms(), Ordering::Relaxed);
    GLOBAL_RATE_LIMITER
        .tokens_scaled
        .store(GLOBAL_RATE_LIMITER.burst_scaled, Ordering::Relaxed);
}

/// Distinct peer buckets kept alive before the least recently used one is evicted.
#[cfg(feature = "http")]
const MAX_PEER_BUCKETS: usize = 4096;

#[cfg(feature = "http")]
static PEER_LIMITERS: std::sync::LazyLock<std::sync::Mutex<lru::LruCache<String, RateLimiter>>> =
    std::sync::LazyLock::new(|| {
        let max = std::num::NonZeroUsize::new(MAX_PEER_BUCKETS)
            .expect("MAX_PEER_BUCKETS must be non-zero");
        std::sync::Mutex::new(lru::LruCache::new(max))
    });

/// Try to consume one token from `peer`'s own bucket.
///
/// Each peer gets the same rate and burst the global limiter uses, so a client
/// that floods the server only exhausts its own allowance.
#[cfg(feature = "http")]
pub fn check_rate_limit_for_peer(peer: &str) -> bool {
    let (rate, burst) = configured_limits();
    let mut cache = PEER_LIMITERS.lock().unwrap_or_else(|e| e.into_inner());
    let limiter =
        cache.get_or_insert_mut(peer.to_string(), || RateLimiter::with_limits(rate, burst));
    limiter.try_acquire()
}

/// Number of peer buckets currently tracked (diagnostics).
#[cfg(feature = "http")]
pub fn tracked_peers() -> usize {
    PEER_LIMITERS
        .lock()
        .map(|c| c.len())
        .unwrap_or_else(|e| e.into_inner().len())
}

/// Tokens left in one peer's own bucket, or `None` if that peer has no bucket yet.
///
/// `tracked_peers()` is a process-wide total, so it moves whenever any other thread
/// touches a bucket; this reads one key, which makes it usable as a per-peer
/// diagnostic and as the subject of a race-free assertion. `peek` is deliberate:
/// reading must not reorder the LRU the eviction policy depends on.
#[cfg(feature = "http")]
pub fn peer_available_tokens(peer: &str) -> Option<u32> {
    PEER_LIMITERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .peek(peer)
        .map(|limiter| limiter.available_tokens())
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
        // The clock is millisecond-resolution and refill adds `elapsed_ms * rate`, so at
        // 1000/s one ms is exactly one token and the drain below re-admits on any tick
        // landing inside it; 100/s keeps a ms at a tenth of a token, 50 ms at ~5 tokens.
        let limiter = RateLimiter::with_limits(100, 10);
        for _ in 0..10 {
            assert!(limiter.try_acquire());
        }
        assert!(!limiter.try_acquire());
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
        // A call that is refused must have found an empty bucket; anything else is a
        // limiter that rejects while tokens are available. Read the count first so a
        // concurrent test draining the shared bucket cannot make this racy.
        let before = available_tokens();
        let allowed = check_rate_limit();
        assert!(
            allowed || before == 0,
            "refused with {before} tokens available"
        );
    }

    #[cfg(feature = "http")]
    #[test]
    fn test_peer_buckets_are_independent() {
        // Unique keys keep this test from coupling to any other test's peer buckets.
        let noisy = "203.0.113.7:peer-independence";
        let quiet = "203.0.113.8:peer-independence";

        assert!(check_rate_limit_for_peer(quiet));

        // Drain the noisy peer's bucket however large the configured burst is.
        let mut drained = false;
        for _ in 0..10_000 {
            if !check_rate_limit_for_peer(noisy) {
                drained = true;
                break;
            }
        }
        assert!(drained, "peer bucket never ran out of tokens");
        assert!(!check_rate_limit_for_peer(noisy));

        assert!(
            check_rate_limit_for_peer(quiet),
            "exhausting one peer's bucket throttled a different peer"
        );
    }

    #[cfg(feature = "http")]
    #[test]
    fn test_peer_buckets_are_tracked() {
        // The key is unique to this test, so nothing else can create or drain this
        // bucket. Every claim below is therefore about state this test owns. The
        // global total is asserted only in the direction a concurrent test cannot
        // break: other peers can add buckets, they cannot remove ours.
        let peer = "198.51.100.9:peer-tracking";
        let (_, burst) = configured_limits();
        assert_eq!(peer_available_tokens(peer), None, "bucket exists pre-use");

        let before = tracked_peers();
        assert!(check_rate_limit_for_peer(peer));
        assert!(tracked_peers() > before || before >= MAX_PEER_BUCKETS);
        assert!(
            peer_available_tokens(peer).is_some(),
            "no bucket after first call"
        );

        // Reuse is measured as persistence, not as a total. Once the bucket has
        // refused a call it must stay near-empty across later calls; a limiter
        // rebuilt per call would hand back burst-1 tokens every time, so any
        // reading under half the burst proves one object kept answering for this
        // peer. The headroom absorbs refill during the drain loop.
        let mut refused = false;
        for _ in 0..(burst + 8) {
            if !check_rate_limit_for_peer(peer) {
                refused = true;
                break;
            }
        }
        assert!(refused, "peer bucket never drained within {burst}+8 calls");
        let left = peer_available_tokens(peer).expect("peer untracked after draining");
        assert!(
            left * 2 < burst.max(1),
            "later calls reset the bucket instead of reusing it: {left} of {burst} left"
        );
    }

    #[cfg(feature = "http")]
    #[test]
    fn test_peer_bucket_respects_configured_burst() {
        let peer = "192.0.2.11:burst-shape";
        let (_, burst) = configured_limits();
        // Every fresh bucket starts full, so at least `burst` calls must succeed.
        for i in 0..burst {
            assert!(
                check_rate_limit_for_peer(peer),
                "refused call {i} of {burst}"
            );
        }
    }
}
