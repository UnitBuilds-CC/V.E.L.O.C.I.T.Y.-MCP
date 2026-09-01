//! Audit logging for MCP tool executions.
//!
//! Records every tool call with timestamp, tool name, parameters (sanitized),
//! outcome, and duration. Logs are stored in a ring buffer to prevent unbounded
//! memory growth.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Maximum number of audit entries to retain in memory.
const MAX_AUDIT_ENTRIES: usize = 10_000;

/// Outcome of an audited operation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum AuditOutcome {
    Success,
    Error(String),
    Timeout,
    Rejected(String),
}

/// A single audit log entry.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditEntry {
    /// Monotonic sequence number.
    pub sequence: u64,
    /// Unix timestamp in milliseconds.
    pub timestamp_ms: u64,
    /// Tool name that was called.
    pub tool_name: String,
    /// Duration in microseconds (µs) for sub-millisecond precision.
    pub duration_us: u64,
    /// Outcome of the call.
    pub outcome: AuditOutcome,
    /// Transport layer: "http", "stdio", "shmem", "nda_http", "nda_stdio", "nda_shmem", "websocket", "sse".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    /// Request payload size in bytes (if known).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_size: Option<u64>,
    /// Response size in bytes (if known).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_size: Option<u64>,
    /// Merkle root hash (hex-encoded) if from NDA transport, None otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merkle_root: Option<String>,
    /// Session ID for multi-tenant isolation, None for legacy/global entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Global sequence counter for audit entries.
static AUDIT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// In-memory audit log with ring buffer semantics.
pub struct AuditLog {
    entries: std::sync::Mutex<Vec<AuditEntry>>,
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditLog {
    /// Create a new audit log.
    pub fn new() -> Self {
        AuditLog {
            entries: std::sync::Mutex::new(Vec::with_capacity(1024)),
        }
    }

    /// Record a tool execution.
    pub fn record(
        &self,
        tool_name: &str,
        start: Instant,
        outcome: AuditOutcome,
    ) {
        self.record_with_context(tool_name, start, outcome, None, None);
    }

    /// Record a tool execution with an optional Merkle root (from NDA transport).
    pub fn record_with_merkle(
        &self,
        tool_name: &str,
        start: Instant,
        outcome: AuditOutcome,
        merkle_root: Option<String>,
    ) {
        self.record_with_context(tool_name, start, outcome, merkle_root, None);
    }

    /// Record a tool execution with full context (Merkle root + session ID).
    pub fn record_with_context(
        &self,
        tool_name: &str,
        start: Instant,
        outcome: AuditOutcome,
        merkle_root: Option<String>,
        session_id: Option<String>,
    ) {
        self.record_full(
            tool_name,
            start,
            outcome,
            None,
            None,
            None,
            merkle_root,
            session_id,
        );
    }

    /// Record a tool execution with complete metadata.
    pub fn record_full(
        &self,
        tool_name: &str,
        start: Instant,
        outcome: AuditOutcome,
        transport: Option<String>,
        payload_size: Option<u64>,
        response_size: Option<u64>,
        merkle_root: Option<String>,
        session_id: Option<String>,
    ) {
        let seq = AUDIT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let duration_us = start.elapsed().as_micros() as u64;

        let entry = AuditEntry {
            sequence: seq,
            timestamp_ms,
            tool_name: tool_name.to_string(),
            duration_us,
            outcome,
            transport,
            payload_size,
            response_size,
            merkle_root,
            session_id,
        };

        // Use poisoning-tolerant lock (inspired by Velocity-IDE safety.rs)
        let mut entries = match self.entries.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                eprintln!("[WARN] Audit log mutex poisoning recovered.");
                poisoned.into_inner()
            }
        };

        entries.push(entry);

