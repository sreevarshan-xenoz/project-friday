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
        let mut project = None;
        let mut failed = false;
        let answer = lines
            .find(|l| {
                if let Some(p) = l
                    .strip_prefix("_project: ")
                    .and_then(|p| p.strip_suffix('_'))
                {
                    project = Some(crate::thoughts::project_name(p).to_string());
                    return false;
                }
                if *l == "_failed_" {
                    failed = true;
                    return false;
                }
                !l.starts_with('>') && *l != "---" && !l.starts_with('<')
            })
            .unwrap_or_default();
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
            project,
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
            // The folder's name, not its path (it goes to the model).
            project.as_deref().unwrap_or("?"),
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
            project: l.project.clone(),
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

/// Words after "continue building" / "get back to" that aren't a project.
const NOT_A_PROJECT: [&str; 11] = [
    "where", "from", "it", "that", "this", "me", "you", "us", "him", "her", "them",
];

/// The rest of a name: without "the"/"my", and `None` when it isn't one.
fn name_in(rest: &str) -> Option<String> {
    let rest = rest.trim();
    let rest = rest
        .strip_prefix("the ")
        .or_else(|| rest.strip_prefix("my "))
        .unwrap_or(rest)
        .trim();
    let first = rest.split_whitespace().next()?;
    (!NOT_A_PROJECT.contains(&first) && rest.chars().count() <= 40).then(|| rest.to_string())
}

/// The project an ask picks back up, with the grammar `friday ask` restores
/// a workspace for: "Friday, let's continue building FRIDAY", "keep working
/// on the site", "get back to the planner", "prepare my desk for rust".
/// Only the first line counts (`friday ask` adds what it restored below).
fn resumed(goal: &str) -> Option<String> {
    let g = goal.lines().next().unwrap_or_default().to_lowercase();
    let g = g.trim().trim_end_matches(['.', '!', '?']);
    let g = g
        .strip_prefix("friday")
        .unwrap_or(g)
        .trim_start_matches([',', ' ']);
    let g = ["let's ", "lets "]
        .iter()
        .find_map(|p| g.strip_prefix(p))
        .unwrap_or(g);
    if let Some(rest) = g.strip_prefix("prepare my ") {
        return ["world for ", "workspace for ", "desk for ", "setup for "]
            .iter()
            .find_map(|p| rest.strip_prefix(p))
            .and_then(name_in);
    }
    // "continue", "resume" and "get back to" may name the project straight
    // away; "keep" needs "building"/"working on"/… ("keep calm" isn't one).
    let (rest, needs_connector) = if let Some(r) = g.strip_prefix("keep ") {
        (r, true)
    } else {
        (
            ["continue ", "resume ", "get back to "]
                .iter()
                .find_map(|v| g.strip_prefix(v))?,
            false,
        )
    };
    match ["building ", "working on ", "with ", "on "]
        .iter()
        .find_map(|c| rest.strip_prefix(c))
    {
        Some(after) => name_in(after),
        None if !needs_connector => name_in(rest),
        None => None,
    }
}

/// The project an ask names: a resume ("continue building X") or the end
/// of an ask about it ("what did I do on Atlas yesterday", "where are we on
/// the site", "what did we decide about the planner").
pub fn project_named(goal: &str) -> Option<String> {
    if let Some(name) = resumed(goal) {
        return Some(name);
    }
    let g = goal.lines().next().unwrap_or_default().to_lowercase();
    let (_, rest) = g.rsplit_once(" on ").or_else(|| g.rsplit_once(" about "))?;
    let mut rest = rest.trim_end_matches(['.', '!', '?']).to_string();
    for when in [
        "yesterday",
        "today",
        "this week",
        "last week",
        "past week",
        "so far",
    ] {
        rest = rest.replace(when, " ");
    }
    let name = name_in(&rest)?;
    let weekdays = [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ];
    (!weekdays.contains(&name.as_str())).then_some(name)
}

