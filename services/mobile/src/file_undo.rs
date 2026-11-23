//! A file FRIDAY wrote can be put back (`friday undo`): what was there
//! before (or that nothing was) is kept beside the undo journal, and putting
//! it back happens only while the file still holds exactly what FRIDAY wrote,
//! so a change made since is never lost.

use std::path::{Path, PathBuf};

use serde_json::json;
use sha2::{Digest, Sha256};

/// Files bigger than this aren't kept for undo.
const MAX_KEPT: u64 = 5 * 1024 * 1024;
/// Kept copies at most (the journal keeps its last 20 too).
const KEEP: usize = 20;

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Where the copies live, beside the undo journal in `dir`.
pub fn copies_dir(dir: &Path) -> PathBuf {
    dir.join("undo-files")
}

/// What was at `path` before a write: `Some(None)` when nothing was,
/// `Some(Some(copy))` for a kept copy, `None` when it can't be kept (too big,
/// unreadable): that write then can't be undone.
pub fn before_write(dir: &Path, path: &Path) -> Option<Option<PathBuf>> {
    let meta = match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(None),
        Err(_) => return None,
        Ok(m) => m,
    };
    if !meta.is_file() || meta.len() > MAX_KEPT {
        return None;
    }
    let copies = copies_dir(dir);
    std::fs::create_dir_all(&copies).ok()?;
    let copy = copies.join(uuid::Uuid::new_v4().simple().to_string());
    std::fs::copy(path, &copy).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o600));
    }
    // The oldest copies go.
    let mut all: Vec<_> = std::fs::read_dir(&copies)
        .ok()?
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    all.sort();
    for (_, p) in all.iter().rev().skip(KEEP) {
        let _ = std::fs::remove_file(p);
    }
    Some(Some(copy))
}

/// The undo journal's entry for a write to `path` that just happened.
pub fn entry(path: &Path, before: &Option<PathBuf>) -> Option<serde_json::Value> {
    let now = std::fs::read(path).ok()?;
    let name = path.file_name()?.to_string_lossy().to_string();
    Some(json!({
        "ts": chrono::Local::now().to_rfc3339(),
        "said": format!("writing {name}"),
        "kind": "file",
        "args": [
            path.display().to_string(),
            before.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            hash(&now),
        ],
    }))
}

/// Puts `path` back as it was before FRIDAY wrote `written` (its hash):
/// the kept copy, or no file when there was none. Only from `dir`'s copies,
/// and only while the file is still what FRIDAY wrote.
pub fn undo(dir: &Path, path: &Path, copy: &str, written: &str) -> Result<String, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    let now = std::fs::read(path)
        .map_err(|_| format!("{name} isn't there any more, so there's nothing to put back"))?;
    if hash(&now) != written {
        return Err(format!(
            "{name} changed after FRIDAY wrote it, so it's left as it is"
        ));
    }
    if copy.is_empty() {
        std::fs::remove_file(path).map_err(|e| format!("couldn't remove {name}: {e}"))?;
        return Ok(format!(
            "{name} didn't exist before FRIDAY wrote it, so it's gone again."
        ));
    }
    let copy = PathBuf::from(copy);
    let copies = copies_dir(dir)
        .canonicalize()
        .map_err(|_| "the kept copy is gone".to_string())?;
    let copy = copy
        .canonicalize()
        .ok()
        .filter(|c| c.parent() == Some(copies.as_path()))
        .ok_or_else(|| "the kept copy is gone".to_string())?;
    std::fs::copy(&copy, path).map_err(|e| format!("couldn't put {name} back: {e}"))?;
    let _ = std::fs::remove_file(&copy);
    Ok(format!("{name} is back as it was before FRIDAY wrote it."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "friday-file-undo-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn args(e: &serde_json::Value, i: usize) -> String {
        e["args"][i].as_str().unwrap().to_string()
    }

    #[test]
    fn a_write_is_put_back_and_a_new_file_removed() {
        let d = dir();
        let file = d.join("notes.txt");
        std::fs::write(&file, "mine").unwrap();
        let before = before_write(&d, &file).unwrap();
        std::fs::write(&file, "FRIDAY's").unwrap();
        let e = entry(&file, &before).unwrap();
        assert_eq!(e["kind"], "file");
        assert!(undo(&d, &file, &args(&e, 1), &args(&e, 2))
            .unwrap()
            .contains("is back"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");

        let new = d.join("new.txt");
        let before = before_write(&d, &new).unwrap();
        assert!(before.is_none());
        std::fs::write(&new, "FRIDAY's").unwrap();
        let e = entry(&new, &before).unwrap();
        undo(&d, &new, &args(&e, 1), &args(&e, 2)).unwrap();
        assert!(!new.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_changed_since_is_left_alone_and_only_kept_copies_are_used() {
        let d = dir();
        let file = d.join("notes.txt");
        std::fs::write(&file, "mine").unwrap();
        let before = before_write(&d, &file).unwrap();
        std::fs::write(&file, "FRIDAY's").unwrap();
        let e = entry(&file, &before).unwrap();
        std::fs::write(&file, "edited since").unwrap();
        assert!(undo(&d, &file, &args(&e, 1), &args(&e, 2))
            .unwrap_err()
            .contains("changed"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "edited since");

        // A journal line pointing at some other file never copies it in.
        std::fs::write(&file, "FRIDAY's").unwrap();
        let elsewhere = d.join("secret.txt");
        std::fs::write(&elsewhere, "not a kept copy").unwrap();
        let err = undo(&d, &file, &elsewhere.display().to_string(), &args(&e, 2)).unwrap_err();
        assert!(err.contains("gone"), "{err}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "FRIDAY's");
        let _ = std::fs::remove_dir_all(&d);
    }
}