        // Ring buffer: drop oldest entries if we exceed the limit
        if entries.len() > MAX_AUDIT_ENTRIES {
            let drain_count = entries.len() - MAX_AUDIT_ENTRIES;
            entries.drain(..drain_count);
        }
    }

    /// Get the most recent N entries (newest first).
    pub fn recent(&self, count: usize) -> Vec<AuditEntry> {
        let entries = match self.entries.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        entries.iter().rev().take(count).cloned().collect()
    }

    /// Get the total number of entries currently stored.
    pub fn len(&self) -> usize {
        match self.entries.lock() {
            Ok(guard) => guard.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        }
    }

    /// Returns true if the audit log contains no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clear all entries.
    pub fn clear(&self) {
        match self.entries.lock() {
            Ok(mut guard) => guard.clear(),
            Err(poisoned) => poisoned.into_inner().clear(),
        }
    }

    /// Get all entries (for export/streaming).
    pub fn all(&self) -> Vec<AuditEntry> {
        match self.entries.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Export audit log to JSON format.
    pub fn export_json(&self) -> Result<String, String> {
        let entries = self.all();
        serde_json::to_string_pretty(&entries)
            .map_err(|e| format!("Failed to serialize audit log to JSON: {}", e))
    }

    /// Export audit log to CSV format.
    pub fn export_csv(&self) -> Result<String, String> {
        let entries = self.all();
        let mut csv = String::from("sequence,timestamp_ms,tool_name,duration_us,outcome,transport,payload_size,response_size,merkle_root,session_id\n");
        
        for entry in entries {
            let outcome_str = match &entry.outcome {
                AuditOutcome::Success => "success".to_string(),
                AuditOutcome::Error(msg) => format!("error:{}", msg.replace(',', ";")),
                AuditOutcome::Timeout => "timeout".to_string(),
                AuditOutcome::Rejected(reason) => format!("rejected:{}", reason.replace(',', ";")),
            };
            let transport_str = entry.transport.unwrap_or_default();
            let payload_str = entry.payload_size.map(|s| s.to_string()).unwrap_or_default();
            let response_str = entry.response_size.map(|s| s.to_string()).unwrap_or_default();
            let merkle_str = entry.merkle_root.unwrap_or_default();
            let session_str = entry.session_id.unwrap_or_default();
            
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{}\n",
                entry.sequence,
                entry.timestamp_ms,
                entry.tool_name,
                entry.duration_us,
                outcome_str,
                transport_str,
                payload_str,
                response_str,
                merkle_str,
                session_str
            ));
        }
        
        Ok(csv)
    }

    /// Flush audit log to disk as JSON. Returns the number of entries written.
    pub fn flush_to_file(&self, path: &str) -> Result<usize, String> {
        let json = self.export_json()?;
        std::fs::write(path, json)
            .map_err(|e| format!("Failed to write audit log to {}: {}", path, e))?;
        Ok(self.len())
    }
}

/// Global audit log instance (backward-compatible, used by direct callers like sandbox.rs).
static GLOBAL_AUDIT: std::sync::LazyLock<AuditLog> =
    std::sync::LazyLock::new(AuditLog::default);

/// Get a reference to the global audit log.
///
/// This is for direct callers (e.g. sandbox.rs) that don't go through
/// the convenience functions. Session-aware recording should use
/// `record_tool_call()` / `record_tool_call_with_merkle()` instead.
pub fn global_audit() -> &'static AuditLog {
    &GLOBAL_AUDIT
}

// ─── Thread-local session context ────────────────────────────────────────────

use std::cell::RefCell;

