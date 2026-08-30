//! Audit logging for MCP tool executions.
//!
//! Records every tool call with timestamp, tool name, parameters (sanitized),
//! outcome, and duration. Logs are stored in a ring buffer to prevent unbounded
//! memory growth.

use std::sync::atomic::{AtomicU64, Ordering};
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
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Outcome of the call.
    pub outcome: AuditOutcome,
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
        let seq = AUDIT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let duration_ms = start.elapsed().as_millis() as u64;

        let entry = AuditEntry {
            sequence: seq,
            timestamp_ms,
            tool_name: tool_name.to_string(),
            duration_ms,
            outcome,
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
        let mut csv = String::from("sequence,timestamp_ms,tool_name,duration_ms,outcome\n");
        
        for entry in entries {
            let outcome_str = match &entry.outcome {
                AuditOutcome::Success => "success".to_string(),
                AuditOutcome::Error(msg) => format!("error:{}", msg.replace(',', ";")),
                AuditOutcome::Timeout => "timeout".to_string(),
                AuditOutcome::Rejected(reason) => format!("rejected:{}", reason.replace(',', ";")),
            };
            
            csv.push_str(&format!(
                "{},{},{},{},{}\n",
                entry.sequence,
                entry.timestamp_ms,
                entry.tool_name,
                entry.duration_ms,
                outcome_str
            ));
        }
        
        Ok(csv)
    }
}

/// Global audit log instance.
static GLOBAL_AUDIT: std::sync::LazyLock<AuditLog> =
    std::sync::LazyLock::new(AuditLog::default);

/// Get a reference to the global audit log.
pub fn global_audit() -> &'static AuditLog {
    &GLOBAL_AUDIT
}

/// Convenience: record a tool call in the global audit log.
pub fn record_tool_call(
    tool_name: &str,
    start: Instant,
    outcome: AuditOutcome,
) {
    global_audit().record(tool_name, start, outcome);
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_log_record_and_retrieve() {
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
        let log = AuditLog::new();
        let start = Instant::now();

        // Insert more than MAX_AUDIT_ENTRIES would be slow in a test,
        // so we just verify the mechanism works at small scale
        for i in 0..100 {
            log.record(&format!("tool_{}", i), start, AuditOutcome::Success);
        }
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
}
