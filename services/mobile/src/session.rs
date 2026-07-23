//! Per-device session state for the mobile console (M6.2a).
//!
//! The phone is a thin client; everything FRIDAY knows about a connected device
//! lives here. The mobile service is **session-centric**: auth, subscriptions,
//! pending approvals and active tasks are all scoped to a [`MobileSession`].

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// What a physical phone is capable of. Captured at pairing time so FRIDAY can
/// route device-specific features (voice needs a microphone, vision needs a camera)
/// without the launcher ever understanding FRIDAY internals.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct PhoneCapabilities {
    pub voice: bool,
    pub camera: bool,
    pub notifications: bool,
    pub speaker: bool,
    pub microphone: bool,
}

impl PhoneCapabilities {
    /// A typical modern Android phone (e.g. the Samsung J7) exposes all of these.
    pub fn full() -> Self {
        Self {
            voice: true,
            camera: true,
            notifications: true,
            speaker: true,
            microphone: true,
        }
    }
}

/// One authenticated (or authenticating) phone connection.
#[derive(Debug, Clone)]
pub struct MobileSession {
    pub session_id: String,
    /// Stable device identifier (set from the client `Hello`).
    pub phone_id: String,
    pub device: String,
    pub model: String,
    pub app_version: String,
    pub authenticated: bool,
    pub connected: bool,
    pub capabilities: PhoneCapabilities,
    /// Event topics this session asked to receive (`Subscribe`). Empty = all.
    pub subscribed_events: HashSet<String>,
    /// Task/action ids awaiting an in-app approval dialog.
    pub pending_approvals: Vec<String>,
    /// Task ids currently executing on behalf of this device.
    pub active_tasks: HashSet<String>,
}

impl MobileSession {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            phone_id: String::new(),
            device: String::new(),
            model: String::new(),
            app_version: String::new(),
            authenticated: false,
            connected: true,
            capabilities: PhoneCapabilities::full(),
            subscribed_events: HashSet::new(),
            pending_approvals: Vec::new(),
            active_tasks: HashSet::new(),
        }
    }
}

/// Thread-safe registry of live mobile sessions, keyed by `session_id`.
#[derive(Debug, Default)]
pub struct SessionRegistry {
    sessions: Mutex<HashMap<String, MobileSession>>,
}

impl SessionRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            sessions: Mutex::new(HashMap::new()),
        })
    }

    pub fn insert(&self, session: MobileSession) {
        self.sessions
            .lock()
            .unwrap()
            .insert(session.session_id.clone(), session);
    }

    pub fn get(&self, session_id: &str) -> Option<MobileSession> {
        self.sessions.lock().unwrap().get(session_id).cloned()
    }

    /// Apply a mutation to a session, if present.
    pub fn update(&self, session_id: &str, f: impl FnOnce(&mut MobileSession)) {
        let mut guard = self.sessions.lock().unwrap();
        if let Some(s) = guard.get_mut(session_id) {
            f(s);
        }
    }

    pub fn remove(&self, session_id: &str) {
        self.sessions.lock().unwrap().remove(session_id);
    }

    pub fn count(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_lifecycle() {
        let reg = SessionRegistry::new();
        let sid = "sess-1".to_string();
        reg.insert(MobileSession::new(sid.clone()));

        reg.update(&sid, |s| {
            s.authenticated = true;
            s.phone_id = "j7".into();
            s.active_tasks.insert("t1".into());
        });
        let s = reg.get(&sid).unwrap();
        assert!(s.authenticated);
        assert_eq!(s.phone_id, "j7");
        assert!(s.active_tasks.contains("t1"));

        reg.remove(&sid);
        assert!(reg.get(&sid).is_none());
    }

    #[test]
    fn full_capabilities_by_default() {
        let s = MobileSession::new("x".into());
        assert!(s.capabilities.voice && s.capabilities.camera);
    }
}
