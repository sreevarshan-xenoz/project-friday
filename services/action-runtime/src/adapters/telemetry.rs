//! Measured provider evidence — the *facts* side of [`ExpertProfile`] beliefs.
//!
//! [`ProviderTelemetry`] records one outcome per provider consult (success /
//! failure + latency) and rolls them into [`ProviderStats`]. The expert router
//! blends a provider's advertised profile with these measurements as the
//! sample count grows: **beliefs set the ceiling, evidence validates or
//! demotes** — measurements never inflate a profile beyond what was
//! advertised.
//!
//! Evidence persists as JSON (atomic temp+rename) under
//! `~/.friday/telemetry/reasoning-providers.json`, so a week of
//! successes/latencies survives restarts — the same pattern as the
//! intelligence fabric's health snapshots. The audit trail reads it back via
//! `reasoning.stats`.
//!
//! Two honest limitations (documented, not hidden): evidence only accumulates
//! for providers that were actually *attempted* — an expert always ranked
//! behind a winner gets no data, so discovery of a better expert is future
//! exploration work, not this layer's job; and persistence is best-effort
//! (load/write failures warn, never fail the consult). Writes are synchronous
//! under the stats lock — fine for a personal daemon, write-behind/tokio-fs
//! is the future optimization if fan-out ever needs it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::adapters::provider::ProviderId;

/// One recorded consult outcome for a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeasuredOutcome {
    pub provider: ProviderId,
    pub success: bool,
    pub duration_ms: u64,
    /// Failure reason (truncated) — the *why* for the audit trail.
    pub error: Option<String>,
    /// Unix milliseconds.
    pub at_ms: i64,
}

/// Rolling measured stats for one provider — what the router's evidence blend
/// reads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderStats {
    pub provider: ProviderId,
    pub attempts: u64,
    pub successes: u64,
    pub failures: u64,
    pub total_latency_ms: u64,
    pub last_latency_ms: u64,
    pub last_error: Option<String>,
    pub last_seen_ms: Option<i64>,
}

impl ProviderStats {
    /// Fraction of attempts that succeeded, 0.0–1.0.
    pub fn success_rate(&self) -> f32 {
        if self.attempts == 0 {
            0.0
        } else {
            self.successes as f32 / self.attempts as f32
        }
    }

    pub fn avg_latency_ms(&self) -> u64 {
        if self.attempts == 0 {
            0
        } else {
            self.total_latency_ms / self.attempts
        }
    }

    /// Evidence confidence: how much to trust the measurements. At
    /// `min_samples` samples the measured data is ~50% trusted; it
    /// asymptotes toward 1 as evidence accumulates.
    pub fn confidence(&self, min_samples: u64) -> f32 {
        if min_samples == 0 {
            return 1.0;
        }
        (self.attempts as f32 / (self.attempts as f32 + min_samples as f32)).clamp(0.0, 1.0)
    }
}

/// Default evidence file: `~/.friday/telemetry/reasoning-providers.json`.
pub fn default_telemetry_path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".friday")
        .join("telemetry")
        .join("reasoning-providers.json")
}

