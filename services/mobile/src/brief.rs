//! "What did I miss?", the morning brief: what happened while the person
//! was away, from FRIDAY's own state only (nothing goes to a model). Coding
//! agents that finished, failed or wait for them; standing tasks that ran,
//! with the first line of their answer; reminders still to come today.
//! `friday-ctl brief` prints it (`--say` speaks it); asking "what did I
//! miss" answers with it on both platforms. Each brief starts where the last
//! one ended (`brief-seen`), at most a day back.

use std::path::{Path, PathBuf};

use chrono::{Local, TimeZone};

use crate::agent_runs::{Run, Status};
use crate::standing::{Store, Trigger};

/// The furthest back a brief looks.
const MAX_BACK: i64 = 24 * 3600;
/// Longest piece of an answer or a summary in one line.
const PIECE: usize = 140;

fn seen_file(state_dir: &Path) -> PathBuf {
    state_dir.join("brief-seen")
}

/// Where this brief starts: the last one, at most a day back.
pub fn since(state_dir: &Path, now: i64) -> i64 {
    std::fs::read_to_string(seen_file(state_dir))
        .ok()
        .and_then(|t| t.trim().parse::<i64>().ok())
        .filter(|&t| t <= now)
        .map_or(now - MAX_BACK, |t| t.max(now - MAX_BACK))
}

/// The next brief starts from `now`.
pub fn mark_seen(state_dir: &Path, now: i64) -> std::io::Result<()> {
    std::fs::write(seen_file(state_dir), now.to_string())
}

fn piece(text: &str) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= PIECE {
        return one;
    }
    let mut out: String = one.chars().take(PIECE).collect();
    out.push('…');
    out
}

fn clock(ts: i64) -> String {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map_or_else(String::new, |t| t.format("%H:%M").to_string())
}

/// The first line of the answer `answers.md` kept for `goal` at or after
/// `after` (a standing task's run), if any.
fn answer_for(answers: &str, goal: &str, after: i64) -> Option<String> {
    for block in answers.split("<!-- friday answer -->").skip(1) {
        let mut lines = block.lines().map(str::trim).filter(|l| !l.is_empty());
        if lines.next().and_then(|l| l.strip_prefix("# ")) != Some(goal) {
            continue;
        }
        let when = lines
            .next()
            .and_then(|l| l.strip_prefix('_')?.strip_suffix('_'))
            .and_then(|d| chrono::NaiveDateTime::parse_from_str(d, "%Y-%m-%d %H:%M").ok())
            .and_then(|d| Local.from_local_datetime(&d).single())?;
        // answers.md keeps minutes: a run at 08:00:40 is kept as 08:00.
        if when.timestamp() + 60 < after {
            continue;
        }
        if let Some(first) = lines.find(|l| {
            !l.starts_with('_') && !l.starts_with('>') && !l.starts_with('<') && *l != "---"
        }) {
            return Some(piece(first));
        }
    }
    None
}