/// The entries of the project an ask names (`project`: its folder's name,
/// resolved once by [`crate::project::resolve`]), or all of them when it
/// names none. Desktop actions and lessons carry no project: those within
/// half an hour of the project's own entries stay with it. A named project
/// with no history of its own (or one FRIDAY doesn't know) gets nothing.
pub fn for_project(entries: Vec<Entry>, goal: &str, project: Option<&str>) -> Vec<Entry> {
    let Some(name) = project else {
        return if project_named(goal).is_some() {
            Vec::new()
        } else {
            entries
        };
    };
    let mine = |e: &Entry| e.project.as_deref() == Some(name);
    let named: Vec<i64> = entries.iter().filter(|e| mine(e)).map(|e| e.ts).collect();
    entries
        .into_iter()
        .filter(|e| {
            mine(e)
                || (!named.is_empty()
                    && e.project.is_none()
                    && named.iter().any(|t| e.ts.abs_diff(*t) <= 30 * 60))
        })
        .collect()
}

/// Which days an ask is about, or `None` when it isn't about the past.
/// "continue where we stopped" / "where was I" / "continue building X"
/// mean the last day with anything on it.
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
    ]) || resumed(goal).is_some();
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

/// A short summary of `day`, by project (`friday day`): how many things
/// happened in each, with the first and the latest. Empty when nothing did.
pub fn day_summary(entries: &[Entry], day: NaiveDate) -> String {
    let mut projects: Vec<(String, Vec<&Entry>)> = Vec::new();
    for e in entries {
        let Some(t) = Local.timestamp_opt(e.ts, 0).single() else {
            continue;
        };
        if t.date_naive() != day {
            continue;
        }
        let name = e.project.clone().unwrap_or_else(|| "Other".into());
        match projects.iter_mut().find(|(n, _)| *n == name) {
            Some((_, list)) => list.push(e),
            None => projects.push((name, vec![e])),
        }
    }
    if projects.is_empty() {
        return String::new();
    }
    // Projects first, by how much happened; "Other" last.
    projects.sort_by_key(|(n, list)| (n == "Other", std::cmp::Reverse(list.len())));
    let at = |e: &Entry| {
        Local
            .timestamp_opt(e.ts, 0)
            .single()
            .map(|t| t.format("%H:%M").to_string())
            .unwrap_or_default()
    };
    let mut out = format!("{} {}\n", day.weekday(), day.format("%Y-%m-%d"));
    for (name, list) in &projects {
        let n = list.len();
        out.push_str(&format!(
            "{name}: {n} {}\n",
            if n == 1 { "thing" } else { "things" }
        ));
        let first = list[0];
        out.push_str(&format!("  {} {}\n", at(first), first.text));
        if n > 1 {
            let last = list[n - 1];
            out.push_str(&format!("  {} {}\n", at(last), last.text));
        }
    }
    out
}

/// A past answer that matched a search.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// Unix seconds.
    pub ts: i64,
    pub question: String,
    /// The answer's line with the most of the words (or its first line).
    pub line: String,
}

const STOP: [&str; 16] = [
    "the", "a", "an", "my", "me", "of", "to", "and", "for", "in", "on", "with", "about", "is",
    "it", "that",
];

/// The words a search looks for: lower case, two letters or more, not "the".
pub fn search_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.')
        .map(|w| w.trim_matches('.'))
        .filter(|w| w.chars().count() >= 2 && !STOP.contains(w))
        .map(String::from)
        .collect()
}

/// Past answers (`answers.md` text, oldest first) holding every word, since
/// `since` (unix seconds), newest first, at most `limit`. With `keep_local`
/// false, answers kept on this computer are left out (for a cloud model).
pub fn search_answers(
    text: &str,
    words: &[String],
    since: i64,
    keep_local: bool,
    limit: usize,
) -> Vec<Found> {
    if words.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for block in text.split("<!-- friday answer -->").skip(1) {
        if !keep_local && block.contains(LOCAL_MARK) {
            continue;
        }
        let mut lines = block.lines().map(str::trim).filter(|l| !l.is_empty());
        let Some(question) = lines.next().and_then(|l| l.strip_prefix("# ")) else {
            continue;
        };
        let Some(ts) = lines
            .next()
            .and_then(|l| l.strip_prefix('_')?.strip_suffix('_'))
            .and_then(|d| NaiveDateTime::parse_from_str(d, "%Y-%m-%d %H:%M").ok())
            .and_then(|d| Local.from_local_datetime(&d).single())
            .map(|d| d.timestamp())
        else {
            continue;
        };
        if ts < since {
            continue;
        }
        let body: Vec<&str> = lines
            .filter(|l| {
                !l.starts_with('_') && !l.starts_with('>') && !l.starts_with('<') && *l != "---"
            })
            .collect();
        let all = format!("{question}\n{}", body.join("\n")).to_lowercase();
        if !words.iter().all(|w| all.contains(w.as_str())) {
            continue;
        }
        let hits = |l: &str| {
            let l = l.to_lowercase();
            words.iter().filter(|w| l.contains(w.as_str())).count()
        };
        // The first line with the most words.
        let line = body
            .iter()
            .enumerate()
            .max_by_key(|(i, l)| (hits(l), std::cmp::Reverse(*i)))
            .map(|(_, l)| *l)
            .unwrap_or_default();
        out.push(Found {
            ts,
            question: question.to_string(),
            line: short(line),
        });
    }
    out.reverse();
    out.truncate(limit);
    out
}

