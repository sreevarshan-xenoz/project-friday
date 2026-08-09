//! Chaos Laboratory — Phase 2: Health Persistence Corruption.
//!
//! Stress-tests the fabric's health persistence layer (`load_health` /
//! `persist_health`) against every way the state file can be wrong. Each
//! scenario injects one corruption into the health-state file, reloads it, and
//! checks the recovery contract:
//!
//!   1. never panic;
//!   2. never poison scoring — loaded reliability is always in `0.0..=1.0`;
//!   3. fall back to default health (1.0 / circuit Closed) with a warning
//!      when the file is unusable;
//!   4. a subsequent `persist_health` must restore a complete, valid file
//!      (atomic temp + rename, so a crash mid-write can't destroy it).
//!
//! Verdicts are *returned*, never panicked, matching Phase 1.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use friday_intelligence::{InferenceFabric, ModelCapability, ModelClient, ModelInfo, RoutingMode};

use crate::chaos::ChaosVerdict;
use crate::mock::{FaultMode, MockProvider};

static TEMP_SEQ: AtomicUsize = AtomicUsize::new(0);

/// A fabric pointed at a unique temp health-state file, with one healthy
/// provider registered so the routing loop can still be driven after a load.
struct CorruptionHarness {
    fabric: InferenceFabric,
    path: PathBuf,
}

impl CorruptionHarness {
    fn new() -> Self {
        let mut fabric = InferenceFabric::new();
        fabric.default_mode = RoutingMode::Free;
        fabric.register_provider(Box::new(MockProvider::new(
            "omniroute".to_string(),
            FaultMode::Ok,
        )));
        fabric.register_model(ModelInfo {
            id: "auto".to_string(),
            family: "auto".to_string(),
            provider: "omniroute".to_string(),
            latency_ms: 100,
            reliability: 1.0,
            capabilities: vec![ModelCapability::Json],
            max_context: 4096,
            tier: None,
        });

        let seq = TEMP_SEQ.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("friday-chaos2-{}-{seq}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(temp_sibling(&path));

        fabric.health_state_path = path.clone();
        Self { fabric, path }
    }

    /// Write raw bytes as the health-state file.
    fn write(&self, bytes: &[u8]) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&self.path, bytes).expect("harness writes the corrupt file");
    }

    fn write_str(&self, s: &str) {
        self.write(s.as_bytes());
    }

    fn health(&self, provider: &str) -> Option<f64> {
        self.fabric
            .health_scores
            .lock()
            .unwrap()
            .get(provider)
            .copied()
    }

    fn circuit_state(&self, provider: &str) -> String {
        let cb = self.fabric.circuits.lock().unwrap();
        match cb.get(provider).map(|c| c.state) {
            Some(friday_intelligence::CircuitState::Closed) => "Closed".to_string(),
            Some(friday_intelligence::CircuitState::Open) => "Open".to_string(),
            Some(friday_intelligence::CircuitState::HalfOpen) => "HalfOpen".to_string(),
            None => "missing".to_string(),
        }
    }

    async fn request_ok(&self) -> bool {
        matches!(
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                self.fabric.complete("probe")
            )
            .await,
            Ok(Ok(_))
        )
    }
}

impl Drop for CorruptionHarness {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(temp_sibling(&self.path));
    }
}

/// Sibling path of the health-state file (matches the fabric's atomic write).
fn temp_sibling(path: &std::path::Path) -> PathBuf {
    let mut os = path.as_os_str().to_owned();
    os.push(".tmp");
    PathBuf::from(os)
}

fn verdict(id: &'static str, passed: bool, note: String) -> ChaosVerdict {
    ChaosVerdict {
        id,
        passed,
        note,
        observations: Vec::new(),
    }
}

/// C1: the file is not JSON at all. Recovery must be silent-but-warned: health
/// stays at defaults, routing still answers.
pub async fn corrupt_json() -> ChaosVerdict {
    let h = CorruptionHarness::new();
    h.write_str("this is definitely not json {[[[");
    h.fabric.load_health();

    let health = h.health("omniroute");
    let state = h.circuit_state("omniroute");
    let request_ok = h.request_ok().await;
    let passed = health == Some(1.0) && state == "Closed" && request_ok;
    verdict(
        "chaos2.corrupt_json",
        passed,
        format!(
            "garbage file → health {health:?} (expect 1.0), circuit {state} (expect Closed), \
             request answered: {request_ok}; no panic"
        ),
    )
}

