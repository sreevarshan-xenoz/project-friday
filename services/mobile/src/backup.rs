//! A backup of what FRIDAY keeps for the person (settings, memory, answer
//! history, standing tasks, workspaces, agent records), checked before it is
//! trusted: each file is listed with its sha256, and a restore checks the
//! whole backup first and keeps the current files aside, so it can be undone.
//!
//! Left out: API keys (a key's line in `friday.env` is kept with no value),
//! downloaded voices and models (they download again), and running files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The two folders backed up, by the name they have inside a backup.
pub fn folders() -> Vec<(&'static str, PathBuf)> {
    #[cfg(windows)]
    let config = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    let state =
        friday_intelligence::egress::state_dir_from(|k| std::env::var(k).ok(), cfg!(windows));
    vec![
        ("config", config.unwrap_or_default().join("friday")),
        ("state", state),
    ]
}

/// Where backups go unless the person names a folder.
pub fn default_parent() -> PathBuf {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join("FRIDAY backups")
}

/// Folders and files that aren't the person's: large downloads, programs,
/// what a running FRIDAY holds open, and earlier restores' leftovers.
const SKIP_DIRS: &[&str] = &[
    "voice", "whisper", "local-ai", "models", "engine", "bin", "logs", "cache", "tmp",
];
const SKIP_PREFIXES: &[&str] = &["before-restore-", ".friday-voice-setup-", "friday-backup-"];
/// The running daemon's state: its session token and address, not the person's.
const SKIP_FILES: &[&str] = &["state.json"];
const SKIP_ENDINGS: &[&str] = &[".pid", ".sock", ".lock", ".tmp", ".part"];
/// One file larger than this is a download or a cache, not a setting or a note.
const MAX_FILE: u64 = 50 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    /// `config/…` or `state/…`, with `/` between parts.
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub created: String,
    pub files: Vec<Entry>,
}

const MANIFEST: &str = "manifest.json";

fn sha256_hex(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).map_err(|e| e.to_string())?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn skipped(name: &str, is_dir: bool) -> bool {
    (is_dir && SKIP_DIRS.contains(&name))
        || (!is_dir && SKIP_FILES.contains(&name))
        || SKIP_PREFIXES.iter().any(|p| name.starts_with(p))
        || (!is_dir && SKIP_ENDINGS.iter().any(|e| name.ends_with(e)))
}

/// Every file to back up under `dir`, as paths relative to it.
fn walk(dir: &Path, rel: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir.join(rel)) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_symlink() || skipped(&name, kind.is_dir()) {
            continue;
        }
        let path = rel.join(&name);
        if kind.is_dir() {
            walk(dir, &path, out);
        } else if e.metadata().is_ok_and(|m| m.len() <= MAX_FILE) {
            out.push(path);
        }
    }
}

/// Whether a `friday.env` line holds a secret: its value isn't backed up.
fn secret_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD", "PASS"]
        .iter()
        .any(|w| n.contains(w))
}

