//! "What was I doing yesterday?", "continue where we stopped": one timeline
//! over what FRIDAY already keeps in its state directory. Questions and
//! answers (`answers.md`), desktop actions (`actions.jsonl`), coding-agent
//! runs (`agents/*.json`, with the project folder they ran in) and lessons
//! from failures (`lessons.jsonl`), by day, newest last. Nothing new is
//! recorded for it.
//!
//! Answers kept on this computer (a private window, screen recall) are left
//! out, so the timeline never carries them to a cloud model; so is screen
//! recall itself. The text is masked before it goes anywhere.

use std::path::Path;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone};

/// The line `friday ask` writes into an answer kept on this computer.
pub const LOCAL_MARK: &str = "_On this computer · nothing was sent_";
/// Most lines of timeline passed to the model.
const MAX_LINES: usize = 60;
/// Longest text of one line, in characters.
const LINE_CHARS: usize = 160;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Unix seconds.
    pub ts: i64,
    pub text: String,
    /// The folder it happened in, when known.
    pub project: Option<String>,
}

fn short(text: &str) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= LINE_CHARS {
        return one;
    }
    let mut out: String = one.chars().take(LINE_CHARS).collect();
    out.push('…');
    out
}

fn answers(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    for block in text.split("<!-- friday answer -->").skip(1) {
        if block.contains(LOCAL_MARK) {
            continue;
        }
        let mut lines = block.lines().map(str::trim).filter(|l| !l.is_empty());
        let Some(goal) = lines.next().and_then(|l| l.strip_prefix("# ")) else {
            continue;
        };
        let Some(when) = lines
            .next()
            .and_then(|l| l.strip_prefix('_')?.strip_suffix('_'))
            .and_then(|d| NaiveDateTime::parse_from_str(d, "%Y-%m-%d %H:%M").ok())
            .and_then(|d| Local.from_local_datetime(&d).single())
        else {
            continue;
        };
        let answer = lines
            .find(|l| !l.starts_with('>') && *l != "---" && !l.starts_with('<'))
            .unwrap_or_default();
        let failed = goal.starts_with("FRIDAY ✗");
        out.push(Entry {
            ts: when.timestamp(),
            text: short(&format!(
                "asked \"{}\"{}{}",
                goal,
                if failed { " (failed)" } else { "" },
                if answer.is_empty() {
                    String::new()
                } else {
                    format!(" → {answer}")
                }
            )),
            project: None,
        });
    }
    out
}

fn actions(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| {
            let ts = DateTime::parse_from_rfc3339(v["ts"].as_str()?).ok()?;
            let ok = v["ok"].as_bool().unwrap_or(true);
            Some(Entry {
                ts: ts.timestamp(),
                text: short(&format!(
                    "did {} {}{} (for \"{}\")",
                    v["verb"].as_str().unwrap_or("?"),
                    v["params"],
                    if ok { "" } else { ", failed" },
                    v["goal"].as_str().unwrap_or_default()
                )),
                project: None,
            })
        })
        .collect()
}

fn agent_run(text: &str) -> Option<Entry> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let folder = v["folder"].as_str().unwrap_or_default();
    let project = Path::new(folder)
        .file_name()
        .map(|n| n.to_string_lossy().to_string());
    Some(Entry {
        ts: v["started"].as_i64()?,
        text: short(&format!(
            "{} worked on \"{}\" in {} ({})",
            v["agent"].as_str().unwrap_or("an agent"),
            v["request"].as_str().unwrap_or_default(),
            folder,
            v["status"].as_str().unwrap_or("?")
        )),
        project,
    })
}

fn lessons(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|l| serde_json::from_str::<crate::lessons::Lesson>(l).ok())
        .map(|l| Entry {
            ts: l.ts,
            text: short(&format!(
                "\"{}\" failed: {}{}",
                l.goal,
                l.failed,
                l.fixed
                    .map(|f| format!("; later worked: {f}"))
                    .unwrap_or_default()
            )),
            project: None,
        })
        .collect()
}

/// Everything in `state_dir`, oldest first.
pub fn collect(state_dir: &Path) -> Vec<Entry> {
    let read = |name: &str| std::fs::read_to_string(state_dir.join(name)).unwrap_or_default();
    let mut all = answers(&read("answers.md"));
    all.extend(actions(&read("actions.jsonl")));
    all.extend(lessons(&read("lessons.jsonl")));
    if let Ok(dir) = std::fs::read_dir(state_dir.join("agents")) {
        for f in dir.flatten() {
            if f.path().extension().is_some_and(|e| e == "json") {
                all.extend(agent_run(
                    &std::fs::read_to_string(f.path()).unwrap_or_default(),
                ));
            }
        }
    }
    all.sort_by_key(|e| e.ts);
    all
}

/// Which days an ask is about, or `None` when it isn't about the past.
/// "continue where we stopped" / "where was I" mean the last day with
/// anything on it.
pub fn wants(
    goal: &str,
    entries: &[Entry],
    now: DateTime<Local>,
) -> Option<(NaiveDate, NaiveDate)> {
    let g = goal.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| g.contains(w));
    let today = now.date_naive();
    let about_past = has(&[
        "where we stopped",
        "where we left off",
        "where i left off",
        "where i stopped",
        "where was i",
        "pick up where",
        "what was i doing",
        "what was i working on",
        "what did i do",
        "what did we do",
        "what have i done",
    ]);
    if !about_past {
        return None;
    }
    if has(&["yesterday"]) {
        let d = today - Duration::days(1);
        return Some((d, d));
    }
    if has(&["this week", "last week", "past week"]) {
        return Some((today - Duration::days(6), today));
    }
    if has(&["today", "so far"]) {
        return Some((today, today));
    }
    let last = entries
        .iter()
        .rev()
        .find(|e| e.ts <= now.timestamp())
        .and_then(|e| Local.timestamp_opt(e.ts, 0).single())?
        .date_naive();
    Some((last, last))
}

