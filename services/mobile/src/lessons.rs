//! Lessons from failed goals: what went wrong, and what worked afterwards.
//!
//! A goal that fails (a tool error, a refused action) leaves one line in
//! `lessons.jsonl` in FRIDAY's state directory. When a later goal succeeds on
//! a similar ask, the line also records what worked. The next similar ask
//! brings the line back into the `[memory]` block, so the model doesn't walk
//! into the same failure again. Provider and web-search errors aren't about
//! the goal and aren't kept. Text is masked, files are owner-only, lines
//! older than 30 days are dropped, and private mode writes nothing.

use std::path::Path;

use friday_goal_execution::{recall_score, shared_tokens, significant_tokens};
use serde::{Deserialize, Serialize};

/// How long a lesson is kept, in seconds.
const KEEP_SECS: i64 = 30 * 24 * 60 * 60;
/// How many lessons are kept.
const MAX: usize = 200;
/// How many lessons ride along with one ask.
const SHOWN: usize = 2;
/// Longest cause or fix kept, in characters.
const CAP: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Lesson {
    /// Unix seconds.
    pub ts: i64,
    pub goal: String,
    pub failed: String,
    /// What a later, similar goal did that worked.
    pub fixed: Option<String>,
    /// The project the ask named (its folder's name), when it named one: the
    /// lesson then comes back only for that project or asks naming none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

fn load(path: &Path, now: i64) -> Vec<Lesson> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Lesson>(l).ok())
        .filter(|l| (0..=KEEP_SECS).contains(&(now - l.ts)))
        .collect()
}

fn save(path: &Path, all: &[Lesson]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(&tmp)?;
    for l in all.iter().skip(all.len().saturating_sub(MAX)) {
        writeln!(f, "{}", serde_json::to_string(l).unwrap_or_default())?;
    }
    f.flush()?;
    drop(f);
    std::fs::rename(&tmp, path)
}

/// Two asks are alike when they share at least two significant words (one,
/// for a one-word ask) and at least half of the shorter ask's words.
fn alike(a: &str, b: &str) -> bool {
    let shorter = significant_tokens(a).len().min(significant_tokens(b).len());
    let shared = shared_tokens(a, b);
    shared >= shorter.min(2) && shared * 2 >= shorter && shared > 0
}

fn short(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty());
    let line = line.unwrap_or_default();
    let mut out: String = line.chars().take(CAP).collect();
    if line.chars().count() > CAP {
        out.push('…');
    }
    out
}

/// Learns from a finished goal: a failure is kept; a success closes the
/// newest open lesson for a similar ask. `goal` and `text` must already be
/// masked.
pub fn learn(
    path: &Path,
    now: i64,
    goal: &str,
    ok: bool,
    text: &str,
    project: Option<&str>,
) -> std::io::Result<()> {
    let text = short(text);
    if text.is_empty() {
        return Ok(());
    }
    let mut all = load(path, now);
    if ok {
        let open = all
            .iter_mut()
            .rev()
            .find(|l| l.fixed.is_none() && alike(goal, &l.goal) && l.project.as_deref() == project);
        match open {
            Some(l) => l.fixed = Some(text),
            None => return Ok(()),
        }
    } else if let Some(l) = all
        .iter_mut()
        .rev()
        .find(|l| l.fixed.is_none() && l.goal == goal && l.project.as_deref() == project)
    {
        l.ts = now;
        l.failed = text;
    } else {
        all.push(Lesson {
            ts: now,
            goal: short(goal),
            failed: text,
            fixed: None,
            project: project.map(String::from),
        });
    }
    save(path, &all)
}

/// Whether a lesson comes back for an ask about `project`: one kept for a
/// project goes only with that project (or an ask naming none). Learning is
/// stricter: a lesson is refreshed or closed only by an ask about its own
/// project (or, for one kept without, an ask naming none).
fn fits(lesson: &Lesson, project: Option<&str>) -> bool {
    match (lesson.project.as_deref(), project) {
        (Some(kept), Some(asked)) => kept == asked,
        _ => true,
    }
}

