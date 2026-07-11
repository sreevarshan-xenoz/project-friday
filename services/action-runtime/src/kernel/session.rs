//! Session management: the source of continuity between actions.
//!
//! A *session* is the logical unit a user thinks of as "my work" — a build
//! session, a research session, a coding session. It owns the working
//! directory, environment, shell history, temporary variables, and (later)
//! auth/cookie state. Because the session outlives any single action, FRIDAY
//! continues where it left off instead of starting cold every command. This is
//! distinct from [`crate::kernel::resource`], which owns *things* (processes,
//! handles); the session owns *state*.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::kernel::lifecycle::{LifecycleManager, LifecycleState};

/// One logical user session: the continuity surface for a related body of work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub name: String,
    pub cwd: PathBuf,
    pub environment: HashMap<String, String>,
    /// Shell/command history, in order.
    pub history: Vec<String>,
    /// Transient variables visible to actions in this session.
    pub variables: HashMap<String, Value>,
    /// Opaque auth/cookie state placeholders (filled in by later capabilities).
    pub auth: HashMap<String, String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Session {
    pub fn new(name: impl Into<String>, cwd: PathBuf) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            cwd,
            environment: std::env::vars().collect(),
            history: Vec::new(),
            variables: HashMap::new(),
            auth: HashMap::new(),
            created_at: chrono::Utc::now(),
        }
    }

    /// Record a command in history (bounded to the last 1000 entries).
    pub fn push_history(&mut self, command: impl Into<String>) {
        self.history.push(command.into());
        if self.history.len() > 1000 {
            let excess = self.history.len() - 1000;
            self.history.drain(0..excess);
        }
    }

    pub fn set_var(&mut self, k: impl Into<String>, v: Value) {
        self.variables.insert(k.into(), v);
    }

    pub fn get_var(&self, k: &str) -> Option<&Value> {
        self.variables.get(k)
    }
}

/// Owns all sessions and their lifecycle. Plugins ask the manager for a session
/// rather than stashing state themselves.
#[derive(Default)]
pub struct SessionManager {
    sessions: Mutex<HashMap<Uuid, Session>>,
    lifecycle: LifecycleManager,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            lifecycle: LifecycleManager::new(),
        }
    }

    /// Create a new session and move it to `Running`.
    pub fn create(&self, name: impl Into<String>, cwd: PathBuf) -> Uuid {
        let session = Session::new(name, cwd);
        let id = session.id;
        self.sessions
            .lock()
            .expect("session map poisoned")
            .insert(id, session);
        self.lifecycle.create(id);
        self.lifecycle.transition(id, LifecycleState::Running).ok();
        id
    }

    /// Get a clone of a session's current state.
    pub fn get(&self, id: Uuid) -> Option<Session> {
        self.sessions
            .lock()
            .expect("session map poisoned")
            .get(&id)
            .cloned()
    }

    /// Mutate a session in place (history, variables, env). Returns `false` if
    /// the session does not exist.
    pub fn with_session_mut(&self, id: Uuid, f: impl FnOnce(&mut Session)) -> bool {
        let mut guard = self.sessions.lock().expect("session map poisoned");
        if let Some(s) = guard.get_mut(&id) {
            f(s);
            true
        } else {
            false
        }
    }

    /// Archive a session and its lifecycle.
    pub fn close(&self, id: Uuid) {
        self.lifecycle.transition(id, LifecycleState::Archived).ok();
        self.sessions
            .lock()
            .expect("session map poisoned")
            .remove(&id);
    }

    pub fn list(&self) -> Vec<Uuid> {
        self.sessions
            .lock()
            .expect("session map poisoned")
            .keys()
            .copied()
            .collect()
    }

    pub fn count(&self) -> usize {
        self.sessions.lock().expect("session map poisoned").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_then_history_continues() {
        let mgr = SessionManager::new();
        let id = mgr.create("build", PathBuf::from("/work"));
        mgr.with_session_mut(id, |s| s.push_history("cargo build"));
        mgr.with_session_mut(id, |s| s.push_history("cargo test"));
        let s = mgr.get(id).unwrap();
        assert_eq!(s.history, vec!["cargo build", "cargo test"]);
        assert_eq!(s.cwd, PathBuf::from("/work"));
    }

    #[test]
    fn variables_persist_across_actions() {
        let mgr = SessionManager::new();
        let id = mgr.create("research", PathBuf::from("/tmp"));
        mgr.with_session_mut(id, |s| s.set_var("topic", Value::from("rust")));
        let v = mgr.get(id).unwrap().get_var("topic").cloned();
        assert_eq!(v, Some(Value::from("rust")));
    }

    #[test]
    fn close_removes_session() {
        let mgr = SessionManager::new();
        let id = mgr.create("ephemeral", PathBuf::from("/tmp"));
        assert_eq!(mgr.count(), 1);
        mgr.close(id);
        assert_eq!(mgr.count(), 0);
    }
}