/// C2: impossible reliability values. Finite out-of-range values must be
/// clamped (5.0 → 1.0, -0.5 → 0.0); a valid entry alongside must load exactly.
/// Non-finite values cannot survive JSON, so clamping is the reachable case.
pub async fn impossible_values() -> ChaosVerdict {
    let h = CorruptionHarness::new();
    h.write_str(
        r#"[{"name":"omniroute","reliability":0.5,"consecutive_failures":0,"circuit_state":"Closed"},
            {"name":"openrouter","reliability":5.0,"consecutive_failures":0,"circuit_state":"Closed"},
            {"name":"opengateway","reliability":-0.5,"consecutive_failures":0,"circuit_state":"Closed"}]"#,
    );
    h.fabric.load_health();

    let valid = h.health("omniroute") == Some(0.5);
    let high_clamped = h.health("openrouter") == Some(1.0);
    let low_clamped = h.health("opengateway") == Some(0.0);
    let all_sane = [0.5_f64, 1.0, 0.0]
        .iter()
        .zip(["omniroute", "openrouter", "opengateway"])
        .all(|(expected, name)| h.health(name).is_some_and(|v| (v - expected).abs() < 1e-9));
    let request_ok = h.request_ok().await;
    let passed = valid && high_clamped && low_clamped && all_sane && request_ok;
    verdict(
        "chaos2.impossible_values",
        passed,
        format!(
            "omniroute {valid} (0.5 kept), openrouter 5.0→1.0 {high_clamped}, \
             opengateway -0.5→0.0 {low_clamped}; all within [0,1] {all_sane}; request answered: {request_ok}"
        ),
    )
}

/// C3: no health-state file at all. First boot. Defaults, no corruption noise,
/// routing works.
pub async fn missing_file() -> ChaosVerdict {
    let h = CorruptionHarness::new();
    h.fabric.load_health();

    let health = h.health("omniroute");
    let request_ok = h.request_ok().await;
    let passed = health == Some(1.0) && request_ok;
    verdict(
        "chaos2.missing_file",
        passed,
        format!("no file → health {health:?} (expect 1.0 default); request answered: {request_ok}"),
    )
}

/// C4: a valid serialization cut mid-way (e.g. interrupted read/copy). Fallback
/// to defaults; a persist must restore a complete, valid file.
pub async fn truncated_file() -> ChaosVerdict {
    let h = CorruptionHarness::new();
    let valid = serde_json::to_vec_pretty(&vec![friday_intelligence::PersistedProviderHealth {
        name: "omniroute".to_string(),
        reliability: 0.4,
        consecutive_failures: 1,
        circuit_state: "HalfOpen".to_string(),
    }])
    .unwrap();
    h.write(&valid[..valid.len() / 2]);
    h.fabric.load_health();

    let fell_back = h.health("omniroute") == Some(1.0);
    h.fabric.persist_health();
    let repaired = std::fs::read(&h.path)
        .map(|bytes| {
            serde_json::from_slice::<Vec<friday_intelligence::PersistedProviderHealth>>(&bytes)
                .is_ok()
        })
        .unwrap_or(false);
    let request_ok = h.request_ok().await;
    let passed = fell_back && repaired && request_ok;
    verdict(
        "chaos2.truncated_file",
        passed,
        format!(
            "half-file → default health {fell_back}; persist rewrote a valid file {repaired}; \
             request answered: {request_ok}"
        ),
    )
}

/// C5: empty file (0 bytes). Fallback to defaults, no panic.
pub async fn empty_file() -> ChaosVerdict {
    let h = CorruptionHarness::new();
    h.write(b"");
    h.fabric.load_health();

    let fell_back = h.health("omniroute") == Some(1.0);
    let request_ok = h.request_ok().await;
    let passed = fell_back && request_ok;
    verdict(
        "chaos2.empty_file",
        passed,
        format!("empty file → default health {fell_back}; request answered: {request_ok}"),
    )
}

