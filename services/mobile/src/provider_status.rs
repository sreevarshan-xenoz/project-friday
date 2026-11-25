//! `friday-ctl providers`: how each AI provider is doing right now, in plain
//! words, and who answered last. Read from what the daemon already keeps next
//! to state.json: `fabric/provider_health.json` (failures in a row, circuit),
//! `fabric/health-changes.jsonl` (why health last changed) and `egress.jsonl`
//! (each request that left the computer). Nothing is sent anywhere.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
struct Health {
    name: String,
    #[serde(default)]
    circuit_state: String,
    #[serde(default)]
    consecutive_failures: u32,
}

#[derive(Deserialize)]
struct Change {
    provider: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    timestamp: String,
}

#[derive(Deserialize)]
struct Sent {
    #[serde(default)]
    provider: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    outcome: String,
    #[serde(default)]
    ts: String,
}

/// The name a person knows a provider by.
pub fn display_name(id: &str) -> String {
    match id {
        "opencode" => "OpenCode Zen",
        "opencode-cli" => "OpenCode (its app)",
        "kilo" => "Kilo",
        "ovh" => "OVHcloud",
        "apisfreellm" => "apisfreellm.com",
        "llamacpp" | "ollama" => "Local model",
        "gemini" => "Google Gemini",
        "groq" => "Groq",
        "nvidia-nim" => "NVIDIA NIM",
        "openrouter" => "OpenRouter",
        "cerebras" => "Cerebras",
        "runcrate" => "Runcrate",
        "axforge" => "AxForge",
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        "antigravity" => "Antigravity",
        "cline" => "Cline",
        "hermes" => "Hermes Agent",
        other => other,
    }
    .to_string()
}

/// A failure reason (`classify_failure_reason`) in words.
fn reason_words(reason: &str) -> &'static str {
    match reason {
        "rate_limit" | "free_allowance_used" | "quota" => "limit hit",
        "auth" | "unauthorized" => "key refused",
        "timeout" => "too slow",
        "unavailable" | "server" | "server_error" => "having trouble",
        _ => "failing",
    }
}

fn ago(ts: &str, now: DateTime<Utc>) -> Option<String> {
    let t = DateTime::parse_from_rfc3339(ts).ok()?.with_timezone(&Utc);
    let s = (now - t).num_seconds().max(0);
    Some(match s {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        _ => format!("{} days ago", s / 86_400),
    })
}

/// The report for the state directory (the one holding state.json).
pub fn report(state_dir: &Path, now: DateTime<Utc>) -> String {
    let fabric = state_dir.join("fabric");
    let health: Vec<Health> = std::fs::read_to_string(fabric.join("provider_health.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let changes: Vec<Change> = std::fs::read_to_string(fabric.join("health-changes.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let mut out = String::new();
    if health.is_empty() {
        out.push_str("No provider has been asked yet.\n");
    }
    for h in &health {
        let state = if h.consecutive_failures == 0 && h.circuit_state != "Open" {
            "OK".to_string()
        } else {
            let last = changes.iter().rev().find(|c| c.provider == h.name);
            let mut s = reason_words(last.map(|c| c.reason.as_str()).unwrap_or("")).to_string();
            if h.circuit_state == "Open" {
                s.push_str(", resting for now");
            }
            if let Some(when) = last.and_then(|c| ago(&c.timestamp, now)) {
                s.push_str(&format!(" ({when})"));
            }
            s
        };
        out.push_str(&format!("{:<20} {state}\n", display_name(&h.name)));
    }
    let last_ok = std::fs::read_to_string(state_dir.join("egress.jsonl"))
        .unwrap_or_default()
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Sent>(l).ok())
        .find(|s| s.outcome == "ok");
    match last_ok {
        Some(s) => out.push_str(&format!(
            "Last answer: {} · {}{}\n",
            display_name(&s.provider),
            s.model,
            ago(&s.ts, now)
                .map(|a| format!(" · {a}"))
                .unwrap_or_default()
        )),
        None => out.push_str("Last answer: none sent from this computer yet\n"),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(health: &str, changes: &str, egress: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("friday-provstatus-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join("fabric")).unwrap();
        std::fs::write(d.join("fabric/provider_health.json"), health).unwrap();
        std::fs::write(d.join("fabric/health-changes.jsonl"), changes).unwrap();
        std::fs::write(d.join("egress.jsonl"), egress).unwrap();
        d
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-20T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn says_which_provider_hit_its_limit_and_who_answered_last() {
        let d = dir(
            r#"[{"name":"opencode","circuit_state":"Closed","consecutive_failures":2},
                {"name":"kilo","circuit_state":"Closed","consecutive_failures":0}]"#,
            concat!(
                r#"{"provider":"opencode","reason":"timeout","timestamp":"2026-11-20T10:00:00Z"}"#,
                "\n",
                r#"{"provider":"opencode","reason":"rate_limit","timestamp":"2026-11-20T11:48:00Z"}"#,
                "\n"
            ),
            concat!(
                r#"{"provider":"kilo","model":"ernie:free","outcome":"ok","ts":"2026-11-20T11:57:00+00:00"}"#,
                "\n",
                r#"{"provider":"opencode","model":"big-pickle","outcome":"error","ts":"2026-11-20T11:58:00+00:00"}"#,
                "\n"
            ),
        );
        let r = report(&d, now());
        assert!(
            r.contains("OpenCode Zen         limit hit (12 min ago)"),
            "{r}"
        );
        assert!(r.contains("Kilo                 OK"), "{r}");
        assert!(
            r.contains("Last answer: Kilo · ernie:free · 3 min ago"),
            "{r}"
        );
    }

    #[test]
    fn an_open_circuit_is_resting_and_an_unknown_reason_is_failing() {
        let d = dir(
            r#"[{"name":"groq","circuit_state":"Open","consecutive_failures":5}]"#,
            "",
            "",
        );
        let r = report(&d, now());
        assert!(
            r.contains("Groq                 failing, resting for now"),
            "{r}"
        );
        assert!(r.contains("none sent from this computer yet"), "{r}");
    }

    #[test]
    fn nothing_recorded_yet_is_said_plainly() {
        let d = std::env::temp_dir().join(format!("friday-provstatus-{}", uuid::Uuid::new_v4()));
        let r = report(&d, now());
        assert!(r.starts_with("No provider has been asked yet."), "{r}");
    }
}
