//! Appending one line to a log FRIDAY keeps (thoughts, screen recall): owner
//! only, in one write, and after a line a crash cut short, so that line is
//! the only one lost and the new one is never glued onto it.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Appends `line` and a newline to `path`, first ending a torn last line.
pub fn append(path: &Path, line: &str) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).read(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(path)?;
    let torn = f.seek(SeekFrom::End(0))? > 0 && {
        f.seek(SeekFrom::End(-1))?;
        let mut last = [0u8; 1];
        f.read_exact(&mut last)?;
        last[0] != b'\n'
    };
    let mut bytes = Vec::with_capacity(line.len() + 2);
    if torn {
        bytes.push(b'\n');
    }
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
    f.write_all(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_after_a_torn_one_is_kept_whole() {
        let p = std::env::temp_dir().join(format!("friday-jsonl-{}.jsonl", rand::random::<u32>()));
        append(&p, r#"{"a":1}"#).unwrap();
        // A crash in the middle of the next write.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(br#"{"a":"#)
            .unwrap();
        append(&p, r#"{"a":3}"#).unwrap();
        let kept: Vec<serde_json::Value> = std::fs::read_to_string(&p)
            .unwrap()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        assert_eq!(
            kept,
            [serde_json::json!({"a":1}), serde_json::json!({"a":3})]
        );
        let _ = std::fs::remove_file(&p);
    }
}
