//! FRIDAY's browser extension and its native host (`friday-ctl browser-host`,
//! started by the browser itself through native messaging): the extension
//! keeps a pipe to the host open, and the host answers FRIDAY's "what tab am
//! I on" on a socket in the user's runtime folder (`friday/browser.sock`,
//! owner-only; any program running as the same user could ask too, as it
//! could read the browser's profile anyway). No port: nothing from the network or a web page can reach
//! it. `friday-ctl browser tab` asks: the tab's title, address, selection and
//! text, read only when asked and never from a private window.

use std::io::{Read, Write};
use std::path::PathBuf;

use serde_json::Value;

/// The socket the host listens on.
/// The socket the host listens on: in a folder only this user can enter,
/// inside the user's runtime folder (never /tmp: no runtime folder, no socket).
pub fn socket() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(|d| PathBuf::from(d).join("friday").join("browser.sock"))
}

/// One native-messaging message: a 4-byte length (native byte order) and
/// that much JSON. None at the end of the stream or on a malformed message.
pub fn read_message(r: &mut impl Read) -> Option<Value> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).ok()?;
    let len = u32::from_ne_bytes(len) as usize;
    // The browser caps messages to the host at 4 GB; FRIDAY's are small.
    if len > 8 * 1024 * 1024 {
        return None;
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

pub fn write_message(w: &mut impl Write, msg: &Value) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(msg)?;
    w.write_all(&(bytes.len() as u32).to_ne_bytes())?;
    w.write_all(&bytes)?;
    w.flush()
}

/// The host: relays each "tab" asked on the socket to the extension and its
/// answer back. Ends when the browser closes the pipe.
#[cfg(unix)]
pub fn run_host() -> u8 {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};

    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let Some(path) = socket() else {
        return 1;
    };
    // The folder is owner-only before the socket exists, so there is no
    // moment when anyone else could reach it.
    let dir = path.parent().expect("socket has a folder");
    let _ = std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir);
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else {
        return 1;
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));

    let waiting: Arc<Mutex<HashMap<u64, mpsc::Sender<Value>>>> = Arc::default();
    let out = Arc::new(Mutex::new(std::io::stdout()));
    {
        // The extension's answers, matched to whoever asked.
        let waiting = waiting.clone();
        let path = path.clone();
        std::thread::spawn(move || {
            let mut input = std::io::stdin().lock();
            while let Some(msg) = read_message(&mut input) {
                let id = msg.get("id").and_then(Value::as_u64).unwrap_or(0);
                if let Some(tx) = waiting.lock().unwrap().remove(&id) {
                    let _ = tx.send(msg);
                }
            }
            // The browser went away: so does the host.
            let _ = std::fs::remove_file(&path);
            std::process::exit(0);
        });
    }
    let mut next = 1u64;
    for conn in listener.incoming() {
        let Ok(mut conn) = conn else { continue };
        // A client that never says anything can't hold the host up.
        let _ = conn.set_read_timeout(Some(std::time::Duration::from_secs(2)));
        let mut line = String::new();
        if BufReader::new(&conn).read_line(&mut line).is_err() || line.trim() != "tab" {
            continue;
        }
        let id = next;
        next += 1;
        let (tx, rx) = mpsc::channel();
        waiting.lock().unwrap().insert(id, tx);
        let asked = write_message(
            &mut *out.lock().unwrap(),
            &serde_json::json!({"type": "tab", "id": id}),
        );
        let reply = asked
            .ok()
            .and_then(|_| rx.recv_timeout(std::time::Duration::from_secs(5)).ok())
            .unwrap_or_else(|| serde_json::json!({"error": "the browser didn't answer"}));
        waiting.lock().unwrap().remove(&id);
        let _ = writeln!(conn, "{reply}");
    }
    0
}

/// Asks the host for the tab in front: its answer, or why there is none.
#[cfg(unix)]
pub fn tab() -> Result<Value, String> {
    tab_at(&socket().ok_or("no runtime folder (XDG_RUNTIME_DIR)")?)
}

#[cfg(unix)]
fn tab_at(path: &std::path::Path) -> Result<Value, String> {
    use std::io::{BufRead, BufReader};
    let mut conn = std::os::unix::net::UnixStream::connect(path).map_err(|_| {
        "FRIDAY's browser extension isn't connected (friday browser setup)".to_string()
    })?;
    let _ = conn.set_read_timeout(Some(std::time::Duration::from_secs(8)));
    writeln!(conn, "tab").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(conn)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let v: Value =
        serde_json::from_str(&line).map_err(|_| "no answer from the browser".to_string())?;
    match v.get("error").and_then(Value::as_str) {
        Some(e) => Err(e.to_string()),
        None => Ok(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_messages_are_length_prefixed_json() {
        let mut buf = Vec::new();
        write_message(&mut buf, &json!({"type": "tab", "id": 7})).unwrap();
        assert_eq!(
            u32::from_ne_bytes(buf[..4].try_into().unwrap()) as usize,
            buf.len() - 4
        );
        let back = read_message(&mut std::io::Cursor::new(buf)).unwrap();
        assert_eq!(back, json!({"type": "tab", "id": 7}));
        // The end of the stream, a cut-off message and an absurd length end it.
        assert!(read_message(&mut std::io::Cursor::new(Vec::new())).is_none());
        assert!(read_message(&mut std::io::Cursor::new(vec![10, 0, 0, 0, b'{'])).is_none());
        assert!(read_message(&mut std::io::Cursor::new(u32::MAX.to_ne_bytes().to_vec())).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn the_host_relays_a_tab_question_to_the_extension_and_back() {
        // The real host as a child, with this test playing the browser.
        let dir = std::env::temp_dir().join(format!("friday-bh-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = std::env::current_exe().unwrap();
        let mut host = std::process::Command::new(exe)
            .args([
                "browser_host::tests::host_child",
                "--exact",
                "--nocapture",
                "--ignored",
            ])
            .env("XDG_RUNTIME_DIR", &dir)
            .env("FRIDAY_BH_CHILD", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let sock = dir.join("friday").join("browser.sock");
        for _ in 0..100 {
            if sock.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let asker = std::thread::spawn({
            let sock = sock.clone();
            move || tab_at(&sock)
        });
        let mut from_host = host.stdout.take().unwrap();
        // The test harness prints a line or two first; skip to the message.
        let mut skipped = Vec::new();
        let msg = loop {
            let mut b = [0u8; 1];
            from_host.read_exact(&mut b).unwrap();
            skipped.push(b[0]);
            if skipped.ends_with(b"\x01FRIDAY-HOST\n") {
                break read_message(&mut from_host).unwrap();
            }
        };
        assert_eq!(msg["type"], "tab");
        let mut to_host = host.stdin.take().unwrap();
        write_message(&mut to_host, &json!({"id": msg["id"], "title": "Rust", "url": "https://example.com/", "text": "hello"})).unwrap();
        let got = asker.join().unwrap().unwrap();
        assert_eq!(got["url"], "https://example.com/");
        drop(to_host);
        let _ = host.wait();
        assert!(!sock.exists(), "the host left its socket behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    #[ignore]
    fn host_child() {
        if std::env::var_os("FRIDAY_BH_CHILD").is_none() {
            return;
        }
        // A marker so the parent knows where the host's own output starts.
        println!("\x01FRIDAY-HOST");
        std::io::stdout().flush().unwrap();
        std::process::exit(i32::from(run_host()));
    }
}
