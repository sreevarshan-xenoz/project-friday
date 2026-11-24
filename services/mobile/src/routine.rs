//! Routines: "save what FRIDAY just did on the desktop, run it again by
//! name". `friday routine save <name>` takes the last desktop task FRIDAY
//! carried out (its steps are in `actions.jsonl`) and keeps the steps that
//! changed something; `friday routine run <name>` does them again with no
//! model: the same typed actions, so a destructive one still isn't run
//! without asking, and the first step that fails stops it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One step: a desktop verb (`ui_…` for inside-of-apps ones) and its parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub verb: String,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Routine {
    pub name: String,
    /// What was asked when the steps were done.
    pub goal: String,
    pub steps: Vec<Step>,
}

/// A name that is safe as a file name: letters, digits, spaces, `-` and `_`,
/// up to 40 characters.
pub fn valid_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty()
        && n.chars().count() <= 40
        && n.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
}

fn key(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

/// Where routines are kept (`~/.local/share/friday/routines`).
pub fn dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("friday/routines")
}

/// What `save` made of the last task: the routine, and how many steps it
/// left out because they ask for confirmation each time.
#[derive(Debug, PartialEq)]
pub struct Recorded {
    pub steps: Vec<Step>,
    pub goal: String,
    pub left_out: usize,
}

/// Steps further apart than this (seconds) belong to different tasks, even
/// with the same goal text.
const TASK_GAP_SECS: i64 = 120;

fn secs(l: &Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(l["ts"].as_str()?)
        .ok()
        .map(|t| t.timestamp())
}

/// The last desktop task in `actions.jsonl` text: the trailing lines that
/// share the last goal, with no gap of two minutes between them. A
/// confirmed action is logged apart, as "(confirmed)", after the task that
/// asked for it: it belongs to that task, and is counted as left out.
/// Only steps that worked and changed something are kept (looking around
/// was the model finding its way). Left out, and counted: destructive
/// steps, and typed text (it can be private, and a routine is a plain file).
pub fn last_task(log: &str) -> Option<Recorded> {
    let lines: Vec<Value> = log
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect();
    let goal = lines
        .iter()
        .rev()
        .find_map(|l| l["goal"].as_str().filter(|g| *g != "(confirmed)"))?
        .to_string();
    let mut task: Vec<&Value> = Vec::new();
    let mut later: Option<i64> = None;
    for l in lines.iter().rev() {
        let g = l["goal"].as_str();
        if g != Some("(confirmed)") && g != Some(goal.as_str()) {
            break;
        }
        if let (Some(later), Some(t)) = (later, secs(l)) {
            if later - t > TASK_GAP_SECS {
                break;
            }
        }
        later = secs(l).or(later);
        task.push(l);
    }
    task.reverse();
    let (steps, left_out) = steps_of(&task)?;
    (!steps.is_empty()).then_some(Recorded {
        steps,
        goal,
        left_out,
    })
}

/// The steps of one task worth doing again, and how many were left out.
fn steps_of(task: &[&Value]) -> Option<(Vec<Step>, usize)> {
    let mut steps = Vec::new();
    let mut left_out = 0;
    for l in task {
        if l["ok"].as_bool() != Some(true) {
            continue;
        }
        let verb = l["verb"].as_str()?;
        match l["risk"].as_str() {
            Some("reversible") if verb == "ui_type_text" => left_out += 1,
            Some("reversible") => steps.push(Step {
                verb: verb.to_string(),
                params: l["params"].clone(),
            }),
            Some("destructive") => left_out += 1,
            _ => {}
        }
    }
    Some((steps, left_out))
}

/// How many times the same steps make a routine worth offering.
pub const OFFER_AFTER: usize = 3;

