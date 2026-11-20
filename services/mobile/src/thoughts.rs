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
    /// idea, decision or question.
    pub kind: String,
    pub text: String,
}

pub const KINDS: &[&str] = &["idea", "decision", "question"];

pub fn file(state_dir: &Path) -> std::path::PathBuf {
    state_dir.join("thoughts.jsonl")
}

pub fn add(state_dir: &Path, thought: &Thought) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(state_dir)?;
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(file(state_dir))?;
    let mut t = thought.clone();
    t.text = t.text.trim().chars().take(MAX_CHARS).collect();
    writeln!(f, "{}", serde_json::to_string(&t).unwrap_or_default())
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

/// Whether `goal` names this project: its folder name, or a word of it
/// ("FRIDAY" names "project-friday").
fn named(goal: &str, project: &str) -> bool {
    let g = goal.to_lowercase();
    let words: Vec<&str> = g
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .collect();
    let name = project_name(project).to_lowercase();
    words.contains(&name.as_str())
        || name
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 3 && *w != "project")
            .any(|w| words.contains(&w))
}

/// The thoughts an ask should see, oldest first, at most `limit`: those of
/// the project it names, or, when it names none, the most recent ones.
pub fn recent(state_dir: &Path, goal: &str, limit: usize) -> Vec<Thought> {
    let all = load(state_dir);
    let of_project: Vec<Thought> = all
        .iter()
        .filter(|t| t.project.as_deref().is_some_and(|p| named(goal, p)))
        .cloned()
        .collect();
    let chosen = if of_project.is_empty() {
        all
    } else {
        of_project
    };
    let skip = chosen.len().saturating_sub(limit);
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
        let mine = recent(&d, "Friday, let's continue building FRIDAY", 10);
        assert_eq!(
            mine.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
            ["A", "C"]
        );
        let any = recent(&d, "where was I", 2);
        assert_eq!(
            any.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
            ["B", "C"]
        );
        let text = render(&mine);
        assert!(text.contains(" idea [project-friday]: A\n"), "{text}");
        assert!(text.contains(" decision [project-friday]: C\n"), "{text}");
    }
}
