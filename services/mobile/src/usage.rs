//! `friday usage`: how each AI provider has done lately, from the local
//! egress log (`egress.jsonl`, one line per call, written as calls go out).
//! Calls, failures, free-limit hits, seconds to the first words and to the
//! whole answer, and tokens: the provider's count when it gives one, else an
//! estimate from what was sent. Nothing is fetched or sent to make it.

use std::collections::BTreeMap;

#[derive(Default)]
struct Row {
    calls: usize,
    failed: usize,
    free_limit: usize,
    timeouts: usize,
    ms: Vec<u64>,
    first_ms: Vec<u64>,
    tokens: u64,
    /// Calls without a token count: their text, for an estimate.
    uncounted_chars: usize,
    local: bool,
}

fn median(v: &mut [u64]) -> Option<u64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    Some(v[v.len() / 2])
}

fn secs(ms: u64) -> String {
    format!("{:.1} s", ms as f64 / 1000.0)
}

/// The usage table for calls at or after `since` (unix seconds), one
/// provider a line, the busiest first.
pub fn report(egress: &str, since: i64, days: u32) -> String {
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    for line in egress.lines() {
        let Ok(e) = serde_json::from_str::<friday_intelligence::EgressEntry>(line) else {
            continue;
        };
        let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&e.ts) else {
            continue;
        };
        if ts.timestamp() < since {
            continue;
        }
        let r = rows.entry(e.provider.clone()).or_default();
        r.calls += 1;
        r.local |= e.local == Some(true);
        if e.outcome != "ok" {
            r.failed += 1;
            match e.error.as_deref() {
                Some("free_limit") => r.free_limit += 1,
                Some("timeout") => r.timeouts += 1,
                _ => {}
            }
        } else if let Some(ms) = e.ms {
            r.ms.push(ms);
        }
        if let Some(f) = e.first_ms {
            r.first_ms.push(f);
        }
        match e.tokens {
            Some(t) => r.tokens += t,
            None => r.uncounted_chars += e.text_chars,
        }
    }
    if rows.is_empty() {
        return format!("No calls to an AI provider in the last {days} day(s).");
    }
    let mut rows: Vec<(String, Row)> = rows.into_iter().collect();
    rows.sort_by(|a, b| b.1.calls.cmp(&a.1.calls).then(a.0.cmp(&b.0)));
    let mut out = vec![format!(
        "AI providers, last {days} day(s) (from this computer's log):"
    )];
    for (name, mut r) in rows {
        let mut parts = vec![format!(
            "{} call{}",
            r.calls,
            if r.calls == 1 { "" } else { "s" }
        )];
        if r.failed > 0 {
            let mut why = Vec::new();
            if r.free_limit > 0 {
                why.push(format!("{} free-limit", r.free_limit));
            }
            if r.timeouts > 0 {
                why.push(format!("{} timed out", r.timeouts));
            }
            parts.push(if why.is_empty() {
                format!("{} failed", r.failed)
            } else {
                format!("{} failed ({})", r.failed, why.join(", "))
            });
        }
        if let Some(f) = median(&mut r.first_ms) {
            parts.push(format!("first words in {}", secs(f)));
        }
        if let Some(m) = median(&mut r.ms) {
            parts.push(format!("answers in {}", secs(m)));
        }
        let estimated = r.uncounted_chars / 4;
        if r.tokens > 0 || estimated > 0 {
            parts.push(match (r.tokens, estimated) {
                (t, 0) => format!("{t} tokens"),
                (0, e) => format!("about {e} tokens sent"),
                (t, e) => format!("{t} tokens (+ about {e} sent uncounted)"),
            });
        }
        let label = if r.local {
            format!("{name} (on this computer)")
        } else {
            name
        };
        out.push(format!("  {label}: {}", parts.join(" · ")));
    }
    out.push("Times are medians; \"first words\" counts streamed answers only.".into());
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_provider_gets_its_calls_failures_times_and_tokens() {
        let log = [
            r#"{"ts":"2026-11-23T08:00:00Z","provider":"opencode","model":"m","kinds":["text"],"text_chars":400,"outcome":"ok","ms":3000,"first_ms":900}"#,
            r#"{"ts":"2026-11-23T08:01:00Z","provider":"opencode","model":"m","kinds":["text"],"text_chars":400,"outcome":"ok","ms":5000,"first_ms":1100}"#,
            r#"{"ts":"2026-11-23T08:02:00Z","provider":"opencode","model":"m","kinds":["text"],"text_chars":400,"outcome":"error","ms":800,"error":"free_limit"}"#,
            r#"{"ts":"2026-11-23T08:03:00Z","provider":"kilo","model":"k","kinds":["text"],"text_chars":100,"outcome":"ok","ms":2000,"tokens":250}"#,
            r#"{"ts":"2026-11-23T08:04:00Z","provider":"llamacpp","model":"g","kinds":["text"],"text_chars":80,"outcome":"error","error":"timeout","local":true}"#,
            r#"{"ts":"2026-11-01T08:00:00Z","provider":"old","model":"o","kinds":["text"],"text_chars":1,"outcome":"ok"}"#,
            "not json",
        ]
        .join("\n");
        let since = chrono::DateTime::parse_from_rfc3339("2026-11-20T00:00:00Z")
            .unwrap()
            .timestamp();
        let said = report(&log, since, 7);
        let lines: Vec<&str> = said.lines().collect();
        assert_eq!(
            lines[1],
            "  opencode: 3 calls · 1 failed (1 free-limit) · first words in 1.1 s · answers in 5.0 s · about 300 tokens sent"
        );
        assert_eq!(lines[2], "  kilo: 1 call · answers in 2.0 s · 250 tokens");
        assert_eq!(
            lines[3],
            "  llamacpp (on this computer): 1 call · 1 failed (1 timed out) · about 20 tokens sent"
        );
        assert!(!said.contains("old"), "{said}");
        assert_eq!(
            report("", since, 7),
            "No calls to an AI provider in the last 7 day(s)."
        );
    }
}