/// When the last desktop task did the very same steps (two or more) as
/// earlier ones, [`OFFER_AFTER`] times in all, and no saved routine
/// (`saved`) already does them: that task and the count, to offer saving
/// it. Only offered, never saved without the person.
pub fn repeated(log: &str, saved: &[Routine]) -> Option<(Recorded, usize)> {
    // The log's tasks, oldest first: a new goal or a two-minute gap starts one.
    let mut tasks: Vec<(String, Vec<Value>)> = Vec::new();
    let mut last_ts: Option<i64> = None;
    for l in log
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
    {
        let goal = l["goal"].as_str().unwrap_or_default().to_string();
        let gap = matches!((last_ts, secs(&l)), (Some(a), Some(b)) if b - a > TASK_GAP_SECS);
        last_ts = secs(&l).or(last_ts);
        match tasks.last_mut() {
            Some((g, lines)) if !gap && (goal == *g || goal == "(confirmed)") => lines.push(l),
            _ if goal == "(confirmed)" => {}
            _ => tasks.push((goal, vec![l])),
        }
    }
    let steps = |lines: &[Value]| {
        let refs: Vec<&Value> = lines.iter().collect();
        steps_of(&refs)
    };
    let (goal, lines) = tasks.last()?;
    let (last, left_out) = steps(lines)?;
    if last.len() < 2 || saved.iter().any(|r| r.steps == last) {
        return None;
    }
    let times = tasks
        .iter()
        .filter(|(_, l)| steps(l).is_some_and(|(s, _)| s == last))
        .count();
    (times >= OFFER_AFTER).then(|| {
        (
            Recorded {
                steps: last,
                goal: goal.clone(),
                left_out,
            },
            times,
        )
    })
}

/// The steps you took while `friday routine record` watched Hyprland's
/// events: apps opened (`openwindow`), windows moved (`movewindowv2`) and
/// workspaces switched to (`workspacev2`), given the windows already open
/// (address → app) when it began. Repeats collapse: the last of several
/// switches in a row, the last move of the same window.
pub fn from_events(events: &str, open: &[(String, String)]) -> Vec<Step> {
    use serde_json::json;
    let bare = |a: &str| a.trim().trim_start_matches("0x").to_lowercase();
    let mut apps: std::collections::HashMap<String, String> =
        open.iter().map(|(a, c)| (bare(a), c.clone())).collect();
    let mut steps: Vec<Step> = Vec::new();
    let mut launched = std::collections::HashSet::new();
    for line in events.lines() {
        let Some((event, data)) = line.split_once(">>") else {
            continue;
        };
        let f: Vec<&str> = data.splitn(4, ',').collect();
        let step = match (event, f.as_slice()) {
            ("openwindow", [addr, _ws, class, ..]) if !class.is_empty() => {
                apps.insert(bare(addr), class.to_string());
                // A second window of an app just opened (a dialog, a splash)
                // isn't a second launch.
                if !launched.insert(class.to_lowercase()) {
                    continue;
                }
                Step {
                    verb: "launch_app".into(),
                    params: json!({ "app": class.to_lowercase() }),
                }
            }
            ("movewindowv2", [addr, _id, name, ..]) if !name.starts_with("special") => {
                let Some(app) = apps.get(&bare(addr)) else {
                    continue;
                };
                Step {
                    verb: "move_window".into(),
                    params: json!({ "target": app, "workspace": name }),
                }
            }
            ("workspacev2", [_id, name, ..]) if !name.starts_with("special") => Step {
                verb: "switch_workspace".into(),
                params: json!({ "workspace": name }),
            },
            _ => continue,
        };
        // The last of several switches in a row; a window moved again
        // replaces its earlier move.
        if step.verb == "switch_workspace" && steps.last().is_some_and(|l| l.verb == step.verb) {
            steps.pop();
        }
        if step.verb == "move_window" {
            steps.retain(|l| {
                !(l.verb == "move_window" && l.params["target"] == step.params["target"])
            });
        }
        steps.push(step);
    }
    steps
}

pub fn save(dir: &Path, routine: &Routine) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join(format!("{}.json", key(&routine.name))),
        serde_json::to_string_pretty(routine)?,
    )
}

pub fn load(dir: &Path, name: &str) -> Option<Routine> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{}.json", key(name)))).ok()?)
        .ok()
}

pub fn list(dir: &Path) -> Vec<Routine> {
    let mut all: Vec<Routine> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok())
        .collect();
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all
}