/// The brief since `since`, one line each, plainly worded to be read or said.
pub fn text(state_dir: &Path, standing: &Path, since: i64, now: i64) -> String {
    let mut lines = Vec::new();

    let mut runs: Vec<Run> = crate::agent_runs::list(&state_dir.join("agents"));
    runs.sort_by_key(|r| r.ended.unwrap_or(r.started));
    for r in &runs {
        let who = r.agent.name();
        match r.status {
            Status::WaitingForYou => lines.push(format!(
                "{who} is waiting for you on \"{}\"{}.",
                piece(&r.request),
                r.pending
                    .as_ref()
                    .map(|p| format!(": {}", piece(&p.why)))
                    .unwrap_or_default()
            )),
            Status::Done if r.ended.is_some_and(|e| e >= since) => lines.push(format!(
                "{who} finished \"{}\"{}",
                piece(&r.request),
                if r.summary.trim().is_empty() {
                    ".".to_string()
                } else {
                    format!(": {}", piece(&r.summary))
                }
            )),
            Status::Failed if r.ended.is_some_and(|e| e >= since) => {
                lines.push(format!("{who} couldn't finish \"{}\".", piece(&r.request)))
            }
            _ => {}
        }
    }

    let store = Store::load(standing);
    let answers = std::fs::read_to_string(state_dir.join("answers.md")).unwrap_or_default();
    let mut ran: Vec<_> = store
        .tasks
        .iter()
        .filter(|t| t.last_run.is_some_and(|r| r >= since && r <= now))
        .collect();
    ran.sort_by_key(|t| t.last_run);
    for t in ran {
        let at = t.last_run.unwrap_or_default();
        lines.push(match answer_for(&answers, &t.goal, at) {
            Some(a) => format!("\"{}\" ran at {}: {a}", piece(&t.goal), clock(at)),
            None => format!("\"{}\" ran at {}.", piece(&t.goal), clock(at)),
        });
    }

    let end_of_day = Local
        .timestamp_opt(now, 0)
        .single()
        .and_then(|d| d.date_naive().and_hms_opt(23, 59, 59))
        .and_then(|d| Local.from_local_datetime(&d).single())
        .map_or(now, |d| d.timestamp());
    let mut later: Vec<(i64, &str)> = store
        .tasks
        .iter()
        .filter(|t| !t.paused)
        .filter_map(|t| match t.trigger {
            Trigger::Once { at } if at > now && at <= end_of_day => Some((at, t.goal.as_str())),
            _ => None,
        })
        .collect();
    later.sort();
    for (at, goal) in later {
        lines.push(format!("Reminder at {}: {}", clock(at), piece(goal)));
    }

    if lines.is_empty() {
        return format!("Nothing new since {}.", clock(since));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("friday-brief-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("agents")).unwrap();
        d
    }

    #[test]
    fn a_brief_says_what_finished_ran_and_is_still_to_come() {
        let d = dir("all");
        let now = Local
            .with_ymd_and_hms(2026, 11, 23, 9, 0, 0)
            .single()
            .unwrap()
            .timestamp();
        let run = |id: &str, status: &str, ended: i64, extra: &str| {
            format!(
                r#"{{"id":"{id}","agent":"opencode","folder":"/w","request":"fix the tests","task":"t","status":"{status}","started":{},"ended":{ended},"summary":"All 12 tests pass now."{extra}}}"#,
                ended - 60
            )
        };
        std::fs::write(d.join("agents/a.json"), run("a", "done", now - 3600, "")).unwrap();
        std::fs::write(
            d.join("agents/b.json"),
            run("b", "done", now - 3 * 86400, ""),
        )
        .unwrap();
        std::fs::write(
            d.join("agents/c.json"),
            run(
                "c",
                "waiting_for_you",
                now - 60,
                r#","pending":{"step":"rm -rf build","why":"it deletes the build folder"}"#,
            ),
        )
        .unwrap();
        let ran_at = now - 1800;
        let standing = d.join("standing.json");
        std::fs::write(
            &standing,
            format!(
                r#"{{"tasks":[
                {{"id":"t1","goal":"summarise the news","trigger":{{"kind":"daily","hour":8,"minute":30}},"created":0,"last_run":{ran_at},"runs":3}},
                {{"id":"t2","goal":"call the bank","trigger":{{"kind":"once","at":{}}},"created":0}},
                {{"id":"t3","goal":"tomorrow's thing","trigger":{{"kind":"once","at":{}}},"created":0}}]}}"#,
                now + 3600,
                now + 86400
            ),
        )
        .unwrap();
        std::fs::write(
            d.join("answers.md"),
            "<!-- friday answer -->\n# summarise the news\n_2026-11-23 08:30_\n\nRates held; markets up.\nMore lines.\n",
        )
        .unwrap();
        let said = text(&d, &standing, now - MAX_BACK, now);
        let lines: Vec<&str> = said.lines().collect();
        assert_eq!(
            lines,
            vec![
                "opencode finished \"fix the tests\": All 12 tests pass now.",
                "opencode is waiting for you on \"fix the tests\": it deletes the build folder.",
                "\"summarise the news\" ran at 08:30: Rates held; markets up.",
                "Reminder at 10:00: call the bank",
            ],
            "{said}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn each_brief_starts_where_the_last_ended_at_most_a_day_back() {
        let d = dir("since");
        assert_eq!(since(&d, 100_000), 100_000 - MAX_BACK);
        mark_seen(&d, 99_000).unwrap();
        assert_eq!(since(&d, 100_000), 99_000);
        assert_eq!(since(&d, 99_000 + 2 * MAX_BACK), 99_000 + MAX_BACK);
        let quiet = text(&d, &d.join("none.json"), 99_000, 100_000);
        assert!(quiet.starts_with("Nothing new since "), "{quiet}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
