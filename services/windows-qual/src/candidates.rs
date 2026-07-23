//! Candidate-issue store.
//!
//! A failure becomes a *candidate* on first observation and only graduates to a
//! real, tracker-worthy issue after it repeats (`GRADUATE_THRESHOLD`
//! occurrences). This keeps a single flaky benchmark from polluting the issue
//! tracker — the loop is `failure -> candidate -> human review -> issue`.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::issue::Issue;

/// Observations required before a candidate becomes a real issue.
pub const GRADUATE_THRESHOLD: usize = 2;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CandidateStore {
    pub entries: HashMap<String, Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub scenario_id: String,
    pub count: usize,
    pub first_seen: String,
    pub last_seen: String,
    pub graduated: bool,
}

impl CandidateStore {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)
    }

    /// Record an observation of `issue` (keyed by `issue.id`). Returns the new
    /// observation count and whether the candidate has now graduated.
    pub fn observe(&mut self, issue: &Issue, now: &str) -> (usize, bool) {
        let e = self.entries.entry(issue.id.clone()).or_insert(Entry {
            scenario_id: issue.observed.clone(),
            count: 0,
            first_seen: now.to_string(),
            last_seen: now.to_string(),
            graduated: false,
        });
        e.count += 1;
        e.last_seen = now.to_string();
        e.scenario_id = issue.observed.clone();
        if !e.graduated && e.count >= GRADUATE_THRESHOLD {
            e.graduated = true;
            (e.count, true)
        } else {
            (e.count, e.graduated)
        }
    }
}