/// C6: a file written by an older build (missing fields the current struct
/// requires). Missing `consecutive_failures`/`circuit_state` fails deserialize
/// → whole file rejected → default health. Never a partial or half-loaded state.
pub async fn older_schema() -> ChaosVerdict {
    let h = CorruptionHarness::new();
    h.write_str(r#"[{"name":"omniroute","reliability":0.5}]"#);
    h.fabric.load_health();

    let fell_back = h.health("omniroute") == Some(1.0);
    let request_ok = h.request_ok().await;
    let passed = fell_back && request_ok;
    verdict(
        "chaos2.older_schema",
        passed,
        format!(
            "older-schema object (missing fields) → rejected as a whole, default health {fell_back}; \
             request answered: {request_ok}"
        ),
    )
}

/// C7: a file from a newer build with extra fields. Unknown fields must be
/// ignored (not rejected), known fields load exactly — a forward-compatible
/// read.
pub async fn newer_schema() -> ChaosVerdict {
    let h = CorruptionHarness::new();
    h.write_str(
        r#"[{"name":"omniroute","reliability":0.4,"consecutive_failures":1,"circuit_state":"HalfOpen",
            "version":2,"last_updated":"2026-08-07T00:00:00Z","schema_url":"https://example/schema.json"}]"#,
    );
    h.fabric.load_health();

    let health_ok = h
        .health("omniroute")
        .is_some_and(|v| (v - 0.4).abs() < 1e-9);
    let state = h.circuit_state("omniroute");
    let request_ok = h.request_ok().await;
    let passed = health_ok && state == "HalfOpen" && request_ok;
    verdict(
        "chaos2.newer_schema",
        passed,
        format!(
            "extra fields ignored; health 0.4 loaded exactly {health_ok}, circuit {state} \
             (expect HalfOpen); request answered: {request_ok}"
        ),
    )
}

/// C8: the same provider appears twice. Recovery must be deterministic — the
/// last entry wins (matches `HashMap::insert` semantics), never a panic.
pub async fn duplicate_entries() -> ChaosVerdict {
    let h = CorruptionHarness::new();
    h.write_str(
        r#"[{"name":"omniroute","reliability":0.9,"consecutive_failures":0,"circuit_state":"Closed"},
            {"name":"omniroute","reliability":0.3,"consecutive_failures":3,"circuit_state":"Open"}]"#,
    );
    h.fabric.load_health();

    let health = h.health("omniroute");
    let state = h.circuit_state("omniroute");
    let passed = health.is_some_and(|v| (v - 0.3).abs() < 1e-9) && state == "Open";
    verdict(
        "chaos2.duplicate_entries",
        passed,
        format!(
            "duplicate provider → last entry wins: health {health:?} (expect 0.3), circuit {state} \
             (expect Open); deterministic, no panic"
        ),
    )
}

/// C9: a file that looks like a crash mid-persist — a valid serialization cut
/// mid-object (the exact shape a torn write leaves). Load must fall back; a
/// later persist must atomically restore a complete file.
pub async fn partial_write() -> ChaosVerdict {
    let h = CorruptionHarness::new();
    let valid = serde_json::to_vec_pretty(&vec![friday_intelligence::PersistedProviderHealth {
        name: "omniroute".to_string(),
        reliability: 0.2,
        consecutive_failures: 2,
        circuit_state: "HalfOpen".to_string(),
    }])
    .unwrap();
    let cut = valid.len() - 18; // mid-way through the last object's `circuit_state`
    h.write(&valid[..cut]);
    h.fabric.load_health();

    let fell_back = h.health("omniroute") == Some(1.0);
    h.fabric.persist_health();
    let repaired = std::fs::read(&h.path)
        .map(|bytes| {
            serde_json::from_slice::<Vec<friday_intelligence::PersistedProviderHealth>>(&bytes)
                .is_ok()
        })
        .unwrap_or(false);
    let no_stale_temp = !temp_sibling(&h.path).exists();
    let request_ok = h.request_ok().await;
    let passed = fell_back && repaired && no_stale_temp && request_ok;
    verdict(
        "chaos2.partial_write",
        passed,
        format!(
            "torn file → default health {fell_back}; persist restored a valid file {repaired}, \
             no stale temp {no_stale_temp}; request answered: {request_ok}"
        ),
    )
}

/// Run every Phase 2 scenario, in corpus order.
pub async fn run_all() -> Vec<ChaosVerdict> {
    vec![
        corrupt_json().await,
        impossible_values().await,
        missing_file().await,
        truncated_file().await,
        empty_file().await,
        older_schema().await,
        newer_schema().await,
        duplicate_entries().await,
        partial_write().await,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn phase2_scenarios_meet_pass_criteria() {
        for v in run_all().await {
            assert!(v.passed, "{}: {}", v.id, v.note);
        }
    }
}
