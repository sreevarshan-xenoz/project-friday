//! The action log (`actions.jsonl`): every desktop and system action FRIDAY
//! took, one JSON line each. Each line carries `prev`, the SHA-256 of the
//! line before it, so a line changed or removed afterwards shows
//! (`friday verify-log`). Lines cut off the end can't be told from lines
//! never written; everything before them still checks.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde_json::Value;
use sha2::{Digest, Sha256};

/// The file, in FRIDAY's state folder.
pub const FILE: &str = "actions.jsonl";

/// One writer at a time, so two actions never chain to the same line.
static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn hash(line: &str) -> String {
    let digest = Sha256::digest(line.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The last line of the file (without its newline), read from its end.
fn last_line(f: &mut std::fs::File) -> Option<String> {
    let len = f.seek(SeekFrom::End(0)).ok()?;
    let from = len.saturating_sub(256 * 1024);
    f.seek(SeekFrom::Start(from)).ok()?;
    let mut tail = String::new();
    f.read_to_string(&mut tail).ok()?;
    tail.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(str::to_string)
}

/// Appends `entry` to the log in `dir`, owner-readable only, chained to the
/// line before it (with the `log-chain` switch on). A write that fails never
/// stops the action it records.
pub fn append(dir: &Path, entry: &Value) {
    let _one = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let _ = std::fs::create_dir_all(dir);
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).read(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let Ok(mut f) = opts.open(dir.join(FILE)) else {
        return;
    };
    let mut entry = entry.clone();
    if crate::features::on("log-chain") {
        if let Some(map) = entry.as_object_mut() {
            let prev = last_line(&mut f).map(|l| hash(&l)).unwrap_or_default();
            map.insert("prev".into(), Value::String(prev));
        }
    }
    let _ = writeln!(f, "{entry}");
}

/// Checks the chain in `dir`'s log: the plain answer `friday verify-log` gives.
/// Err when a line was changed or removed (naming the first one).
pub fn verify(dir: &Path) -> Result<String, String> {
    let text = match std::fs::read_to_string(dir.join(FILE)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok("The action log is empty: FRIDAY hasn't acted on this computer yet.".into())
        }
        Err(e) => return Err(format!("couldn't read the action log: {e}")),
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    // Lines from before the chain began carry no `prev`.
    let prev_of = |l: &str| {
        serde_json::from_str::<Value>(l)
            .ok()
            .and_then(|v| v.get("prev").and_then(Value::as_str).map(str::to_string))
    };
    let Some(start) = lines.iter().position(|l| prev_of(l).is_some()) else {
        return Ok(format!(
            "{} actions logged, none of them chained yet (the next one starts the chain).",
            lines.len()
        ));
    };
    // A line written with the chain off (or added at the end by hand) has no
    // link: counted, not an error. A changed or inserted line still breaks
    // the link of the chained line after it.
    let mut unlinked = 0;
    for (i, line) in lines.iter().enumerate().skip(start) {
        let expected = if i == 0 {
            String::new()
        } else {
            hash(lines[i - 1])
        };
        match prev_of(line) {
            Some(p) if p == expected => {}
            Some(_) => {
                return Err(format!(
                    "line {} of the action log doesn't follow the line before it: that line, or one between them, was changed or removed.",
                    i + 1
                ))
            }
            None => unlinked += 1,
        }
    }
    let mut notes = Vec::new();
    if start > 0 {
        notes.push(format!(
            "{start} older ones from before the chain aren't covered"
        ));
    }
    if unlinked > 0 {
        notes.push(format!(
            "{unlinked} written with log-chain off or added by hand aren't covered"
        ));
    }
    let notes = if notes.is_empty() {
        String::new()
    } else {
        format!(" ({})", notes.join("; "))
    };
    Ok(format!(
        "The action log is intact: {} actions, each chained to the one before{notes}.",
        lines.len() - start - unlinked
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "friday-action-log-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_changed_or_removed_line_shows_and_an_intact_log_checks() {
        let d = dir();
        assert!(verify(&d).unwrap().contains("empty"));
        // One line from before the chain.
        std::fs::write(d.join(FILE), "{\"verb\":\"old\"}\n").unwrap();
        for verb in ["focus_window", "volume", "media"] {
            append(&d, &json!({ "verb": verb, "ok": true }));
        }
        let ok = verify(&d).unwrap();
        assert!(
            ok.contains("intact: 3 actions") && ok.contains("1 older"),
            "{ok}"
        );

        let text = std::fs::read_to_string(d.join(FILE)).unwrap();
        let changed = text.replace("\"volume\"", "\"close_window\"");
        std::fs::write(d.join(FILE), &changed).unwrap();
        let err = verify(&d).unwrap_err();
        assert!(err.contains("line 4"), "{err}");

        let mut lines: Vec<&str> = text.lines().collect();
        lines.remove(2);
        std::fs::write(d.join(FILE), lines.join("\n") + "\n").unwrap();
        assert!(verify(&d).unwrap_err().contains("line 3"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_line_slipped_in_between_shows_and_one_at_the_end_is_counted() {
        let d = dir();
        append(&d, &json!({ "verb": "volume" }));
        append(&d, &json!({ "verb": "media" }));
        let text = std::fs::read_to_string(d.join(FILE)).unwrap();
        let mut lines: Vec<&str> = text.lines().collect();
        lines.insert(1, "{\"verb\":\"close_window\"}");
        std::fs::write(d.join(FILE), lines.join("\n") + "\n").unwrap();
        assert!(verify(&d).unwrap_err().contains("line 3"));

        std::fs::write(d.join(FILE), text.clone() + "{\"verb\":\"close_window\"}\n").unwrap();
        let ok = verify(&d).unwrap();
        assert!(
            ok.contains("intact: 2 actions") && ok.contains("1 written with log-chain off"),
            "{ok}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