/// `[memory]` lines for the lessons that fit this ask, best first.
pub fn recall(path: &Path, now: i64, goal: &str, project: Option<&str>) -> Vec<String> {
    let mut hits: Vec<(usize, Lesson)> = load(path, now)
        .into_iter()
        .filter(|l| alike(goal, &l.goal) && fits(l, project))
        .map(|l| (recall_score(goal, &l.goal), l))
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.ts.cmp(&a.1.ts)));
    hits.into_iter()
        .take(SHOWN)
        .map(|(_, l)| match l.fixed {
            Some(fix) => format!(
                "lesson: an earlier ask \"{}\" failed: {}\nwhat worked afterwards: {fix}",
                l.goal, l.failed
            ),
            None => format!("lesson: an earlier ask \"{}\" failed: {}", l.goal, l.failed),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("friday-lessons-{}", uuid::Uuid::new_v4()))
            .join("lessons.jsonl")
    }

    #[test]
    fn a_failure_comes_back_for_a_similar_ask_and_not_for_another() {
        let p = tmp();
        learn(
            &p,
            1_000,
            "restart the nginx service",
            false,
            "systemctl: access denied",
            None,
        )
        .unwrap();
        let again = recall(&p, 2_000, "please restart nginx service now", None);
        assert_eq!(again.len(), 1, "{again:?}");
        assert!(again[0].contains("access denied"), "{again:?}");
        assert!(recall(&p, 2_000, "what is the weather in Paris", None).is_empty());
        // One shared word of a longer ask isn't alike.
        assert!(recall(&p, 2_000, "restart my laptop after the update", None).is_empty());
    }

    #[test]
    fn a_lesson_kept_for_one_project_stays_with_it() {
        let p = tmp();
        learn(
            &p,
            1_000,
            "run the tests for the site",
            false,
            "npm: missing script test",
            Some("site"),
        )
        .unwrap();
        assert_eq!(
            recall(&p, 2_000, "run the tests for the site", Some("site")).len(),
            1
        );
        assert_eq!(
            recall(&p, 2_000, "run the tests for the site", None).len(),
            1
        );
        assert!(recall(&p, 2_000, "run the tests for the site", Some("garden")).is_empty());
        // A success in another project, or in an ask naming none, doesn't close it.
        learn(
            &p,
            1_500,
            "run the tests for the site",
            true,
            "ok",
            Some("garden"),
        )
        .unwrap();
        learn(&p, 1_600, "run the tests for the site", true, "ok", None).unwrap();
        assert!(
            !recall(&p, 2_000, "run the tests for the site", Some("site"))[0]
                .contains("what worked")
        );
    }

    #[test]
    fn a_later_success_is_kept_as_what_worked() {
        let p = tmp();
        learn(
            &p,
            1_000,
            "restart the nginx service",
            false,
            "access denied",
            None,
        )
        .unwrap();
        learn(
            &p,
            1_500,
            "restart nginx service with sudo",
            true,
            "Restarted nginx.\nok",
            None,
        )
        .unwrap();
        let lines = recall(&p, 2_000, "restart the nginx service", None);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("access denied"), "{lines:?}");
        assert!(
            lines[0].contains("what worked afterwards: Restarted nginx."),
            "{lines:?}"
        );
        // The lesson is closed: another success doesn't overwrite it.
        learn(
            &p,
            1_800,
            "restart the nginx service",
            true,
            "something else",
            None,
        )
        .unwrap();
        assert!(!recall(&p, 2_000, "restart the nginx service", None)[0].contains("something else"));
    }

    #[test]
    fn a_success_with_no_failure_before_it_writes_nothing() {
        let p = tmp();
        learn(&p, 1_000, "restart the nginx service", true, "done", None).unwrap();
        assert!(!p.exists());
    }

    #[test]
    fn the_same_failing_ask_is_refreshed_not_repeated() {
        let p = tmp();
        learn(
            &p,
            1_000,
            "restart the nginx service",
            false,
            "access denied",
            None,
        )
        .unwrap();
        learn(
            &p,
            1_100,
            "restart the nginx service",
            false,
            "still denied",
            None,
        )
        .unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text.lines().count(), 1, "{text}");
        assert!(text.contains("still denied") && !text.contains("\"access denied\""));
    }

    #[test]
    fn lessons_are_owner_only_bounded_and_expire() {
        let p = tmp();
        learn(
            &p,
            0,
            "restart the nginx service",
            false,
            "access denied",
            None,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(recall(&p, KEEP_SECS + 1, "restart the nginx service", None).is_empty());
        // A new failure rewrites the file without the expired line.
        learn(
            &p,
            KEEP_SECS + 1,
            "mount the backup drive",
            false,
            "no such device",
            None,
        )
        .unwrap();
        assert!(!std::fs::read_to_string(&p).unwrap().contains("nginx"));
        // A long cause is cut.
        let long = "x".repeat(CAP * 3);
        learn(
            &p,
            KEEP_SECS + 2,
            "compile the kernel module",
            false,
            &long,
            None,
        )
        .unwrap();
        let line = recall(&p, KEEP_SECS + 3, "compile the kernel module", None).remove(0);
        assert!(line.chars().count() < CAP + 120, "{}", line.chars().count());
    }
}
