//! Thought threads (vNext N1): "keep this idea: …", "we decided …", "open
//! question: …" are filed under the project you are working in, newest
//! last, in `thoughts.jsonl` in FRIDAY's state directory (owner-only). The
//! file is only ever appended to: a thought is never rewritten, so a thread
//! shows how the thinking changed. Asks about the past and about a project
//! get the thread back (`recent`).

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Longest thought kept, in characters.
const MAX_CHARS: usize = 600;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Thought {
    /// Unix seconds.
    pub ts: i64,
    /// The project folder it belongs to; `None` when none was in front.
    pub project: Option<String>,
    /// idea, decision, question or action (from a meeting).
    pub kind: String,
    pub text: String,
}

pub const KINDS: &[&str] = &["idea", "decision", "question", "action"];

pub fn file(state_dir: &Path) -> std::path::PathBuf {
    state_dir.join("thoughts.jsonl")
}

pub fn add(state_dir: &Path, thought: &Thought) -> std::io::Result<()> {
    std::fs::create_dir_all(state_dir)?;
    let mut t = thought.clone();
    t.text = t.text.trim().chars().take(MAX_CHARS).collect();
    crate::jsonl::append(
        &file(state_dir),
        &serde_json::to_string(&t).unwrap_or_default(),
    )
}

pub fn load(state_dir: &Path) -> Vec<Thought> {
    std::fs::read_to_string(file(state_dir))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// The project folder's name ("/home/me/code/project-friday" → "project-friday").
pub fn project_name(project: &str) -> &str {
    Path::new(project)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(project)
}

/// Whether `goal` names this project: its folder name, or a distinctive
/// word of it ("FRIDAY" names "project-friday"; "the" or "app" never count).
pub fn named(goal: &str, project: &str) -> bool {
    use friday_goal_execution::significant_tokens;
    let words = significant_tokens(goal);
    let name = project_name(project).to_lowercase();
    words.contains(&name)
        || significant_tokens(&name)
            .iter()
            .filter(|w| *w != "project" && *w != "code")
            .any(|w| words.contains(w))
}

/// How many thoughts an ask that names no known project sees.
const UNNAMED: usize = 5;

/// The thoughts an ask should see, oldest first: those of the project it
/// names (`project`: the folder's name from [`crate::project::resolve`]; at
/// most `limit`), none for a named project with none kept, or, when it
/// names none, only the last few.
pub fn recent(state_dir: &Path, goal: &str, limit: usize, project: Option<&str>) -> Vec<Thought> {
    let all = load(state_dir);
    let (chosen, keep) = match project {
        Some(name) => (
            all.into_iter()
                .filter(|t| t.project.as_deref().map(project_name) == Some(name))
                .collect(),
            limit,
        ),
        // A project the ask names but FRIDAY doesn't know gets nothing, not
        // another project's thoughts.
        None if crate::timeline::project_named(goal).is_some() => return Vec::new(),
        None => (all, UNNAMED.min(limit)),
    };
    let skip = chosen.len().saturating_sub(keep);
    chosen.into_iter().skip(skip).collect()
}

/// The thread as `[memory]` text: one line per thought, dated.
pub fn render(thoughts: &[Thought]) -> String {
    thoughts
        .iter()
        .map(|t| {
            let when = chrono::DateTime::from_timestamp(t.ts, 0)
                .map(|d| {
                    d.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d")
                        .to_string()
                })
                .unwrap_or_default();
            let project = t
                .project
                .as_deref()
                .map(|p| format!(" [{}]", project_name(p)))
                .unwrap_or_default();
            format!("{when} {}{project}: {}\n", t.kind, t.text)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("friday-thoughts-{}", uuid::Uuid::new_v4()))
    }

    fn t(ts: i64, project: Option<&str>, kind: &str, text: &str) -> Thought {
        Thought {
            ts,
            project: project.map(String::from),
            kind: kind.into(),
            text: text.into(),
        }
    }

    #[test]
    fn a_thread_is_appended_never_rewritten_and_owner_only() {
        let d = dir();
        add(
            &d,
            &t(
                1,
                Some("/c/project-friday"),
                "idea",
                "memory as an index, not a store",
            ),
        )
        .unwrap();
        add(
            &d,
            &t(
                2,
                Some("/c/project-friday"),
                "decision",
                "the index is computed from the logs",
            ),
        )
        .unwrap();
        add(&d, &t(3, Some("/c/garden"), "question", "which tomatoes?")).unwrap();
        let all = load(&d);
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].text, "memory as an index, not a store");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(file(&d)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn an_ask_naming_a_project_gets_its_thread() {
        let d = dir();
        add(&d, &t(1, Some("/c/project-friday"), "idea", "A")).unwrap();
        add(&d, &t(2, Some("/c/garden"), "question", "B")).unwrap();
        add(&d, &t(3, Some("/c/project-friday"), "decision", "C")).unwrap();
        let mine = recent(
            &d,
            "Friday, let's continue building FRIDAY",
            10,
            Some("project-friday"),
        );
        assert_eq!(
            mine.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
            ["A", "C"]
        );
        let any = recent(&d, "where was I", 2, None);
        assert_eq!(
            any.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
            ["B", "C"]
        );
        // Common words in a folder name don't make a question about it.
        assert!(!named("what's the weather in the app store", "/c/the-app"));
        assert!(named("continue building the garden", "/c/garden"));
        // An ask naming no project gets only the last few, not every
        // thought; a named project with none kept gets none of another's.
        for i in 0..10 {
            add(&d, &t(10 + i, Some("/c/garden"), "idea", "g")).unwrap();
        }
        assert_eq!(recent(&d, "what did we decide?", 15, None).len(), UNNAMED);
        assert!(recent(&d, "what did we decide about Atlas", 15, None).is_empty());
        assert!(recent(&d, "let's continue building Atlas", 15, None).is_empty());
        let text = render(&mine);
        assert!(text.contains(" idea [project-friday]: A\n"), "{text}");
        assert!(text.contains(" decision [project-friday]: C\n"), "{text}");
    }
}
