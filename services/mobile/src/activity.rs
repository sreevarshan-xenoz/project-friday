//! One list of what FRIDAY is doing for the person: agent steps waiting for
//! their answer, runs going now, standing tasks, and runs finished lately.
//! Read from the files the daemon already keeps (`agents/` and the standing
//! task store); nothing is sent anywhere (`friday activity`, Settings).

use std::path::Path;

use crate::agent_runs::{Run, Status};
use crate::brief::{clock, piece};
use crate::standing::Store;

/// How long a finished run stays on the list.
const RECENT: i64 = 7 * 86_400;
/// Finished runs shown at most.
const RECENT_MAX: usize = 5;

/// The list, in sections; a section with nothing in it is left out.
pub fn report(agents_dir: &Path, standing: &Path, now: i64) -> String {
    let runs = crate::agent_runs::list(agents_dir);
    let store = Store::load(standing);
    let line = |r: &Run| format!("{} · \"{}\" · {}", r.agent.name(), piece(&r.request), r.id);
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();

    let waiting: Vec<String> = runs
        .iter()
        .filter(|r| r.status == Status::WaitingForYou)
        .map(|r| {
            let step = r
                .pending
                .as_ref()
                .map(|p| format!("\n  wants to: {}", piece(&p.step)))
                .unwrap_or_default();
            format!("{}{step}", line(r))
        })
        .collect();
    if !waiting.is_empty() {
        let how = if cfg!(windows) {
            "answer in its FRIDAY window"
        } else {
            "friday agents allow <id>, deny <id> or reply <id> \"…\""
        };
        sections.push((format!("Waiting for you ({how})"), waiting));
    }

    let going: Vec<String> = runs
        .iter()
        .filter(|r| matches!(r.status, Status::Running | Status::Queued))
        .map(|r| {
            let state = if r.status == Status::Queued {
                "queued".to_string()
            } else {
                format!("since {}", clock(r.started, now))
            };
            format!("{} · {state}", line(r))
        })
        .collect();
    if !going.is_empty() {
        sections.push(("Running".into(), going));
    }

    let tasks: Vec<String> = store
        .tasks
        .iter()
        .map(|t| {
            let mut s = format!("{} · {} · {}", piece(&t.goal), t.trigger.describe(), t.id);
            if t.paused {
                s.push_str(" · paused");
            }
            if let Some(last) = t.last_run {
                s.push_str(&format!(" · last ran {}", clock(last, now)));
            }
            s
        })
        .collect();
    if !tasks.is_empty() {
        sections.push(("Standing tasks".into(), tasks));
    }

    let mut finished: Vec<&Run> = runs
        .iter()
        .filter(|r| matches!(r.status, Status::Done | Status::Failed))
        .filter(|r| r.ended.unwrap_or(r.started) >= now - RECENT)
        .collect();
    finished.sort_by_key(|r| std::cmp::Reverse(r.ended.unwrap_or(r.started)));
    let finished: Vec<String> = finished
        .into_iter()
        .take(RECENT_MAX)
        .map(|r| {
            let how = if r.status == Status::Done {
                "done"
            } else {
                "stopped"
            };
            format!(
                "{} · {how} {}",
                line(r),
                clock(r.ended.unwrap_or(r.started), now)
            )
        })
        .collect();
    if !finished.is_empty() {
        sections.push(("Finished this week".into(), finished));
    }

    if sections.is_empty() {
        return "Nothing running and nothing waiting. Agent runs (\"have opencode add a README to ~/code/site\") and standing tasks (\"every morning at 8 tell me the weather\") show here.".into();
    }
    sections
        .into_iter()
        .map(|(title, items)| {
            let items: Vec<String> = items.into_iter().map(|i| format!("- {i}")).collect();
            format!("{title}\n{}", items.join("\n"))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runs::{Agent, Pending};
    use crate::standing::{Task, Trigger};

    fn run(id: &str, status: Status, started: i64, ended: Option<i64>) -> Run {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "agent": "opencode",
            "folder": "/tmp/site",
            "request": format!("task {id}"),
            "task": "t",
            "status": status,
            "started": started,
            "ended": ended,
        }))
        .unwrap()
    }

    fn dirs() -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "friday-activity-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        let agents = root.join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        (agents, root.join("standing.json"))
    }

    fn save(dir: &Path, r: &Run) {
        std::fs::write(
            dir.join(format!("{}.json", r.id)),
            serde_json::to_vec(r).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn nothing_yet_says_what_would_show() {
        let (agents, standing) = dirs();
        assert!(report(&agents, &standing, 1_000_000)
            .starts_with("Nothing running and nothing waiting."));
        let _ = std::fs::remove_dir_all(agents.parent().unwrap());
    }

    #[test]
    fn waiting_running_tasks_and_finished_each_get_a_section_in_that_order() {
        let (agents, standing) = dirs();
        let now = 2_000_000_000;
        let mut waiting = run("r1", Status::WaitingForYou, now - 600, None);
        waiting.pending = Some(Pending {
            step: "run npm install".into(),
            why: "installs packages".into(),
        });
        save(&agents, &waiting);
        save(&agents, &run("r2", Status::Running, now - 300, None));
        save(
            &agents,
            &run("r3", Status::Done, now - 9000, Some(now - 8000)),
        );
        // Finished long ago: not on the list.
        save(
            &agents,
            &run(
                "r4",
                Status::Failed,
                now - 30 * 86_400,
                Some(now - 30 * 86_400),
            ),
        );
        let store = Store {
            tasks: vec![Task {
                id: "t1".into(),
                goal: "tell me the weather".into(),
                trigger: Trigger::Every { minutes: 120 },
                paused: true,
                created: now - 86_400,
                last_run: None,
                runs: 0,
                armed: true,
            }],
            ..Default::default()
        };
        std::fs::write(&standing, serde_json::to_vec(&store).unwrap()).unwrap();

        let r = report(&agents, &standing, now);
        let at = |s: &str| r.find(s).unwrap_or_else(|| panic!("no {s:?} in:\n{r}"));
        assert!(at("Waiting for you") < at("Running") && at("Running") < at("Standing tasks"));
        assert!(at("Standing tasks") < at("Finished this week"));
        assert!(
            r.contains("opencode · \"task r1\" · r1\n  wants to: run npm install"),
            "{r}"
        );
        assert!(
            r.contains("tell me the weather · every 2 hours · t1 · paused"),
            "{r}"
        );
        assert!(r.contains("\"task r3\" · r3 · done"), "{r}");
        assert!(!r.contains("r4"), "old runs drop off: {r}");
        assert_eq!(Agent::Opencode.name(), "opencode");
        let _ = std::fs::remove_dir_all(agents.parent().unwrap());
    }
}