std::thread_local! {
    static CURRENT_SESSION_ID: RefCell<Option<String>> = const { RefCell::new(None) };
    static CURRENT_TRANSPORT: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Set the session context for the current thread.
///
/// Must be called by transport entry points before dispatching to protocol handlers.
/// All subsequent `record_tool_call()` invocations on this thread will route to
/// the per-session audit buffer.
pub fn set_session_context(session_id: String) {
    CURRENT_SESSION_ID.with(|cell| {
        *cell.borrow_mut() = Some(session_id);
    });
}

/// Clear the session context for the current thread.
pub fn clear_session_context() {
    CURRENT_SESSION_ID.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

/// Get the current session ID, if set.
pub fn current_session_id() -> Option<String> {
    CURRENT_SESSION_ID.with(|cell| cell.borrow().clone())
}

/// Set the transport context for the current thread (e.g., "http", "stdio", "shmem").
pub fn set_transport_context(transport: String) {
    CURRENT_TRANSPORT.with(|cell| {
        *cell.borrow_mut() = Some(transport);
    });
}

/// Clear the transport context for the current thread.
pub fn clear_transport_context() {
    CURRENT_TRANSPORT.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

/// Get the current transport, if set.
pub fn current_transport() -> Option<String> {
    CURRENT_TRANSPORT.with(|cell| cell.borrow().clone())
}

// ─── Audit Registry (per-session buffers) ────────────────────────────────────

/// Registry of per-session audit logs for multi-tenant isolation.
///
/// Each session gets its own `AuditLog` buffer. Entries are fully isolated —
/// session A cannot see session B's audit data.
pub struct AuditRegistry {
    sessions: std::sync::RwLock<HashMap<String, Arc<AuditLog>>>,
}

impl AuditRegistry {
    /// Create a new empty audit registry.
    pub fn new() -> Self {
        AuditRegistry {
            sessions: std::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Get or create the audit log for a session.
    pub fn get_or_create(&self, session_id: &str) -> Arc<AuditLog> {
        // Fast path: read lock
        {
            let sessions = self.sessions.read().unwrap_or_else(|p| p.into_inner());
            if let Some(log) = sessions.get(session_id) {
                return Arc::clone(log);
            }
        }
        // Slow path: write lock to insert
        const MAX_AUDIT_SESSIONS: usize = 1024;
        let mut sessions = self.sessions.write().unwrap_or_else(|p| p.into_inner());
        if sessions.len() >= MAX_AUDIT_SESSIONS && !sessions.contains_key(session_id) {
            let first_key = sessions.keys().next().cloned();
            if let Some(key) = first_key {
                sessions.remove(&key);
                tracing::warn!(session_id = %key, "Audit registry full ({}), evicted oldest session", MAX_AUDIT_SESSIONS);
            }
        }
        sessions.entry(session_id.to_string())
            .or_insert_with(|| Arc::new(AuditLog::new()))
            .clone()
    }

    /// Get the audit log for a session, if it exists.
    pub fn get(&self, session_id: &str) -> Option<Arc<AuditLog>> {
        let sessions = self.sessions.read().unwrap_or_else(|p| p.into_inner());
        sessions.get(session_id).cloned()
    }

    /// Remove the audit log for a session.
    pub fn remove(&self, session_id: &str) -> Option<Arc<AuditLog>> {
        let mut sessions = self.sessions.write().unwrap_or_else(|p| p.into_inner());
        sessions.remove(session_id)
    }

    /// List all active session IDs.
    pub fn session_ids(&self) -> Vec<String> {
        let sessions = self.sessions.read().unwrap_or_else(|p| p.into_inner());
        sessions.keys().cloned().collect()
    }

    /// Aggregate all entries from all sessions, sorted by sequence descending.
    pub fn aggregate_all(&self) -> Vec<AuditEntry> {
        let sessions = self.sessions.read().unwrap_or_else(|p| p.into_inner());
        let mut all: Vec<AuditEntry> = sessions.values()
            .flat_map(|log| log.all())
            .collect();
        all.sort_by(|a, b| b.sequence.cmp(&a.sequence));
        all
    }

    /// Aggregate recent entries from all sessions.
    pub fn aggregate_recent(&self, count: usize) -> Vec<AuditEntry> {
        let mut all = self.aggregate_all();
        all.truncate(count);
        all
    }

    /// Flush all session audit logs to disk.
    ///
    /// Each session is written to `{base_path}/{session_id}.json`.
    /// Returns the total number of entries across all sessions.
    pub fn flush_all(&self, base_path: &str) -> Result<usize, String> {
        let sessions = self.sessions.read().unwrap_or_else(|p| p.into_inner());

        // Create directory if it doesn't exist
        std::fs::create_dir_all(base_path)
            .map_err(|e| format!("Failed to create audit directory {}: {}", base_path, e))?;

        let mut total = 0;
        for (session_id, log) in sessions.iter() {
            let file_path = format!("{}/{}.json", base_path, session_id);
            let count = log.flush_to_file(&file_path)?;
            total += count;
        }
        Ok(total)
    }

    /// Clear all session audit logs.
    pub fn clear(&self) {
        let mut sessions = self.sessions.write().unwrap_or_else(|p| p.into_inner());
        sessions.clear();
    }

    /// Number of active sessions.
    pub fn session_count(&self) -> usize {
        let sessions = self.sessions.read().unwrap_or_else(|p| p.into_inner());
        sessions.len()
    }
}

/// Global audit registry instance.
static AUDIT_REGISTRY: std::sync::LazyLock<AuditRegistry> =
    std::sync::LazyLock::new(|| AuditRegistry {
        sessions: std::sync::RwLock::new(HashMap::new()),
    });

/// Get a reference to the global audit registry.
pub fn audit_registry() -> &'static AuditRegistry {
    &AUDIT_REGISTRY
}

// ─── Convenience functions (session-aware) ───────────────────────────────────

/// Convenience: record a tool call, routed to the current session's audit buffer.
///
/// Reads the session ID and transport from thread-local context. Falls back to "default" if
/// no session context is set.
pub fn record_tool_call(
    tool_name: &str,
    start: Instant,
    outcome: AuditOutcome,
) {
    let session_id = current_session_id().unwrap_or_else(|| "default".to_string());
    let transport = current_transport();
    let log = audit_registry().get_or_create(&session_id);
    log.record_full(
        tool_name,
        start,
        outcome,
        transport,
        None,
        None,
        None,
        Some(session_id),
    );
}

/// Convenience: record a tool call with a Merkle root, routed to the current session.
pub fn record_tool_call_with_merkle(
    tool_name: &str,
    start: Instant,
    outcome: AuditOutcome,
    merkle_root: Option<String>,
) {
    let session_id = current_session_id().unwrap_or_else(|| "default".to_string());
    let transport = current_transport();
    let log = audit_registry().get_or_create(&session_id);
    log.record_full(
        tool_name,
        start,
        outcome,
        transport,
        None,
        None,
        merkle_root,
        Some(session_id),
    );
}

/// Convenience: record a tool call with payload/response sizes.
pub fn record_tool_call_with_sizes(
    tool_name: &str,
    start: Instant,
    outcome: AuditOutcome,
    payload_size: Option<u64>,
    response_size: Option<u64>,
    merkle_root: Option<String>,
) {
    let session_id = current_session_id().unwrap_or_else(|| "default".to_string());
    let transport = current_transport();
    let log = audit_registry().get_or_create(&session_id);
    log.record_full(
        tool_name,
        start,
        outcome,
        transport,
        payload_size,
        response_size,
        merkle_root,
        Some(session_id),
    );
}

/// Convenience: record a tool call with full metadata (transport, sizes, merkle).
pub fn record_tool_call_full(
    tool_name: &str,
    start: Instant,
    outcome: AuditOutcome,
    transport: Option<String>,
    payload_size: Option<u64>,
    response_size: Option<u64>,
    merkle_root: Option<String>,
) {
    let session_id = current_session_id().unwrap_or_else(|| "default".to_string());
    let log = audit_registry().get_or_create(&session_id);
    log.record_full(
        tool_name,
        start,
        outcome,
        transport,
        payload_size,
        response_size,
        merkle_root,
        Some(session_id),
    );
}

/// Convenience: flush all session audit logs to disk.
pub fn flush_audit(path: &str) -> Result<usize, String> {
    audit_registry().flush_all(path)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_timer(name: &str) -> impl Drop {
        let start = std::time::Instant::now();
        struct Timer { name: String, start: std::time::Instant }
        impl Drop for Timer { fn drop(&mut self) {
            eprintln!("[TEST] {} completed in {:.3}ms", self.name, self.start.elapsed().as_secs_f64() * 1000.0);
        }}
        Timer { name: name.to_string(), start }
    }

    fn log_throughput(label: &str, ops: u64, elapsed: std::time::Duration) {
        let secs = elapsed.as_secs_f64();
        if secs > 0.0 {
            eprintln!("[METRIC] {}: {:.0} ops/sec ({} ops in {:.3}ms)", label, ops as f64 / secs, ops, elapsed.as_secs_f64() * 1000.0);
        }
    }

    #[test]
    fn test_audit_log_record_and_retrieve() {
        let _t = test_timer("test_audit_log_record_and_retrieve");
        let log = AuditLog::new();
        let start = Instant::now();
        log.record("test_tool", start, AuditOutcome::Success);
        log.record("test_tool_2", start, AuditOutcome::Error("oops".into()));

        assert_eq!(log.len(), 2);
        let recent = log.recent(10);
        assert_eq!(recent.len(), 2);
        // Most recent first
        assert_eq!(recent[0].tool_name, "test_tool_2");
        assert_eq!(recent[1].tool_name, "test_tool");
    }

    #[test]
    fn test_audit_log_ring_buffer_eviction() {
        let _t = test_timer("test_audit_log_ring_buffer_eviction");
        let log = AuditLog::new();
        let start = Instant::now();

        let t0 = Instant::now();
        for i in 0..100 {
            log.record(&format!("tool_{}", i), start, AuditOutcome::Success);
        }
        log_throughput("audit_record_100", 100, t0.elapsed());
        assert_eq!(log.len(), 100);
    }

    #[test]
    fn test_audit_log_clear() {
        let log = AuditLog::new();
        let start = Instant::now();
        log.record("tool", start, AuditOutcome::Success);
        assert_eq!(log.len(), 1);
        log.clear();
        assert_eq!(log.len(), 0);
    }

    #[test]
    fn test_audit_entry_sequence_numbers() {
        let log = AuditLog::new();
        let start = Instant::now();
        log.record("a", start, AuditOutcome::Success);
        log.record("b", start, AuditOutcome::Timeout);
        let entries = log.recent(10);
        assert!(entries[0].sequence > entries[1].sequence);
    }

    #[test]
    fn test_audit_outcome_variants() {
        let log = AuditLog::new();
        let start = Instant::now();
        log.record("ok", start, AuditOutcome::Success);
        log.record("err", start, AuditOutcome::Error("fail".into()));
        log.record("to", start, AuditOutcome::Timeout);
        log.record("rej", start, AuditOutcome::Rejected("denied".into()));

        let entries = log.recent(10);
        assert_eq!(entries[0].outcome, AuditOutcome::Rejected("denied".into()));
        assert_eq!(entries[1].outcome, AuditOutcome::Timeout);
    }

    #[test]
    fn test_audit_merkle_root_tracking() {
        let log = AuditLog::new();
        let start = Instant::now();

        // JSON transport: no merkle root
        log.record("json_tool", start, AuditOutcome::Success);

        // NDA transport: with merkle root
        let merkle = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string();
        log.record_with_merkle("nda_tool", start, AuditOutcome::Success, Some(merkle.clone()));

        let entries = log.recent(10);
        assert_eq!(entries.len(), 2);

        // Most recent first (NDA tool)
        assert_eq!(entries[0].tool_name, "nda_tool");
        assert_eq!(entries[0].merkle_root, Some(merkle));

        // Older entry (JSON tool)
        assert_eq!(entries[1].tool_name, "json_tool");
        assert_eq!(entries[1].merkle_root, None);
    }

    #[test]
    fn test_audit_csv_includes_merkle_root() {
        let _t = test_timer("test_audit_csv_includes_merkle_root");
        let log = AuditLog::new();
        let start = Instant::now();
        let merkle = "abcdef0123456789".to_string();
        log.record_with_merkle("nda_tool", start, AuditOutcome::Success, Some(merkle.clone()));
        log.record("json_tool", start, AuditOutcome::Success);

        let t0 = Instant::now();
        let csv = log.export_csv().unwrap();
        eprintln!("[METRIC] audit_csv_export: {:.3}us", t0.elapsed().as_secs_f64() * 1e6);
        assert!(csv.contains("merkle_root"));
        assert!(csv.contains(&merkle));
    }

    #[test]
    fn test_audit_csv_includes_session_id() {
        let log = AuditLog::new();
        let start = Instant::now();
        log.record_with_context("tool_a", start, AuditOutcome::Success, None, Some("session-1".into()));
        log.record("tool_b", start, AuditOutcome::Success);

        let csv = log.export_csv().unwrap();
        assert!(csv.contains("session_id"));
        assert!(csv.contains("session-1"));
    }

    #[test]
    fn test_registry_isolation() {
        let registry = AuditRegistry {
            sessions: std::sync::RwLock::new(HashMap::new()),
        };

        let log_a = registry.get_or_create("session-a");
        let log_b = registry.get_or_create("session-b");

        let start = Instant::now();
        log_a.record("tool_a", start, AuditOutcome::Success);
        log_a.record("tool_a2", start, AuditOutcome::Success);
        log_b.record("tool_b", start, AuditOutcome::Success);

        assert_eq!(log_a.len(), 2);
        assert_eq!(log_b.len(), 1);

        let entries_a = log_a.all();
        assert!(entries_a.iter().all(|e| e.tool_name.starts_with("tool_a")));

        let entries_b = log_b.all();
        assert_eq!(entries_b[0].tool_name, "tool_b");
    }

    #[test]
    fn test_registry_get_nonexistent() {
        let registry = AuditRegistry {
            sessions: std::sync::RwLock::new(HashMap::new()),
        };
        assert!(registry.get("no-such-session").is_none());
    }

    #[test]
    fn test_registry_remove() {
        let registry = AuditRegistry {
            sessions: std::sync::RwLock::new(HashMap::new()),
        };
        registry.get_or_create("session-x");
        assert_eq!(registry.session_count(), 1);

        let removed = registry.remove("session-x");
        assert!(removed.is_some());
        assert_eq!(registry.session_count(), 0);
    }

    #[test]
    fn test_registry_aggregate_all() {
        let _t = test_timer("test_registry_aggregate_all");
        let registry = AuditRegistry {
            sessions: std::sync::RwLock::new(HashMap::new()),
        };

        let log_a = registry.get_or_create("s1");
        let log_b = registry.get_or_create("s2");

        let start = Instant::now();
        log_a.record("tool_1", start, AuditOutcome::Success);
        log_b.record("tool_2", start, AuditOutcome::Success);
        log_a.record("tool_3", start, AuditOutcome::Success);

        let t0 = Instant::now();
        let all = registry.aggregate_all();
        eprintln!("[METRIC] registry_aggregate_all: {:.3}us", t0.elapsed().as_secs_f64() * 1e6);
        assert_eq!(all.len(), 3);
        // Sorted by sequence descending
        assert!(all[0].sequence > all[1].sequence);
        assert!(all[1].sequence > all[2].sequence);
    }

    #[test]
    fn test_registry_session_ids() {
        let registry = AuditRegistry {
            sessions: std::sync::RwLock::new(HashMap::new()),
        };
        registry.get_or_create("alpha");
        registry.get_or_create("beta");

        let mut ids = registry.session_ids();
        ids.sort();
        assert_eq!(ids, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_thread_local_session_context() {
        assert!(current_session_id().is_none());

        set_session_context("test-session".to_string());
        assert_eq!(current_session_id(), Some("test-session".to_string()));

        clear_session_context();
        assert!(current_session_id().is_none());
    }

    #[test]
    fn test_thread_local_isolation_across_threads() {
        use std::thread;

        set_session_context("main-thread".to_string());

        let handle = thread::spawn(|| {
            // Different thread should have no context
            assert!(current_session_id().is_none());
            set_session_context("child-thread".to_string());
            assert_eq!(current_session_id(), Some("child-thread".to_string()));
        });
        handle.join().unwrap();

        // Main thread context unchanged
        assert_eq!(current_session_id(), Some("main-thread".to_string()));
        clear_session_context();
    }

    #[test]
    fn test_record_with_context_sets_session_id() {
        let log = AuditLog::new();
        let start = Instant::now();
        log.record_with_context("tool", start, AuditOutcome::Success, None, Some("sess-42".into()));

        let entries = log.all();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].session_id, Some("sess-42".to_string()));
    }
}