pub fn remove(dir: &Path, name: &str) -> bool {
    std::fs::remove_file(dir.join(format!("{}.json", key(name)))).is_ok()
}

/// What running a step did.
#[async_trait::async_trait]
pub trait Runner {
    async fn run(&mut self, step: &Step) -> Result<(), String>;
}

/// Runs the steps in order; stops at the first failure and says which.
pub async fn replay(routine: &Routine, runner: &mut dyn Runner) -> Result<usize, String> {
    for (i, step) in routine.steps.iter().enumerate() {
        runner.run(step).await.map_err(|e| {
            format!(
                "stopped at step {} of {} ({}): {e}",
                i + 1,
                routine.steps.len(),
                step.verb
            )
        })?;
    }
    Ok(routine.steps.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn your_own_steps_become_a_routine() {
        let open = [("0xaaa1".to_string(), "kitty".to_string())];
        let events =
            "workspacev2>>2,2\nworkspacev2>>3,3\nopenwindow>>bbb2,3,firefox,Mozilla Firefox\nopenwindow>>ddd4,3,firefox,Save as\n\
            movewindowv2>>bbb2,1,1\nmovewindowv2>>bbb2,4,4\nmovewindowv2>>aaa1,2,2\n\
            activewindow>>kitty,zsh\nworkspacev2>>-98,special:magic\nmovewindowv2>>ccc3,1,1\n";
        let steps = from_events(events, &open);
        let shown: Vec<String> = steps
            .iter()
            .map(|s| format!("{} {}", s.verb, s.params))
            .collect();
        assert_eq!(
            shown,
            [
                r#"switch_workspace {"workspace":"3"}"#,
                r#"launch_app {"app":"firefox"}"#,
                r#"move_window {"target":"firefox","workspace":"4"}"#,
                r#"move_window {"target":"kitty","workspace":"2"}"#,
            ]
        );
    }
    use serde_json::json;

    fn line(verb: &str, risk: &str, ok: bool, goal: &str) -> String {
        at(0, verb, risk, ok, goal)
    }

    fn at(t: i64, verb: &str, risk: &str, ok: bool, goal: &str) -> String {
        let ts = chrono::DateTime::from_timestamp(1_700_000_000 + t, 0)
            .unwrap()
            .to_rfc3339();
        json!({"ts": ts, "verb": verb, "params": {"x": verb}, "risk": risk, "ok": ok, "goal": goal})
            .to_string()
    }

    #[test]
    fn the_last_task_keeps_what_changed_something_and_counts_the_destructive() {
        let log = [
            line("windows", "read", true, "old goal"),
            line("move_window", "reversible", true, "old goal"),
            line("ui_tree", "read", true, "set up my screen"),
            line("launch_app", "reversible", true, "set up my screen"),
            line("focus_window", "reversible", false, "set up my screen"),
            line("close_window", "destructive", true, "set up my screen"),
            line("switch_workspace", "reversible", true, "set up my screen"),
        ]
        .join("\n");
        let got = last_task(&log).unwrap();
        assert_eq!(got.goal, "set up my screen");
        let verbs: Vec<&str> = got.steps.iter().map(|s| s.verb.as_str()).collect();
        assert_eq!(verbs, ["launch_app", "switch_workspace"]);
        assert_eq!(got.left_out, 1);
        assert!(last_task("").is_none());
        // Only looking around: nothing to save.
        assert!(last_task(&line("windows", "read", true, "g")).is_none());
        // A lone confirmed action isn't a task.
        assert!(last_task(&line("close_window", "destructive", true, "(confirmed)")).is_none());
    }

    #[test]
    fn the_same_steps_a_third_time_are_offered_once_saved_never() {
        let task = |t: i64, goal: &str| {
            [
                at(t, "ui_tree", "read", true, goal),
                at(t + 5, "launch_app", "reversible", true, goal),
                at(t + 10, "switch_workspace", "reversible", true, goal),
            ]
            .join("\n")
        };
        let two = [task(0, "set up"), task(1000, "get my desk ready")].join("\n");
        assert!(repeated(&two, &[]).is_none(), "twice isn't yet");
        let three = [two.clone(), task(2000, "set up")].join("\n");
        let (rec, n) = repeated(&three, &[]).unwrap();
        assert_eq!(n, 3);
        assert_eq!(rec.goal, "set up");
        assert_eq!(rec.steps.len(), 2);
        // Something else since: the last task is what counts.
        let other = [
            three.clone(),
            at(3000, "move_window", "reversible", true, "x"),
        ]
        .join("\n");
        assert!(repeated(&other, &[]).is_none());
        // Already a routine: not offered.
        let saved = Routine {
            name: "desk".into(),
            goal: "set up".into(),
            steps: rec.steps.clone(),
        };
        assert!(repeated(&three, &[saved]).is_none());
        // One step isn't worth a routine.
        let one = (0..3)
            .map(|i| at(i * 1000, "launch_app", "reversible", true, "kitty"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(repeated(&one, &[]).is_none());
    }

    #[test]
    fn a_repeated_goal_is_a_new_task_after_a_pause() {
        let log = [
            at(0, "launch_app", "reversible", true, "open terminal"),
            at(30, "switch_workspace", "reversible", true, "open terminal"),
            // An hour later, the same words again.
            at(3600, "launch_app", "reversible", true, "open terminal"),
        ]
        .join("\n");
        assert_eq!(last_task(&log).unwrap().steps.len(), 1);
    }

    #[test]
    fn a_confirmed_step_belongs_to_its_task_and_typed_text_is_left_out() {
        let log = [
            at(0, "launch_app", "reversible", true, "tidy up"),
            at(5, "ui_type_text", "reversible", true, "tidy up"),
            at(10, "switch_workspace", "reversible", true, "tidy up"),
            at(60, "close_window", "destructive", true, "(confirmed)"),
        ]
        .join("\n");
        let got = last_task(&log).unwrap();
        let verbs: Vec<&str> = got.steps.iter().map(|s| s.verb.as_str()).collect();
        assert_eq!(verbs, ["launch_app", "switch_workspace"]);
        assert_eq!(got.goal, "tidy up");
        assert_eq!(got.left_out, 2);
    }

    #[test]
    fn routines_are_saved_listed_found_by_any_spelling_and_removed() {
        let d = std::env::temp_dir().join(format!("friday-routines-{}", uuid::Uuid::new_v4()));
        let r = Routine {
            name: "Morning Setup".into(),
            goal: "g".into(),
            steps: vec![Step {
                verb: "launch_app".into(),
                params: json!({"app": "firefox"}),
            }],
        };
        save(&d, &r).unwrap();
        assert_eq!(load(&d, "morning  setup").unwrap(), r);
        assert_eq!(list(&d).len(), 1);
        assert!(remove(&d, "MORNING SETUP"));
        assert!(load(&d, "Morning Setup").is_none());
        assert!(!remove(&d, "Morning Setup"));
    }

    #[test]
    fn names_are_plain_words() {
        assert!(valid_name("morning setup"));
        assert!(valid_name("work-2"));
        assert!(!valid_name(""));
        assert!(!valid_name("../x"));
        assert!(!valid_name(&"a".repeat(41)));
    }

    struct Fake(Vec<&'static str>, usize);
    #[async_trait::async_trait]
    impl Runner for Fake {
        async fn run(&mut self, step: &Step) -> Result<(), String> {
            self.1 += 1;
            if self.0.contains(&step.verb.as_str()) {
                Err("no such window".into())
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn replay_stops_at_the_first_failure_and_says_which() {
        let steps = ["a", "b", "c"]
            .map(|v| Step {
                verb: v.into(),
                params: json!({}),
            })
            .to_vec();
        let r = Routine {
            name: "n".into(),
            goal: "g".into(),
            steps,
        };
        let mut ok = Fake(vec![], 0);
        assert_eq!(replay(&r, &mut ok).await, Ok(3));
        let mut bad = Fake(vec!["b"], 0);
        assert_eq!(
            replay(&r, &mut bad).await,
            Err("stopped at step 2 of 3 (b): no such window".into())
        );
        assert_eq!(bad.1, 2, "step c must not run");
    }
}