/// "What did you tell me about X last week": X and since when (unix
/// seconds; 0 for any time). `None` when the ask isn't that.
pub fn told_about(goal: &str, now: DateTime<Local>) -> Option<(String, i64)> {
    let g = goal.lines().next()?.trim().to_lowercase();
    let g = g.trim_end_matches(['?', '.', '!', ' ']);
    let rest = [
        "what did you tell me about ",
        "what did you say about ",
        "what have you told me about ",
        "what did friday tell me about ",
        "what did friday say about ",
        "what did you tell me on ",
        "what did we talk about ",
        // A call's notes (friday meeting): "what did we decide in the 3pm call".
        "what did we decide in ",
        "what did we decide on ",
        "what did we decide at ",
        "what did we decide about ",
        "what was decided in ",
        "what was decided at ",
    ]
    .iter()
    .find_map(|p| g.strip_prefix(p))?;
    let midnight = |d: NaiveDate| {
        Local
            .from_local_datetime(&d.and_hms_opt(0, 0, 0)?)
            .earliest()
            .map(|t| t.timestamp())
    };
    let today = now.date_naive();
    let (topic, since) = if let Some(t) = rest
        .strip_suffix(" last week")
        .or_else(|| rest.strip_suffix(" this week"))
    {
        (t, (now - Duration::days(7)).timestamp())
    } else if let Some(t) = rest.strip_suffix(" yesterday") {
        (t, midnight(today.pred_opt()?)?)
    } else if let Some(t) = rest.strip_suffix(" today") {
        (t, midnight(today)?)
    } else if let Some(t) = rest
        .strip_suffix(" last month")
        .or_else(|| rest.strip_suffix(" this month"))
    {
        (t, (now - Duration::days(30)).timestamp())
    } else {
        (rest, 0)
    };
    let topic = topic.trim();
    (!search_words(topic).is_empty()).then(|| (topic.to_string(), since))
}

