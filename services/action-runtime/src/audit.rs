//! The Audit Engine: every action leaves a breadcrumb.
//!
//! For each dispatched action the runtime records `who / what / when / trace id
//! / latency / result` into a bounded in-memory ring. This is the foundation of
//! the platform's accountability story (see `docs/Security.md`): nothing the
//! agent does is invisible. A later milestone persists this to durable memory;
//! for now it is queryable in-process and mirrored to the tracing log.

use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::sync::Mutex;
use uuid::Uuid;

/// One immutable record of an action that was evaluated by the runtime.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuditEntry {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub trace_id: Uuid,
    pub tool: String,
    pub action: String,
    /// Permission verdict that was applied (deny / allow / sandbox).
    pub decision: String,
    /// Wall-clock latency of the dispatch in milliseconds.
    pub latency_ms: u128,
    pub success: bool,
    pub detail: Option<String>,
}

/// Bounded ring of recent audit entries.
pub struct AuditEngine {
    entries: Mutex<VecDeque<AuditEntry>>,
    capacity: usize,
}

impl Default for AuditEngine {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl AuditEngine {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    pub fn record(&self, entry: AuditEntry) {
        tracing::info!(
            audit_id = %entry.id,
            tool = %entry.tool,
            action = %entry.action,
            decision = %entry.decision,
            success = entry.success,
            latency_ms = entry.latency_ms,
            "action audited"
        );
        let mut guard = self.entries.lock().expect("audit ring poisoned");
        if guard.len() >= self.capacity {
            guard.pop_front();
        }
        guard.push_back(entry);
    }

    /// Most recent `n` entries, newest last.
    pub fn recent(&self, n: usize) -> Vec<AuditEntry> {
        let guard = self.entries.lock().expect("audit ring poisoned");
        guard.iter().rev().take(n).rev().cloned().collect()
    }

    pub fn count(&self) -> usize {
        self.entries.lock().expect("audit ring poisoned").len()
    }
}
