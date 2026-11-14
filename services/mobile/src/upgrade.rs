//! Executable upgrade detection for daemon lifecycle management.
//!
//! When system packages (such as Arch's `friday-git`) are upgraded, the old
//! binary running in user sessions should detect that its executable on disk
//! was replaced and gracefully exit so systemd or the service supervisor
//! can restart it with the new binary.

use std::path::Path;
use std::time::SystemTime;

/// Snapshot of the executable's file identity on disk at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExeSnapshot {
    pub dev: u64,
    pub ino: u64,
    pub len: u64,
    pub mtime: Option<SystemTime>,
}

impl ExeSnapshot {
    /// Capture the file identity of the executable at `path`.
    pub fn from_path(path: &Path) -> std::io::Result<Self> {
        let meta = std::fs::metadata(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                dev: meta.dev(),
                ino: meta.ino(),
                len: meta.len(),
                mtime: meta.modified().ok(),
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {
                dev: 0,
                ino: 0,
                len: meta.len(),
                mtime: meta.modified().ok(),
            })
        }
    }
}

/// Pure detection function: returns true if the executable has been replaced on disk.
///
/// Replacement is detected if:
/// 1. `proc_exe_link` ends with `" (deleted)"` (Linux kernel marker for unlinked/replaced binary).
/// 2. The file at `current_path` no longer exists or cannot be read.
/// 3. The file at `current_path` has a different inode, device, size, or modification time than `initial`.
pub fn is_executable_replaced(
    initial: &ExeSnapshot,
    current_path: &Path,
    proc_exe_link: Option<&Path>,
) -> bool {
    if let Some(link) = proc_exe_link {
        if link.to_string_lossy().ends_with(" (deleted)") {
            return true;
        }
    }

    match std::fs::metadata(current_path) {
        Ok(current) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if current.dev() != initial.dev || current.ino() != initial.ino {
                    return true;
                }
            }
            if current.len() != initial.len {
                return true;
            }
            if let (Some(init_mtime), Ok(curr_mtime)) = (initial.mtime, current.modified()) {
                if init_mtime != curr_mtime {
                    return true;
                }
            }
            false
        }
        Err(_) => true,
    }
}

/// Check if the currently running executable has been replaced on disk.
pub fn is_self_replaced(initial: &ExeSnapshot, current_path: &Path) -> bool {
    #[cfg(target_os = "linux")]
    let proc_link = std::fs::read_link("/proc/self/exe").ok();
    #[cfg(not(target_os = "linux"))]
    let proc_link: Option<std::path::PathBuf> = None;

    is_executable_replaced(initial, current_path, proc_link.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    struct TempDirGuard(PathBuf);
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn test_temp_dir() -> (PathBuf, TempDirGuard) {
        let dir =
            std::env::temp_dir().join(format!("friday-upgrade-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create test temp dir");
        (dir.clone(), TempDirGuard(dir))
    }

    #[test]
    fn test_same_file_is_not_replaced() {
        let (dir, _guard) = test_temp_dir();
        let bin_path = dir.join("test-binary");
        std::fs::write(&bin_path, b"initial-binary-v1").expect("write test file");

        let initial = ExeSnapshot::from_path(&bin_path).expect("snapshot initial");
        assert!(
            !is_executable_replaced(&initial, &bin_path, None),
            "unmodified file must not be reported as replaced"
        );
    }

    #[test]
    fn test_file_replaced_via_rename() {
        let (dir, _guard) = test_temp_dir();
        let bin_path = dir.join("test-binary");
        std::fs::write(&bin_path, b"initial-binary-v1").expect("write initial file");

        let initial = ExeSnapshot::from_path(&bin_path).expect("snapshot initial");

        // Write a new file and atomically rename it over the old one
        let new_path = dir.join("test-binary.new");
        std::fs::write(&new_path, b"upgraded-binary-v2").expect("write new file");
        std::fs::rename(&new_path, &bin_path).expect("rename over initial");

        assert!(
            is_executable_replaced(&initial, &bin_path, None),
            "renaming a new file over the binary must be detected as replaced"
        );
    }

    #[test]
    fn test_proc_exe_deleted_marker() {
        let (dir, _guard) = test_temp_dir();
        let bin_path = dir.join("test-binary");
        std::fs::write(&bin_path, b"binary-v1").expect("write initial file");

        let initial = ExeSnapshot::from_path(&bin_path).expect("snapshot initial");
        let deleted_link = Path::new("/usr/bin/friday-linux-daemon (deleted)");

        assert!(
            is_executable_replaced(&initial, &bin_path, Some(deleted_link)),
            "proc link ending in (deleted) must be detected as replaced"
        );
    }

    #[test]
    fn test_missing_file_is_detected_as_replaced() {
        let (dir, _guard) = test_temp_dir();
        let bin_path = dir.join("test-binary");
        std::fs::write(&bin_path, b"binary-v1").expect("write initial file");

        let initial = ExeSnapshot::from_path(&bin_path).expect("snapshot initial");
        std::fs::remove_file(&bin_path).expect("remove file");

        assert!(
            is_executable_replaced(&initial, &bin_path, None),
            "missing binary file must be detected as replaced"
        );
    }

    #[test]
    fn test_modified_in_place() {
        let (dir, _guard) = test_temp_dir();
        let bin_path = dir.join("test-binary");
        std::fs::write(&bin_path, b"binary-v1").expect("write initial file");

        let initial = ExeSnapshot::from_path(&bin_path).expect("snapshot initial");

        // Append bytes to change len and mtime
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&bin_path)
            .expect("open file");
        f.write_all(b"-appended").expect("append");
        drop(f);

        assert!(
            is_executable_replaced(&initial, &bin_path, None),
            "in-place file modification must be detected as replaced"
        );
    }
}