/// `friday.env` with every secret's value taken out (`NAME=`).
fn without_secrets(env: &str) -> String {
    env.lines()
        .map(|l| match l.split_once('=') {
            Some((name, _)) if !l.trim_start().starts_with('#') && secret_name(name.trim()) => {
                format!("{}=", name)
            }
            _ => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn rel_string(top: &str, rel: &Path) -> String {
    let mut s = top.to_string();
    for part in rel.components() {
        s.push('/');
        s.push_str(&part.as_os_str().to_string_lossy());
    }
    s
}

/// Makes a backup of `sources` in a new folder under `parent` and checks it.
/// Returns the folder and how many files it holds.
pub fn create(
    sources: &[(&str, PathBuf)],
    parent: &Path,
    stamp: &str,
) -> Result<(PathBuf, usize), String> {
    let out = parent.join(format!("friday-backup-{stamp}"));
    // Not inside a folder being backed up: it would copy itself.
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let parent_real = real(parent);
    if sources
        .iter()
        .any(|(_, dir)| parent_real.starts_with(real(dir)))
    {
        return Err("put the backup outside FRIDAY's own folders".into());
    }
    if out.exists() {
        return Err(format!("{} is there already", out.display()));
    }
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for (top, dir) in sources {
        let mut found = Vec::new();
        walk(dir, Path::new(""), &mut found);
        for rel in found {
            let path = rel_string(top, &rel);
            let dest = out.join(&path);
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            if rel.file_name().is_some_and(|n| n == "friday.env") {
                let env = std::fs::read_to_string(dir.join(&rel)).map_err(|e| e.to_string())?;
                std::fs::write(&dest, without_secrets(&env)).map_err(|e| e.to_string())?;
            } else {
                std::fs::copy(dir.join(&rel), &dest).map_err(|e| e.to_string())?;
            }
            files.push(Entry {
                size: std::fs::metadata(&dest).map_err(|e| e.to_string())?.len(),
                sha256: sha256_hex(&dest)?,
                path,
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let n = files.len();
    let manifest = Manifest {
        format: 1,
        created: stamp.to_string(),
        files,
    };
    std::fs::write(
        out.join(MANIFEST),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    check(&out)?;
    Ok((out, n))
}

/// Checks a backup: its list is readable and every file is there, whole.
/// The error names what is wrong.
pub fn check(backup: &Path) -> Result<Manifest, String> {
    let bytes = std::fs::read(backup.join(MANIFEST))
        .map_err(|_| format!("{} isn't a FRIDAY backup (no {MANIFEST})", backup.display()))?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| format!("{MANIFEST} is damaged"))?;
    if manifest.format != 1 {
        return Err("this backup was made by a newer FRIDAY".into());
    }
    for f in &manifest.files {
        // Only plain names between the slashes: no "..", no Windows "\\" or
        // drive letters, which could reach outside FRIDAY's folders.
        if f.path
            .split('/')
            .any(|p| p == ".." || p == "." || p.is_empty() || p.contains(['\\', ':']))
            || !(f.path.starts_with("config/") || f.path.starts_with("state/"))
        {
            return Err(format!(
                "{MANIFEST} lists a path outside FRIDAY's folders: {}",
                f.path
            ));
        }
        let path = backup.join(&f.path);
        let size = std::fs::metadata(&path)
            .map_err(|_| format!("{} is missing", f.path))?
            .len();
        if size != f.size || sha256_hex(&path)? != f.sha256 {
            return Err(format!("{} has changed since the backup was made", f.path));
        }
    }
    Ok(manifest)
}

/// Brings a checked backup back into `targets`. Each file it replaces is
/// moved first to `before-restore-<stamp>` in the same folder, and the API
/// keys in the current `friday.env` are kept. Nothing is written unless the
/// whole backup checks out. Returns how many files were restored.
pub fn restore(backup: &Path, targets: &[(&str, PathBuf)], stamp: &str) -> Result<usize, String> {
    let manifest = check(backup)?;
    // What has been done so far: each file written, and where the one it
    // replaced went. A failure puts every one back.
    let mut done: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let result = (|| {
        for f in &manifest.files {
            let (top, rest) = f.path.split_once('/').expect("checked above");
            let Some((_, dir)) = targets.iter().find(|(t, _)| *t == top) else {
                continue;
            };
            let dest = rest.split('/').fold(dir.clone(), |p, part| p.join(part));
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut current_env = None;
            let mut aside = None;
            if dest.is_file() {
                if dest.file_name().is_some_and(|n| n == "friday.env") {
                    current_env = std::fs::read_to_string(&dest).ok();
                }
                let to = rest
                    .split('/')
                    .fold(dir.join(format!("before-restore-{stamp}")), |p, part| {
                        p.join(part)
                    });
                if let Some(p) = to.parent() {
                    std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
                }
                std::fs::rename(&dest, &to).map_err(|e| {
                    format!("couldn't move {} aside (is FRIDAY using it?): {e}", f.path)
                })?;
                aside = Some(to);
            }
            done.push((dest.clone(), aside));
            let from = backup.join(&f.path);
            match current_env {
                Some(env) => {
                    let restored = std::fs::read_to_string(&from).map_err(|e| e.to_string())?;
                    std::fs::write(&dest, keep_keys(&restored, &env)).map_err(|e| e.to_string())?;
                }
                None => {
                    std::fs::copy(&from, &dest).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(manifest.files.len())
    })();
    if result.is_err() {
        for (dest, aside) in done.into_iter().rev() {
            let _ = std::fs::remove_file(&dest);
            if let Some(aside) = aside {
                let _ = std::fs::rename(aside, &dest);
            }
        }
    }
    result
}

/// The restored `friday.env` with the current file's secret values put back.
fn keep_keys(restored: &str, current: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let value = |name: &str| {
        current
            .lines()
            .find_map(|l| l.split_once('=').filter(|(n, _)| n.trim() == name))
            .map(|(_, v)| v.to_string())
    };
    let mut seen = Vec::new();
    for l in restored.lines() {
        match l.split_once('=') {
            Some((name, _)) if secret_name(name.trim()) => {
                seen.push(name.trim().to_string());
                match value(name.trim()) {
                    Some(v) => out.push(format!("{name}={v}")),
                    None => out.push(l.to_string()),
                }
            }
            _ => out.push(l.to_string()),
        }
    }
    // Keys set since the backup was made stay too.
    for l in current.lines() {
        if let Some((name, _)) = l.split_once('=') {
            if secret_name(name.trim()) && !seen.iter().any(|s| s == name.trim()) {
                out.push(l.to_string());
            }
        }
    }
    out.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "friday-backup-test-{name}-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn a_backup_holds_notes_and_settings_but_no_keys_or_downloads() {
        let (config, state, out) = (tmp("config"), tmp("state"), tmp("out"));
        put(&config, "memory.md", "- I use fish\n");
        put(
            &config,
            "friday.env",
            "FRIDAY_PROVIDER=opencode\nOPENROUTER_API_KEY=not-a-real-one\n# a note\n",
        );
        put(&state, "answers.md", "## q\nanswer\n");
        put(&state, "agents/r1.json", "{}");
        put(&state, "voice/model/model.onnx", "big");
        put(&state, "friday-say.pid", "123");
        let sources = [("config", config.clone()), ("state", state.clone())];
        let (backup, n) = create(&sources, &out, "2026-11-23-2000").unwrap();
        assert_eq!(n, 4, "memory, env, answers, agent record");
        let env = std::fs::read_to_string(backup.join("config/friday.env")).unwrap();
        assert!(env.contains("FRIDAY_PROVIDER=opencode"));
        assert!(env.contains("OPENROUTER_API_KEY=\n"), "{env}");
        assert!(!env.contains("not-a-real-one"));
        assert!(!backup.join("state/voice").exists());
        assert!(!backup.join("state/friday-say.pid").exists());
        assert!(check(&backup).is_ok());
        for d in [config, state, out] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn a_changed_or_missing_file_fails_the_check_and_nothing_is_restored() {
        let (config, out, target) = (tmp("config"), tmp("out"), tmp("target"));
        put(&config, "memory.md", "- old fact\n");
        let (backup, _) = create(&[("config", config.clone())], &out, "s1").unwrap();
        put(&target, "memory.md", "- current fact\n");
        std::fs::write(backup.join("config/memory.md"), "- tampered\n").unwrap();
        let err = restore(&backup, &[("config", target.clone())], "s2").unwrap_err();
        assert!(err.contains("config/memory.md has changed"), "{err}");
        assert_eq!(
            std::fs::read_to_string(target.join("memory.md")).unwrap(),
            "- current fact\n",
            "nothing was replaced"
        );
        std::fs::remove_file(backup.join("config/memory.md")).unwrap();
        assert!(check(&backup).unwrap_err().contains("is missing"));
        for d in [config, out, target] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn a_restore_keeps_the_current_files_aside_and_the_current_keys() {
        let (config, out, target) = (tmp("config"), tmp("out"), tmp("target"));
        put(&config, "memory.md", "- from the backup\n");
        put(
            &config,
            "friday.env",
            "FRIDAY_PROVIDER=opencode\nOPENROUTER_API_KEY=old-one\n",
        );
        let (backup, _) = create(&[("config", config.clone())], &out, "s1").unwrap();
        put(&target, "memory.md", "- current\n");
        put(
            &target,
            "friday.env",
            "FRIDAY_PROVIDER=local\nOPENROUTER_API_KEY=current-one\nGROQ_API_KEY=added-later\n",
        );
        assert_eq!(
            restore(&backup, &[("config", target.clone())], "s2").unwrap(),
            2
        );
        assert_eq!(
            std::fs::read_to_string(target.join("memory.md")).unwrap(),
            "- from the backup\n"
        );
        let env = std::fs::read_to_string(target.join("friday.env")).unwrap();
        assert!(env.contains("FRIDAY_PROVIDER=opencode"), "{env}");
        assert!(env.contains("OPENROUTER_API_KEY=current-one"), "{env}");
        assert!(env.contains("GROQ_API_KEY=added-later"), "{env}");
        assert_eq!(
            std::fs::read_to_string(target.join("before-restore-s2/memory.md")).unwrap(),
            "- current\n",
            "the replaced file is kept"
        );
        for d in [config, out, target] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn the_daemons_token_and_old_backups_stay_out_and_a_backup_cant_go_inside() {
        let (state, out) = (tmp("state"), tmp("out"));
        put(&state, "state.json", r#"{"token":"tok"}"#);
        put(&state, "answers.md", "a\n");
        put(&state, "friday-backup-old/manifest.json", "{}");
        let (backup, n) = create(&[("state", state.clone())], &out, "s1").unwrap();
        assert_eq!(n, 1, "only answers.md");
        assert!(!backup.join("state/state.json").exists());
        let err = create(&[("state", state.clone())], &state, "s2").unwrap_err();
        assert!(err.contains("outside FRIDAY's own folders"), "{err}");
        for d in [state, out] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn a_failed_restore_puts_everything_back() {
        let (config, out, target) = (tmp("config"), tmp("out"), tmp("target"));
        put(&config, "a.md", "from backup a\n");
        put(&config, "b/c.md", "from backup c\n");
        let (backup, _) = create(&[("config", config.clone())], &out, "s1").unwrap();
        put(&target, "a.md", "current a\n");
        // b is a file where the backup needs a folder: the second file fails.
        put(&target, "b", "a file in the way\n");
        assert!(restore(&backup, &[("config", target.clone())], "s2").is_err());
        assert_eq!(
            std::fs::read_to_string(target.join("a.md")).unwrap(),
            "current a\n"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("b")).unwrap(),
            "a file in the way\n"
        );
        for d in [config, out, target] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn a_manifest_cannot_reach_out_with_windows_paths() {
        let out = tmp("evil-win");
        for path in ["config/..\\..\\x", "config/C:\\evil.bat", "config/./x"] {
            std::fs::write(
                out.join(MANIFEST),
                format!(r#"{{"format":1,"created":"x","files":[{{"path":{},"size":1,"sha256":"00"}}]}}"#, serde_json::to_string(path).unwrap()),
            )
            .unwrap();
            assert!(
                check(&out)
                    .unwrap_err()
                    .contains("outside FRIDAY's folders"),
                "{path}"
            );
        }
        let _ = std::fs::remove_dir_all(out);
    }

    #[test]
    fn a_manifest_cannot_write_outside_fridays_folders() {
        let out = tmp("evil");
        std::fs::write(
            out.join(MANIFEST),
            r#"{"format":1,"created":"x","files":[{"path":"config/../../etc/x","size":1,"sha256":"00"}]}"#,
        )
        .unwrap();
        assert!(check(&out)
            .unwrap_err()
            .contains("outside FRIDAY's folders"));
        let _ = std::fs::remove_dir_all(out);
    }
}