/// The timeline of those days, by day, newest last, at most [`MAX_LINES`].
pub fn render(entries: &[Entry], from: NaiveDate, to: NaiveDate) -> String {
    let lines: Vec<(NaiveDate, String)> = entries
        .iter()
        .filter_map(|e| {
            let t = Local.timestamp_opt(e.ts, 0).single()?;
            let d = t.date_naive();
            (from <= d && d <= to).then(|| {
                let project = e
                    .project
                    .as_ref()
                    .map(|p| format!(" [{p}]"))
                    .unwrap_or_default();
                (d, format!("{} {}{project}", t.format("%H:%M"), e.text))
            })
        })
        .collect();
    let skip = lines.len().saturating_sub(MAX_LINES);
    let mut out = String::new();
    let mut day = None;
    for (d, line) in lines.into_iter().skip(skip) {
        if day != Some(d) {
            out.push_str(&format!("{} {}:\n", d.weekday(), d.format("%Y-%m-%d")));
            day = Some(d);
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("friday-timeline-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join("agents")).unwrap();
        d
    }

    fn at(day: &str, hm: &str) -> DateTime<Local> {
        let d = NaiveDateTime::parse_from_str(&format!("{day} {hm}"), "%Y-%m-%d %H:%M").unwrap();
        Local.from_local_datetime(&d).single().unwrap()
    }

    #[test]
    fn the_logs_become_one_timeline_without_local_answers() {
        let d = dir();
        std::fs::write(
            d.join("answers.md"),
            "<!-- friday answer -->\n# why does cargo build fail\n\n_2026-11-20 10:05_\n\n> error[E0425]\n\nThe `cfg` value is missing.\n\n---\n\n\
             <!-- friday answer -->\n# summarise my bank statement\n\n_2026-11-20 10:30_\n\n_On this computer · nothing was sent_\n\nYou spent…\n\n---\n\n",
        )
        .unwrap();
        let ts = at("2026-11-20", "11:00").to_rfc3339();
        std::fs::write(
            d.join("actions.jsonl"),
            format!("{{\"ts\":\"{ts}\",\"verb\":\"move_window\",\"params\":{{\"target\":\"kitty\"}},\"risk\":\"reversible\",\"ok\":false,\"goal\":\"put kitty on 2\"}}\n"),
        )
        .unwrap();
        std::fs::write(
            d.join("agents/r1.json"),
            format!(
                "{{\"id\":\"r1\",\"agent\":\"opencode\",\"folder\":\"/home/me/code/site\",\"request\":\"add a README\",\"task\":\"t\",\"status\":\"done\",\"started\":{}}}",
                at("2026-11-20", "12:00").timestamp()
            ),
        )
        .unwrap();
        let all = collect(&d);
        assert_eq!(all.len(), 3, "{all:?}");
        let day = at("2026-11-20", "00:00").date_naive();
        let text = render(&all, day, day);
        assert!(text.starts_with("Fri 2026-11-20:\n10:05 asked \"why does cargo build fail\" → The `cfg` value is missing."), "{text}");
        assert!(
            text.contains("11:00 did move_window {\"target\":\"kitty\"}, failed"),
            "{text}"
        );
        assert!(
            text.contains(
                "12:00 opencode worked on \"add a README\" in /home/me/code/site (done) [site]"
            ),
            "{text}"
        );
        assert!(
            !text.contains("bank"),
            "a local answer never goes in: {text}"
        );
    }

    #[test]
    fn asks_about_the_past_pick_their_days() {
        let now = at("2026-11-21", "09:00");
        let entries = vec![Entry {
            ts: at("2026-11-19", "18:00").timestamp(),
            text: "x".into(),
            project: None,
        }];
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(
            wants("Continue where we stopped", &entries, now),
            Some((d("2026-11-19"), d("2026-11-19")))
        );
        assert_eq!(
            wants("what was I doing yesterday?", &entries, now),
            Some((d("2026-11-20"), d("2026-11-20")))
        );
        assert_eq!(
            wants("what did I do this week", &entries, now),
            Some((d("2026-11-15"), d("2026-11-21")))
        );
        assert_eq!(
            wants("what's the weather yesterday in Paris", &entries, now),
            None
        );
        assert_eq!(
            wants("where was I", &[], now),
            None,
            "nothing to go back to"
        );
    }

    #[test]
    fn a_long_day_keeps_its_last_lines() {
        let base = at("2026-11-20", "08:00").timestamp();
        let entries: Vec<Entry> = (0..100)
            .map(|i| Entry {
                ts: base + i * 60,
                text: format!("step {i}"),
                project: None,
            })
            .collect();
        let day = at("2026-11-20", "00:00").date_naive();
        let text = render(&entries, day, day);
        assert_eq!(text.lines().count(), MAX_LINES + 1);
        assert!(text.contains("step 99") && !text.contains("step 39\n"));
    }
}
