//! Who may call the helper's mutating methods.
//!
//! Any local process can send a message to a system-bus service, and the
//! polkit prompt that follows would say FRIDAY wants something. So the helper
//! first checks what the caller is: the program behind the caller's bus
//! connection must be one of FRIDAY's own. Polkit then asks the person.

use async_trait::async_trait;

/// The programs allowed to call by default: the FRIDAY daemon.
pub const DEFAULT_ALLOWED: &[&str] = &["/usr/bin/friday-linux-daemon"];

/// Extra allowed programs from `FRIDAY_HELPER_ALLOW_EXE` (colon-separated).
/// It is read from the service's own environment, which only root sets
/// (a systemd drop-in); it exists for testing the service.
pub fn allowed_from_env(env: Option<&str>) -> Vec<String> {
    let mut allowed: Vec<String> = DEFAULT_ALLOWED.iter().map(|s| s.to_string()).collect();
    if let Some(extra) = env {
        allowed.extend(
            extra
                .split(':')
                .map(str::trim)
                .filter(|p| p.starts_with('/'))
                .map(str::to_string),
        );
    }
    allowed
}

/// Whether `exe` (a /proc/<pid>/exe link) is one of `allowed`, exactly. A
/// program replaced by an update shows " (deleted)" after its path while it
/// still runs: that is still the same program.
pub fn exe_allowed(exe: &str, allowed: &[String]) -> bool {
    let exe = exe.strip_suffix(" (deleted)").unwrap_or(exe);
    allowed.iter().any(|a| a == exe)
}

/// Decides whether the program behind a bus connection may call.
#[async_trait]
pub trait CallerCheck: Send + Sync {
    /// `Ok(())` when `sender` (a unique bus name) may call; the error says why not.
    async fn check(&self, sender: &str) -> Result<(), String>;
}

/// Asks the bus for the caller's process, and reads the program it runs.
pub struct DbusCallers {
    conn: zbus::Connection,
    allowed: Vec<String>,
}

impl DbusCallers {
    pub fn new(conn: zbus::Connection, allowed: Vec<String>) -> Self {
        Self { conn, allowed }
    }
}

#[async_trait]
impl CallerCheck for DbusCallers {
    async fn check(&self, sender: &str) -> Result<(), String> {
        let reply = self
            .conn
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "GetConnectionUnixProcessID",
                &(sender,),
            )
            .await
            .map_err(|e| format!("couldn't tell who is calling: {e}"))?;
        let pid: u32 = reply
            .body()
            .deserialize()
            .map_err(|e| format!("couldn't tell who is calling: {e}"))?;
        let exe = std::fs::read_link(format!("/proc/{pid}/exe"))
            .map_err(|e| format!("couldn't tell what program is calling: {e}"))?;
        if exe_allowed(&exe.to_string_lossy(), &self.allowed) {
            Ok(())
        } else {
            Err("only FRIDAY itself may ask for this".to_string())
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A stand-in: allows everyone or no one, and counts the questions.
    pub struct StubCallers {
        allow: bool,
        asked: Mutex<usize>,
    }

    impl StubCallers {
        pub fn allow() -> Arc<Self> {
            Arc::new(Self {
                allow: true,
                asked: Mutex::new(0),
            })
        }
        pub fn deny() -> Arc<Self> {
            Arc::new(Self {
                allow: false,
                asked: Mutex::new(0),
            })
        }
        pub fn asked(&self) -> usize {
            *self.asked.lock().unwrap()
        }
    }

    #[async_trait]
    impl CallerCheck for StubCallers {
        async fn check(&self, _sender: &str) -> Result<(), String> {
            *self.asked.lock().unwrap() += 1;
            if self.allow {
                Ok(())
            } else {
                Err("only FRIDAY itself may ask for this".to_string())
            }
        }
    }

    #[test]
    fn only_listed_programs_are_allowed_exactly() {
        let allowed = allowed_from_env(None);
        assert!(exe_allowed("/usr/bin/friday-linux-daemon", &allowed));
        // Replaced by an update while running: still it.
        assert!(exe_allowed(
            "/usr/bin/friday-linux-daemon (deleted)",
            &allowed
        ));
        for no in [
            "/usr/bin/bash",
            "/usr/bin/busctl",
            "/tmp/friday-linux-daemon",
            "/usr/bin/friday-linux-daemon-evil",
            "/usr/bin/friday-linux-daemon/../bash",
            "friday-linux-daemon",
            "",
        ] {
            assert!(!exe_allowed(no, &allowed), "{no:?} was allowed");
        }
    }

    #[test]
    fn the_environment_adds_absolute_paths_only() {
        let allowed = allowed_from_env(Some("/usr/bin/busctl: /opt/x/y ;relative:bad"));
        assert!(exe_allowed("/usr/bin/busctl", &allowed));
        assert!(exe_allowed("/opt/x/y ;relative", &allowed));
        assert!(!exe_allowed("relative", &allowed));
        assert!(
            exe_allowed("/usr/bin/friday-linux-daemon", &allowed),
            "the default stays"
        );
    }
}