/// The hits as a list with their dates, for a person or a model.
pub fn render_found(found: &[Found]) -> String {
    found
        .iter()
        .filter_map(|f| {
            let t = Local.timestamp_opt(f.ts, 0).single()?;
            Some(format!(
                "{} — \"{}\"\n    {}",
                t.format("%a %Y-%m-%d %H:%M"),
                f.question,
                f.line
            ))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn past_answers_are_found_by_their_words_with_dates() {
        let text = "<!-- friday answer -->\n# how do rust lifetimes work\n\n_2026-11-17 09:00_\n\nA lifetime says how long a borrow lives.\nRust checks them at compile time.\n\n---\n\n\
                    <!-- friday answer -->\n# summarise my bank statement\n\n_2026-11-18 10:00_\n\n_On this computer · nothing was sent_\n\nRust money spent: 40.\n\n---\n\n\
                    <!-- friday answer -->\n# why is cargo slow\n\n_2026-11-20 11:00_\n\n> (a follow-up)\n\nIt rebuilds; rust lifetimes aren't the cause.\n\n---\n\n";
        let words = search_words("Rust lifetimes");
        assert_eq!(words, ["rust", "lifetimes"]);
        let found = search_answers(text, &words, 0, true, 10);
        let qs: Vec<&str> = found.iter().map(|f| f.question.as_str()).collect();
        assert_eq!(
            qs,
            ["why is cargo slow", "how do rust lifetimes work"],
            "newest first"
        );
        assert_eq!(found[1].line, "Rust checks them at compile time.");
        // Since a day: only the newer one.
        let since = at("2026-11-19", "00:00").timestamp();
        assert_eq!(search_answers(text, &words, since, true, 10).len(), 1);
        // A local answer: in the person's own view, never for a model.
        let rust = search_words("rust");
        assert_eq!(search_answers(text, &rust, 0, true, 10).len(), 3);
        assert_eq!(search_answers(text, &rust, 0, false, 10).len(), 2);
        assert!(render_found(&found).contains("2026-11-20 11:00 — \"why is cargo slow\""));
        assert!(search_answers(text, &[], 0, true, 10).is_empty());
    }

    #[test]
    fn what_did_you_tell_me_about_names_a_topic_and_a_time() {
        let now = at("2026-11-24", "15:00");
        let (topic, since) =
            told_about("What did you tell me about Rust lifetimes last week?", now).unwrap();
        assert_eq!(topic, "rust lifetimes");
        assert_eq!(since, (now - Duration::days(7)).timestamp());
        let (_, since) = told_about("what did you say about docker yesterday", now).unwrap();
        assert_eq!(since, at("2026-11-23", "00:00").timestamp());
        assert_eq!(told_about("what did you tell me about it", now), None);
        assert_eq!(told_about("what is rust", now), None);
        assert_eq!(
            told_about("what did we talk about the planner", now)
                .unwrap()
                .1,
            0
        );
        let (topic, _) = told_about("What did we decide in the 3pm call?", now).unwrap();
        assert_eq!(search_words(&topic), ["3pm", "call"]);
        let notes = "<!-- friday answer -->\n# Notes from the 3pm call (15:02 to 15:40)\n_2026-11-25 15:02_\n\n- Decision at 15:05:12: Ship on Friday\n---\n";
        let found = search_answers(notes, &search_words(&topic), 0, true, 5);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, "- Decision at 15:05:12: Ship on Friday");
    }

    #[test]
    fn a_day_is_summed_up_by_project() {
        let e = |day: &str, hm: &str, text: &str, project: Option<&str>| Entry {
            ts: at(day, hm).timestamp(),
            text: text.into(),
            project: project.map(String::from),
        };
        let entries = vec![
            e("2026-11-19", "09:00", "the day before", Some("site")),
            e("2026-11-20", "09:00", "moved kitty", None),
            e("2026-11-20", "10:00", "why does cargo fail", Some("friday")),
            e(
                "2026-11-20",
                "11:00",
                "agent fixed the test",
                Some("friday"),
            ),
            e("2026-11-20", "12:00", "explain the planner", Some("friday")),
            e("2026-11-20", "13:00", "fix the footer", Some("site")),
        ];
        let day = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
        let s = day_summary(&entries, day);
        assert!(s.starts_with("Fri 2026-11-20\n"), "{s}");
        assert!(
            s.contains(
                "friday: 3 things\n  10:00 why does cargo fail\n  12:00 explain the planner\n"
            ),
            "{s}"
        );
        assert!(s.contains("site: 1 thing\n  13:00 fix the footer\n"), "{s}");
        assert!(s.ends_with("Other: 1 thing\n  09:00 moved kitty\n"), "{s}");
        assert!(!s.contains("the day before"), "{s}");
        let none = NaiveDate::from_ymd_opt(2026, 11, 21).unwrap();
        assert_eq!(day_summary(&entries, none), "");
    }

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
             <!-- friday answer -->\n# summarise my bank statement\n\n_2026-11-20 10:30_\n\n_On this computer · nothing was sent_\n\nYou spent…\n\n---\n\n\
             <!-- friday answer -->\n# restart nosuch.service\n\n_2026-11-20 10:40_\n\n_failed_\n\nCouldn't restart it.\n\n---\n\n",
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
        assert_eq!(all.len(), 4, "{all:?}");
        let day = at("2026-11-20", "00:00").date_naive();
        let text = render(&all, day, day);
        assert!(
            text.contains("10:40 asked \"restart nosuch.service\" (failed) → Couldn't restart it."),
            "{text}"
        );
        assert!(text.starts_with("Fri 2026-11-20:\n10:05 asked \"why does cargo build fail\" → The `cfg` value is missing."), "{text}");
        assert!(
            text.contains("11:00 did move_window {\"target\":\"kitty\"}, failed"),
            "{text}"
        );
        assert!(
            text.contains("12:00 opencode worked on \"add a README\" in site (done) [site]"),
            "{text}"
        );
        assert!(
            !text.contains("bank"),
            "a local answer never goes in: {text}"
        );
    }

    #[test]
    fn an_ask_naming_a_project_keeps_its_history() {
        let d = dir();
        std::fs::write(
            d.join("answers.md"),
            "<!-- friday answer -->\n# fix the build\n\n_2026-11-20 10:05_\n\n_project: /home/me/code/project-friday_\n\nAdded the missing cfg.\n\n---\n\n\
             <!-- friday answer -->\n# what's for dinner\n\n_2026-11-20 19:00_\n\nPasta.\n\n---\n\n",
        )
        .unwrap();
        let all = collect(&d);
        assert_eq!(all[0].project.as_deref(), Some("project-friday"));
        assert!(
            all[0].text.ends_with("→ Added the missing cfg."),
            "{:?}",
            all[0]
        );
        let friday = for_project(
            all.clone(),
            "let's continue building FRIDAY",
            Some("project-friday"),
        );
        assert_eq!(friday.len(), 1);
        assert_eq!(for_project(all, "where was I", None).len(), 2);
    }

    #[test]
    fn a_project_keeps_its_actions_and_never_borrows_another_projects() {
        let t = |hm: &str| at("2026-11-20", hm).timestamp();
        let e = |hm: &str, text: &str, project: Option<&str>| Entry {
            ts: t(hm),
            text: text.into(),
            project: project.map(String::from),
        };
        let all = vec![
            e("10:00", "asked about the build", Some("project-friday")),
            e("10:20", "did move_window", None),
            e("10:25", "lesson: restart failed", None),
            e("13:00", "asked about the budget", Some("money")),
            e("19:00", "asked what's for dinner", None),
        ];
        let texts = |v: Vec<Entry>| v.into_iter().map(|e| e.text).collect::<Vec<_>>();
        assert_eq!(
            texts(for_project(
                all.clone(),
                "continue building FRIDAY",
                Some("project-friday")
            )),
            [
                "asked about the build",
                "did move_window",
                "lesson: restart failed"
            ]
        );
        // Atlas has no history: nothing, not FRIDAY's actions or dinner.
        assert!(for_project(all.clone(), "let's keep working on Atlas", None).is_empty());
        assert!(for_project(all.clone(), "what did I do on Atlas yesterday?", None).is_empty());
        assert_eq!(for_project(all.clone(), "where was I", None).len(), 5);
        assert_eq!(
            for_project(all.clone(), "what did I do on monday", None).len(),
            5
        );
        assert_eq!(
            for_project(all.clone(), "continue where we stopped", None).len(),
            5
        );
        // A known project with nothing of its own: nothing, not the actions
        // that happened near another project's entries.
        assert!(for_project(all, "continue building money", Some("garden")).is_empty());
    }

    /// The same table `friday ask` is checked against (checks/cli.sh).
    #[test]
    fn resume_asks_follow_the_shared_table() {
        let table = include_str!("../../../tests/fixtures/resume-phrases.tsv");
        for line in table
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let (ask, want) = line.split_once('\t').expect("ask<TAB>project");
            let want = (!want.is_empty()).then(|| want.to_string());
            assert_eq!(resumed(ask), want, "{ask}");
        }
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
        for ask in [
            "Friday, let's continue building FRIDAY",
            "keep working on the site",
            "get back to the garden planner",
            "continue with the site.",
            "prepare my desk for rust",
        ] {
            assert_eq!(
                wants(ask, &entries, now),
                Some((d("2026-11-19"), d("2026-11-19"))),
                "{ask}"
            );
        }
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
        for ask in [
            "get back to me on the budget",
            "can you keep building on this idea",
            "keep calm",
        ] {
            assert_eq!(wants(ask, &entries, now), None, "{ask}");
        }
        assert_eq!(
            wants("where was I", &[], now),
            None,
            "nothing to go back to"
        );
    }

    #[test]
    fn a_lesson_on_the_timeline_keeps_its_project() {
        let e =
            lessons(r#"{"ts":10,"goal":"run the tests","failed":"no script","project":"garden"}"#);
        assert_eq!(e[0].project.as_deref(), Some("garden"));
        // So it never rides along with another project's entries.
        let near = vec![
            Entry {
                ts: 0,
                text: "asked".into(),
                project: Some("project-friday".into()),
            },
            e[0].clone(),
        ];
        assert_eq!(
            for_project(near, "continue building FRIDAY", Some("project-friday")).len(),
            1
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