/// Write JSON atomically (temp file + rename) so a crash mid-write cannot
/// corrupt the evidence file — same pattern as the fabric's health snapshots.
fn write_atomic(path: &PathBuf, stats: &HashMap<ProviderId, ProviderStats>) -> Result<(), String> {
    let json = serde_json::to_string_pretty(stats).map_err(|e| e.to_string())?;
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("create telemetry dir: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

/// The per-provider measurement store. Cheap to clone into an `Arc` and share
/// between the router (writer) and anything that reads the evidence back.
#[derive(Default)]
pub struct ProviderTelemetry {
    stats: Mutex<HashMap<ProviderId, ProviderStats>>,
    path: Option<PathBuf>,
}

impl ProviderTelemetry {
    /// In-memory telemetry (no persistence).
    pub fn new() -> Self {
        Self::default()
    }

    /// Telemetry with persistence: any existing evidence file is loaded on
    /// construction and every [`Self::record`] rewrites it atomically. The
    /// file is only created on the first recorded outcome.
    pub fn with_persist(path: PathBuf) -> Self {
        let mut t = Self::new();
        if path.exists() {
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|s| {
                    serde_json::from_str::<HashMap<ProviderId, ProviderStats>>(&s)
                        .map_err(|e| e.to_string())
                }) {
                Ok(saved) => *t.stats.lock().expect("telemetry lock poisoned") = saved,
                Err(e) => {
                    tracing::warn!(path = %path.display(), "ignoring unreadable provider telemetry: {e}")
                }
            }
        }
        t.path = Some(path);
        t
    }

    /// Record one outcome and (if persistence is attached) persist.
    pub fn record(&self, outcome: MeasuredOutcome) {
        {
            let mut stats = self.stats.lock().expect("telemetry lock poisoned");
            let s = stats.entry(outcome.provider).or_default();
            s.provider = outcome.provider;
            s.attempts += 1;
            if outcome.success {
                s.successes += 1;
            } else {
                s.failures += 1;
            }
            s.total_latency_ms += outcome.duration_ms;
            s.last_latency_ms = outcome.duration_ms;
            s.last_error = outcome.error;
            s.last_seen_ms = Some(outcome.at_ms);
        }
        if let Some(path) = &self.path {
            let stats = self.stats.lock().expect("telemetry lock poisoned");
            if let Err(e) = write_atomic(path, &stats) {
                tracing::warn!(path = %path.display(), "provider telemetry persist failed: {e}");
            }
        }
    }

    /// All providers' stats, most-measured first.
    pub fn snapshot(&self) -> Vec<ProviderStats> {
        let mut v: Vec<ProviderStats> = self
            .stats
            .lock()
            .expect("telemetry lock poisoned")
            .values()
            .cloned()
            .collect();
        v.sort_by_key(|s| std::cmp::Reverse(s.attempts));
        v
    }
    /// A provider's rolling stats. The `provider` field inside is
    /// denormalized-for-readability — the map key is the canonical identity,
    /// and the field mirrors it so snapshots/JSON read self-describing.
    pub fn stats(&self, id: ProviderId) -> Option<ProviderStats> {
        self.stats
            .lock()
            .expect("telemetry lock poisoned")
            .get(&id)
            .cloned()
    }

    /// Persist the current snapshot now (used by hosts that want a checkpoint).
    pub fn save(&self) -> Result<(), String> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| "no telemetry path configured".to_string())?;
        let stats = self.stats.lock().expect("telemetry lock poisoned");
        write_atomic(path, &stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    fn outcome(provider: ProviderId, success: bool, ms: u64) -> MeasuredOutcome {
        MeasuredOutcome {
            provider,
            success,
            duration_ms: ms,
            error: if success {
                None
            } else {
                Some("boom".to_string())
            },
            at_ms: now_ms(),
        }
    }

    #[test]
    fn record_aggregates_success_failure_and_latency() {
        let t = ProviderTelemetry::new();
        t.record(outcome(ProviderId::ChatGPT, true, 100));
        t.record(outcome(ProviderId::ChatGPT, true, 200));
        t.record(outcome(ProviderId::ChatGPT, true, 300));
        t.record(outcome(ProviderId::ChatGPT, false, 50));

        let s = t.stats(ProviderId::ChatGPT).expect("stats");
        assert_eq!(s.attempts, 4);
        assert_eq!(s.successes, 3);
        assert_eq!(s.failures, 1);
        assert_eq!(s.avg_latency_ms(), 162); // (100+200+300+50)/4
        assert_eq!(s.last_latency_ms, 50);
        assert!((s.success_rate() - 0.75).abs() < f32::EPSILON);
        assert_eq!(s.last_error.as_deref(), Some("boom"));
        assert!(s.last_seen_ms.is_some());

        // Unmeasured providers have no stats (no fabricated evidence).
        assert_eq!(t.stats(ProviderId::Grok), None);
    }

    #[test]
    fn confidence_ramps_with_samples() {
        let s = ProviderStats::default();
        assert_eq!(s.confidence(8), 0.0);
        let s8 = ProviderStats {
            attempts: 8,
            ..ProviderStats::default()
        };
        assert!((s8.confidence(8) - 0.5).abs() < 1e-6, "8/16 = 0.5");
        let s24 = ProviderStats {
            attempts: 24,
            ..ProviderStats::default()
        };
        assert!((s24.confidence(8) - 0.75).abs() < 1e-6, "24/32 = 0.75");
    }

    #[test]
    fn snapshot_orders_most_measured_first() {
        let t = ProviderTelemetry::new();
        t.record(outcome(ProviderId::Grok, true, 10));
        t.record(outcome(ProviderId::Grok, true, 10));
        t.record(outcome(ProviderId::ChatGPT, true, 10));
        let snap = t.snapshot();
        assert_eq!(snap[0].provider, ProviderId::Grok);
        assert_eq!(snap[0].attempts, 2);
        assert_eq!(snap[1].provider, ProviderId::ChatGPT);
    }

    #[test]
    fn persist_roundtrips_across_instances() {
        let dir = std::env::temp_dir().join(format!(
            "friday-telemetry-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let path = dir.join("reasoning-providers.json");

        let t = ProviderTelemetry::with_persist(path.clone());
        t.record(outcome(ProviderId::ChatGPT, true, 120));
        t.record(outcome(ProviderId::Perplexity, false, 900));
        drop(t);

        // A fresh instance (e.g. the daemon restarted) reloads the evidence.
        let reloaded = ProviderTelemetry::with_persist(path.clone());
        let chatgpt = reloaded
            .stats(ProviderId::ChatGPT)
            .expect("reloaded chatgpt");
        assert_eq!(chatgpt.attempts, 1);
        assert_eq!(chatgpt.last_latency_ms, 120);
        let perp = reloaded
            .stats(ProviderId::Perplexity)
            .expect("reloaded perplexity");
        assert_eq!(perp.failures, 1);
        assert_eq!(perp.last_error.as_deref(), Some("boom"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_without_path_errors() {
        let t = ProviderTelemetry::new();
        assert!(t.save().is_err(), "no path configured");
    }
}
