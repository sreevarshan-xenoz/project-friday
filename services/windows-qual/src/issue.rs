//! A benchmark-linked qualification issue.
//!
//! Every issue points back to the corpus item / failure injection that produced
//! it, so the loop `harness -> evidence -> issue -> fix` is closed. This is the
//! "file them, but make them point back to a benchmark" discipline.

use serde::Serialize;

/// Severity is intentionally minimal: an issue is either open or resolved.
#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub id: String,
    pub title: String,
    /// Which corpus group / scenario observed the failure.
    pub observed: String,
    pub expected: String,
    pub actual: String,
    /// Human-readable pointer to the benchmark + failure-injection catalog.
    pub benchmark: String,
    pub status: String,
}

impl Issue {
    pub fn to_markdown(&self) -> String {
        format!(
            "{}  \n\n\
             ## Title  \n{}  \n\n\
             ## Observed  \n{}  \n\n\
             ## Expected  \n{}  \n\n\
             ## Actual  \n{}  \n\n\
             ## Benchmark  \n{}  \n\n\
             ## Status  \n{}  \n\n",
            self.id,
            self.title,
            self.observed,
            self.expected,
            self.actual,
            self.benchmark,
            self.status
        )
    }
}
