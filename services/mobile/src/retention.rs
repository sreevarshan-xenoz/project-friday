//! How long each kind of record FRIDAY keeps stays (`friday-ctl retention`).
//! What the person asked FRIDAY to remember stays until they say otherwise;
//! what only helped with one moment goes on its own. Most kinds already
//! expire where they are written (screen recall, conversations, lessons,
//! clips, answers, the egress log); this sweeps the records that otherwise
//! grew for ever: finished agent runs, the agent hooks log and `friday wrong`
//! reports, after 30 days.

use std::path::Path;

/// Finished agent runs, the hooks log and wrong-answer reports: 30 days.
pub const KEEP_SECS: i64 = 30 * 24 * 60 * 60;

/// Each kind, how long it stays, and why: the table `friday-ctl retention` shows.
pub const RULES: &[(&str, &str)] = &[
    (
        "Facts you asked FRIDAY to remember (memory.md)",
        "until you forget them",
    ),
    (
        "Memory journal and thoughts",
        "until you forget them (friday forget memory)",
    ),
    ("Day summaries", "until you forget them (friday day forget)"),
    ("Standing tasks", "until you delete them"),
    (
        "Desktop action log",
        "kept: it is chained for friday verify-log (friday forget actions)",
    ),
    (
        "Screen recall",
        "3 hours (or 24 hours or 7 days, as set), 20 MB at most",
    ),
    ("Conversations", "1 day"),
    ("Lessons from answers", "30 days, 200 at most"),
    ("Finished agent runs and the agent hooks log", "30 days"),
    ("Wrong-answer reports (friday wrong)", "30 days"),
    ("Answer history", "the newest 2 MB, and the one before"),
    ("Ask history", "the last 50 questions"),
    ("Clips", "the last 200"),
    ("Dictations", "the last 20"),
    ("Undo list", "the last 20 changes"),
    (
        "What went to AI providers (egress log)",
        "about 1 MB, newest kept",
    ),
];

fn modified_secs(p: &Path) -> Option<i64> {
    let m = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(m.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64)
}

/// Removes what is past its keep in the state folder `state` at `now` (unix
/// seconds). Returns how many records went.
pub fn sweep(state: &Path, now: i64) -> usize {
    let old = |t: i64| now - t > KEEP_SECS;
    let mut gone = 0;
    // Agent runs that ended over 30 days ago and have no copy left open.
    let agents = state.join("agents");
    let runs = crate::agent_runs::list(&agents);
    for run in &runs {
        let ended_long_ago = run.ended.is_some_and(old);
        // Still needed: its copy (or the copy named after it) is open, or
        // another kept run follows on from it.
        let copy_open = run.worktree.as_ref().is_some_and(|w| w.is_dir())
            || agents.join("worktrees").join(&run.id).is_dir();
        let followed = runs
            .iter()
            .any(|r| r.follows.as_deref() == Some(run.id.as_str()));
        if ended_long_ago && !copy_open && !followed {
            for ext in ["json", "jsonl"] {
                if std::fs::remove_file(agents.join(format!("{}.{ext}", run.id))).is_ok()
                    && ext == "json"
                {
                    gone += 1;
                }
            }
        }
    }
    // The hooks log: its lines past the keep.
    let hooks = agents.join("hooks.jsonl");
    if let Ok(text) = std::fs::read_to_string(&hooks) {
        let lines: Vec<&str> = text.lines().collect();
        let keep: Vec<&str> = lines
            .iter()
            .copied()
            .filter(|l| {
                serde_json::from_str::<serde_json::Value>(l)
                    .ok()
                    .and_then(|v| v["ts"].as_i64())
                    .is_none_or(|t| !old(t))
            })
            .collect();
        if keep.len() < lines.len() {
            let tmp = hooks.with_extension("jsonl.tmp");
            let body = if keep.is_empty() {
                String::new()
            } else {
                keep.join("\n") + "\n"
            };
            if std::fs::write(&tmp, body).is_ok() && std::fs::rename(&tmp, &hooks).is_ok() {
                gone += lines.len() - keep.len();
            }
        }
    }
    // Wrong-answer reports, by when they were written.
    if let Ok(entries) = std::fs::read_dir(state.join("repros")) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json")
                && modified_secs(&p).is_some_and(old)
                && std::fs::remove_file(&p).is_ok()
            {
                gone += 1;
            }
        }
    }
    gone
}

/// The rules as text.
pub fn describe() -> String {
    let mut out =
        vec!["How long FRIDAY keeps each kind of record (all on this computer):".to_string()];
    for (what, keep) in RULES {
        out.push(format!("  {what}: {keep}"));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_temporary_records_go_and_the_rest_stay() {
        let state = std::env::temp_dir().join(format!("friday-retain-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let agents = state.join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::create_dir_all(state.join("repros")).unwrap();
        let now = 100 * 24 * 3600;
        let run = |id: &str, ended: Option<i64>| {
            format!(
                r#"{{"id":"{id}","agent":"opencode","folder":"/w","request":"x","task":"x","status":"done","started":0,"ended":{}}}"#,
                ended.map_or("null".into(), |e| e.to_string())
            )
        };
        std::fs::write(
            agents.join("rold.json"),
            run("rold", Some(now - 40 * 24 * 3600)),
        )
        .unwrap();
        std::fs::write(agents.join("rold.jsonl"), "{}\n").unwrap();
        std::fs::write(agents.join("rnew.json"), run("rnew", Some(now - 3600))).unwrap();
        std::fs::write(agents.join("rgoing.json"), run("rgoing", None)).unwrap();
        // Ended long ago, but a kept run follows on from it.
        std::fs::write(
            agents.join("rbase.json"),
            run("rbase", Some(now - 50 * 24 * 3600)),
        )
        .unwrap();
        std::fs::write(
            agents.join("rnext.json"),
            run("rnext", None).replace("\"ended\":null", "\"ended\":null,\"follows\":\"rbase\""),
        )
        .unwrap();
        std::fs::write(
            agents.join("hooks.jsonl"),
            format!(
                "{{\"ts\":{}}}\n{{\"ts\":{}}}\n",
                now - 40 * 24 * 3600,
                now - 60
            ),
        )
        .unwrap();
        std::fs::write(state.join("memory.md"), "kept").unwrap();
        let gone = sweep(&state, now);
        assert!(!agents.join("rold.json").exists() && !agents.join("rold.jsonl").exists());
        assert!(agents.join("rnew.json").exists() && agents.join("rgoing.json").exists());
        assert!(
            agents.join("rbase.json").exists(),
            "a run followed on from stays"
        );
        let hooks = std::fs::read_to_string(agents.join("hooks.jsonl")).unwrap();
        assert_eq!(hooks.lines().count(), 1, "{hooks}");
        assert!(state.join("memory.md").exists());
        assert_eq!(gone, 2);
        assert!(describe().contains("Screen recall"));
        let _ = std::fs::remove_dir_all(&state);
    }
}
