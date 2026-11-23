//! What FRIDAY may do on its own, in one table: each tool action is safe
//! (runs), watched (runs and is recorded; asks first when the ask carries a
//! page or selection, or with "ask before everything"), ask (asks with the
//! exact action every time) or block (refused). The deciders are the tools
//! themselves; the tests below pin this table to them, so a change to one
//! shows up here (`friday risks`, Settings).

use friday_actions::DesktopRisk;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Safe,
    Watched,
    Ask,
    Block,
}

impl Level {
    fn heading(self) -> &'static str {
        match self {
            Level::Safe => "Runs without asking",
            Level::Watched => "Runs and is recorded; asks first when the ask carries a page or selection, or with \"ask before everything\" on",
            Level::Ask => "Asks first, with the exact action",
            Level::Block => "Never runs",
        }
    }
}

/// One tool action and its level.
pub struct Rule {
    pub tool: &'static str,
    pub action: &'static str,
    pub level: Level,
    pub why: &'static str,
}

const fn rule(tool: &'static str, action: &'static str, level: Level, why: &'static str) -> Rule {
    Rule {
        tool,
        action,
        level,
        why,
    }
}

/// Desktop verbs, by what they can do (the desktop adapter's own classes).
pub const DESKTOP_SAFE: [&str; 6] = [
    "windows",
    "active_window",
    "workspaces",
    "now_playing",
    "find_app",
    "system_health",
];
pub const DESKTOP_WATCHED: [&str; 9] = [
    "focus_window",
    "move_window",
    "place_window",
    "switch_workspace",
    "launch_app",
    "media",
    "volume",
    "brightness",
    "power_profile",
];
pub const DESKTOP_ASK: [&str; 1] = ["close_window"];

pub const TABLE: &[Rule] = &[
    rule("memory", "recall what was saved", Level::Safe, "reads FRIDAY's own notes"),
    rule("memory", "remember a fact", Level::Safe, "saves what you asked it to"),
    rule("desktop", "look: windows, workspaces, what's playing, find an app, health", Level::Safe, "only reads"),
    rule("system", "diagnose with commands that only look", Level::Safe, "a fixed list of read-only commands"),
    rule("agents", "a step that reads or edits inside the run's folder", Level::Safe, "routine for a coding agent; secrets and ignored files still ask"),
    rule("files", "read a file", Level::Watched, "the file's text goes to your AI provider"),
    rule("browser", "open a web page in your browser", Level::Watched, "you see it, signed in as you"),
    rule("desktop", "focus, move or place a window, switch workspace, open an app as a step of a task, media, volume, brightness, power profile", Level::Watched, "undone by doing the opposite"),
    rule("desktop", "click a harmless button in an app", Level::Watched, "found by its label"),
    rule("agents", "start a coding agent run", Level::Watched, "its steps follow the rules below"),
    rule("tasks", "add, pause or remove a standing task", Level::Watched, "runs later, on its own"),
    rule("files", "write a file", Level::Ask, "shows the change first"),
    rule("terminal", "run a command", Level::Ask, "can change anything"),
    rule("apps", "open an app you name (\"open notepad\")", Level::Ask, "starts a program"),
    rule("desktop", "close a window", Level::Ask, "can lose unsaved work"),
    rule("desktop", "click delete, send, submit, pay and the like in an app", Level::Ask, "can't be taken back"),
    rule("system", "update, install or remove packages", Level::Ask, "changes the system, as root"),
    rule("agents", "a step that deletes, moves, fetches from the web, or works outside the folder", Level::Ask, "can't be undone from the run"),
    rule("browser", "open an address that isn't http or https", Level::Block, "file: and other schemes can read this computer"),
    rule("system", "diagnose with a command that changes something", Level::Block, "diagnosing never changes anything"),
    rule("agents", "start a run in private mode", Level::Block, "private asks stay on this computer"),
];

/// The table for people, by level.
pub fn report() -> String {
    [Level::Safe, Level::Watched, Level::Ask, Level::Block]
        .iter()
        .map(|level| {
            let rows: Vec<String> = TABLE
                .iter()
                .filter(|r| r.level == *level)
                .map(|r| format!("- {}: {} ({})", r.tool, r.action, r.why))
                .collect();
            format!("{}\n{}", level.heading(), rows.join("\n"))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The level a desktop verb has in this table.
pub fn desktop_level(verb: &str) -> Option<Level> {
    if DESKTOP_SAFE.contains(&verb) {
        Some(Level::Safe)
    } else if DESKTOP_WATCHED.contains(&verb) {
        Some(Level::Watched)
    } else if DESKTOP_ASK.contains(&verb) {
        Some(Level::Ask)
    } else {
        None
    }
}

/// The level the desktop adapter's own class means.
pub fn level_of(risk: DesktopRisk) -> Level {
    match risk {
        DesktopRisk::Read => Level::Safe,
        DesktopRisk::Reversible => Level::Watched,
        DesktopRisk::Destructive => Level::Ask,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_policy::{decide, Decision, Kind, Step};

    #[test]
    fn every_desktop_verb_has_the_level_its_adapter_gives_it() {
        for verb in DESKTOP_SAFE
            .iter()
            .chain(&DESKTOP_WATCHED)
            .chain(&DESKTOP_ASK)
        {
            let risk = friday_actions::desktop_risk_of(verb).unwrap_or_else(|| {
                panic!("{verb} is in the table but the adapter doesn't know it")
            });
            assert_eq!(Some(level_of(risk)), desktop_level(verb), "{verb}");
        }
        assert!(friday_actions::desktop_risk_of("delete_everything").is_none());
    }

    #[test]
    fn dangerous_app_buttons_ask_and_others_are_watched() {
        for label in ["Delete", "Send", "Submit order"] {
            assert!(
                friday_actions::label_is_dangerous(label),
                "{label} must ask first"
            );
        }
        assert!(!friday_actions::label_is_dangerous("Search"));
    }

    #[test]
    fn diagnosing_only_looks() {
        assert!(crate::diagnose::read_only("df -h", false));
        assert!(!crate::diagnose::read_only("rm -rf /tmp/x", false));
        assert!(!crate::diagnose::read_only("df -h; rm x", false));
    }

    #[test]
    fn agent_steps_ask_for_deletes_and_fetches_and_allow_reads_in_the_folder() {
        let folder = std::env::temp_dir();
        let step = |kind| Step {
            kind,
            command: None,
            paths: vec![folder.join("a.txt").display().to_string()],
        };
        assert!(matches!(
            decide(&step(Kind::Delete), &folder),
            Decision::Ask(_)
        ));
        assert!(matches!(
            decide(&step(Kind::Fetch), &folder),
            Decision::Ask(_)
        ));
        assert_eq!(decide(&step(Kind::Read), &folder), Decision::Allow);
    }

    #[test]
    fn every_level_is_shown_and_every_rule_says_why() {
        let r = report();
        for level in [Level::Safe, Level::Watched, Level::Ask, Level::Block] {
            assert!(r.contains(level.heading()));
        }
        assert!(TABLE.iter().all(|t| !t.why.is_empty()));
        assert!(r.contains("- terminal: run a command (can change anything)"));
    }
}
