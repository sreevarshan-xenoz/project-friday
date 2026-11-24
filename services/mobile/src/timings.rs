//! How long each stage of an ask takes, kept on this computer
//! (`timings.jsonl` beside `state.json`, one line per stage): the screen
//! capture, the first word of the answer and the whole answer, from when the
//! daemon got the ask. `friday-ctl timings` (and `friday doctor`) shows the
//! median and the slow end (p95) of each, with the providers' own first words
//! from the egress log. The keypress and the drawing on screen happen in the
//! hosts, so they aren't in it.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

static OPEN: LazyLock<Mutex<HashMap<String, Instant>>> = LazyLock::new(Default::default);

/// Something started now (an ask, by its goal id; a screen capture).
pub fn start(key: &str) {
    let mut open = OPEN.lock().unwrap();
    // An ask that never finished isn't kept for ever.
    open.retain(|_, t| t.elapsed() < Duration::from_secs(600));
    open.insert(key.to_string(), Instant::now());
}

/// `stage` took this long since `key` started; `key` stays open.
pub fn lap(key: &str, stage: &str) {
    let at = OPEN.lock().unwrap().get(key).copied();
    if let Some(t) = at {
        record(stage, t.elapsed().as_millis() as u64);
    }
}

/// `stage` took this long since `key` started; `key` is done.
pub fn end(key: &str, stage: &str) {
    let at = OPEN.lock().unwrap().remove(key);
    if let Some(t) = at {
        record(stage, t.elapsed().as_millis() as u64);
    }
}

fn path() -> std::path::PathBuf {
    friday_intelligence::egress::default_state_dir().join("timings.jsonl")
}

fn record(stage: &str, ms: u64) {
    // Tests never write to the person's own folder.
    if cfg!(test) || !crate::features::on("ask-timings") {
        return;
    }
    let p = path();
    let line = serde_json::json!({
        "ts": chrono::Utc::now().timestamp(),
        "stage": stage,
        "ms": ms,
    })
    .to_string();
    let _ = crate::jsonl::append(&p, &line);
    // Kept small: the newest 2000 lines once it grows past about 200 KB.
    if std::fs::metadata(&p).is_ok_and(|m| m.len() > 200_000) {
        if let Ok(text) = std::fs::read_to_string(&p) {
            let lines: Vec<&str> = text.lines().collect();
            let keep = lines[lines.len().saturating_sub(2000)..].join("\n") + "\n";
            let _ = std::fs::write(&p, keep);
        }
    }
}

/// The file's text, empty when there is none.
pub fn read() -> String {
    std::fs::read_to_string(path()).unwrap_or_default()
}

fn pct(v: &mut [u64], p: usize) -> u64 {
    v.sort_unstable();
    v[((v.len() - 1) * p) / 100]
}

fn secs(ms: u64) -> String {
    format!("{:.1} s", ms as f64 / 1000.0)
}

/// The stages since `since` (unix seconds), p50 and p95 each.
pub fn report(timings: &str, egress: &str, since: i64, days: u32) -> String {
    let mut by: HashMap<String, Vec<u64>> = HashMap::new();
    for line in timings.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let (Some(ts), Some(stage), Some(ms)) =
            (v["ts"].as_i64(), v["stage"].as_str(), v["ms"].as_u64())
        else {
            continue;
        };
        if ts >= since {
            by.entry(stage.to_string()).or_default().push(ms);
        }
    }
    for line in egress.lines() {
        let Ok(e) = serde_json::from_str::<friday_intelligence::EgressEntry>(line) else {
            continue;
        };
        let recent =
            chrono::DateTime::parse_from_rfc3339(&e.ts).is_ok_and(|t| t.timestamp() >= since);
        if let (true, "ok", Some(f)) = (recent, e.outcome.as_str(), e.first_ms) {
            by.entry("provider".into()).or_default().push(f);
        }
    }
    let rows = [
        ("capture", "screen capture, until it went"),
        ("provider", "the provider's first words"),
        ("first_word", "the first word, from the ask"),
        ("answer", "the whole answer"),
    ];
    let mut out = vec![format!(
        "How long asks took, last {days} day(s) (median / slowest 5%):"
    )];
    for (key, label) in rows {
        if let Some(v) = by.get_mut(key).filter(|v| !v.is_empty()) {
            let n = v.len();
            out.push(format!(
                "  {label}: {} / {} ({n})",
                secs(pct(v, 50)),
                secs(pct(v, 95))
            ));
        }
    }
    if out.len() == 1 {
        return format!("No asks timed in the last {days} day(s).");
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_stage_shows_its_median_and_slow_end() {
        let mut t = String::new();
        for ms in [1000, 1200, 1100, 900, 8000] {
            t.push_str(&format!(
                "{{\"ts\":100,\"stage\":\"first_word\",\"ms\":{ms}}}\n"
            ));
        }
        t.push_str("{\"ts\":100,\"stage\":\"capture\",\"ms\":300}\n");
        t.push_str("{\"ts\":1,\"stage\":\"answer\",\"ms\":99}\nnot json\n");
        let egress = r#"{"ts":"2026-11-24T08:00:00Z","provider":"p","model":"m","kinds":["text"],"text_chars":1,"outcome":"ok","ms":3000,"first_ms":700}"#;
        let out = report(&t, egress, 50, 7);
        assert!(
            out.contains("the first word, from the ask: 1.1 s / 1.2 s (5)"),
            "{out}"
        );
        assert!(
            out.contains("screen capture, until it went: 0.3 s / 0.3 s (1)"),
            "{out}"
        );
        assert!(out.contains("the provider's first words: 0.7 s"), "{out}");
        assert!(
            !out.contains("whole answer"),
            "older lines are left out: {out}"
        );
        assert!(report("", "", 0, 7).starts_with("No asks timed"));
    }

    #[test]
    fn an_ask_is_timed_from_its_start() {
        start("g1");
        std::thread::sleep(Duration::from_millis(5));
        lap("g1", "first_word");
        end("g1", "answer");
        assert!(!OPEN.lock().unwrap().contains_key("g1"));
        end("never", "answer");
    }
}
