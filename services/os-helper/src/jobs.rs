//! Job table and execution state tracking.
//!
//! Tracks at most the last 20 jobs in memory, manages states (`queued`, `running`,
//! `done`, `failed`), and buffers log output with caps at 200 lines and 16 KiB.

use serde::{Deserialize, Serialize};

/// Maximum number of jobs retained in memory.
pub const MAX_JOBS: usize = 20;

/// Maximum number of log lines returned in log tail.
pub const MAX_LOG_LINES: usize = 200;

/// Maximum byte size of log tail (16 KiB).
pub const MAX_LOG_BYTES: usize = 16 * 1024;

/// Execution state of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed,
}

impl JobState {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Running => "running",
            JobState::Done => "done",
            JobState::Failed => "failed",
        }
    }
}

/// Buffer for job output lines, capped by line count and byte length.
#[derive(Debug, Clone, Default)]
pub struct LogBuffer {
    lines: Vec<String>,
}

impl LogBuffer {
    pub fn push(&mut self, line: String) {
        self.lines.push(line);
        // Retain reasonable memory bounds if a runaway command outputs millions of lines
        if self.lines.len() > 1000 {
            let drain_count = self.lines.len() - MAX_LOG_LINES;
            self.lines.drain(..drain_count);
        }
    }

    /// Formats the log tail: at most the last 200 lines and at most 16 KiB.
    pub fn tail(&self) -> String {
        let start_idx = self.lines.len().saturating_sub(MAX_LOG_LINES);
        let slice = &self.lines[start_idx..];
        let joined = slice.join("\n");
        if joined.len() <= MAX_LOG_BYTES {
            return joined;
        }

        let start_byte = joined.len() - MAX_LOG_BYTES;
        let mut boundary = start_byte;
        while boundary < joined.len() && !joined.is_char_boundary(boundary) {
            boundary += 1;
        }
        joined[boundary..].to_string()
    }
}

/// A tracked job entry.
pub struct Job {
    pub id: String,
    pub state: JobState,
    pub log: LogBuffer,
}

impl Job {
    /// Serializes job metadata to JSON.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "state": self.state.as_str(),
            "log_lines": self.log.lines.len(),
        })
    }
}

/// In-memory table of recent jobs.
#[derive(Default)]
pub struct JobTable {
    jobs: Vec<Job>,
}

impl JobTable {
    pub fn new() -> Self {
        Self { jobs: Vec::new() }
    }

    /// Returns true if any job is currently queued or running.
    pub fn is_running(&self) -> bool {
        self.jobs
            .iter()
            .any(|j| j.state == JobState::Queued || j.state == JobState::Running)
    }

    /// Appends a new job in the `Queued` state.
    ///
    /// Fails if another job is currently active.
    /// Drops the oldest job if the table has reached `MAX_JOBS`.
    pub fn add_job(&mut self, id: String) -> Result<(), String> {
        if self.is_running() {
            return Err("another job is running".into());
        }
        if self.jobs.len() >= MAX_JOBS {
            self.jobs.remove(0);
        }
        self.jobs.push(Job {
            id,
            state: JobState::Queued,
            log: LogBuffer::default(),
        });
        Ok(())
    }

    /// Updates the state of a job.
    pub fn set_state(&mut self, id: &str, state: JobState) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
            job.state = state;
        }
    }

    /// Appends a log line to a job's output buffer.
    pub fn add_log_line(&mut self, id: &str, line: &str) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
            job.log.push(line.to_string());
        }
    }

    /// Looks up a job by its unique ID.
    pub fn get(&self, id: &str) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }

    /// Number of tracked jobs.
    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_tail_cap_lines() {
        let mut buffer = LogBuffer::default();
        for i in 1..=250 {
            buffer.push(format!("line {i}"));
        }
        let tail = buffer.tail();
        let lines: Vec<&str> = tail.lines().collect();
        assert_eq!(lines.len(), 200);
        assert_eq!(lines.first().copied(), Some("line 51"));
        assert_eq!(lines.last().copied(), Some("line 250"));
    }

    #[test]
    fn test_log_tail_cap_bytes() {
        let mut buffer = LogBuffer::default();
        // 10 lines of 3 KiB each = ~30 KiB total, well under 200 lines
        let chunk = "x".repeat(3 * 1024);
        for i in 1..=10 {
            buffer.push(format!("{chunk}-line{i}"));
        }
        let tail = buffer.tail();
        assert!(tail.len() <= MAX_LOG_BYTES);
        assert!(tail.ends_with("-line10"));
    }

    #[test]
    fn test_job_table_max_20() {
        let mut table = JobTable::new();
        for i in 1..=25 {
            let id = format!("job-{i}");
            assert!(table.add_job(id.clone()).is_ok());
            table.set_state(&id, JobState::Done);
        }

        assert_eq!(table.len(), 20);
        // Oldest jobs 1..=5 dropped
        assert!(table.get("job-1").is_none());
        assert!(table.get("job-5").is_none());
        // Newer jobs 6..=25 present
        assert!(table.get("job-6").is_some());
        assert!(table.get("job-25").is_some());
    }

    #[test]
    fn test_job_table_one_job_at_a_time() {
        let mut table = JobTable::new();
        assert!(table.add_job("job-1".into()).is_ok());
        // While queued, second job fails
        assert_eq!(
            table.add_job("job-2".into()).unwrap_err(),
            "another job is running"
        );

        table.set_state("job-1", JobState::Running);
        // While running, second job fails
        assert_eq!(
            table.add_job("job-2".into()).unwrap_err(),
            "another job is running"
        );

        table.set_state("job-1", JobState::Done);
        // Once done, next job succeeds
        assert!(table.add_job("job-2".into()).is_ok());
    }

    #[test]
    fn test_job_to_json() {
        let mut job = Job {
            id: "job-10".into(),
            state: JobState::Running,
            log: LogBuffer::default(),
        };
        job.log.push("first line".into());
        let val = job.to_json();
        assert_eq!(val["id"], "job-10");
        assert_eq!(val["state"], "running");
        assert_eq!(val["log_lines"], 1);
    }
}
