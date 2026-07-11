//! Lifecycle management: one state machine for tasks, sessions, resources, and
//! workflows.
//!
//! Centralising lifecycle transitions prevents plugins from inventing their own
//! ad-hoc states and makes background jobs (M9) observable from day one. The
//! manager is the single authority on *whether* a given transition is legal.

use std::collections::HashMap;
use std::sync::Mutex;

use uuid::Uuid;

/// The shared lifecycle for every long-lived entity in the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    Created,
    Waiting,
    Running,
    Paused,
    Cancelled,
    Completed,
    Archived,
}

impl LifecycleState {
    /// States a transition *from* `self` may legally target.
    pub fn next_allowed(&self) -> &'static [LifecycleState] {
        match self {
            LifecycleState::Created => &[
                LifecycleState::Waiting,
                LifecycleState::Running,
                LifecycleState::Cancelled,
            ],
            LifecycleState::Waiting => &[LifecycleState::Running, LifecycleState::Cancelled],
            LifecycleState::Running => &[
                LifecycleState::Paused,
                LifecycleState::Completed,
                LifecycleState::Cancelled,
            ],
            LifecycleState::Paused => &[LifecycleState::Running, LifecycleState::Cancelled],
            LifecycleState::Cancelled => &[LifecycleState::Archived],
            LifecycleState::Completed => &[LifecycleState::Archived],
            LifecycleState::Archived => &[],
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            LifecycleState::Created => "created",
            LifecycleState::Waiting => "waiting",
            LifecycleState::Running => "running",
            LifecycleState::Paused => "paused",
            LifecycleState::Cancelled => "cancelled",
            LifecycleState::Completed => "completed",
            LifecycleState::Archived => "archived",
        }
    }
}

/// Error returned when a transition is not legal from the current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleError {
    pub from: LifecycleState,
    pub to: LifecycleState,
    pub message: String,
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "illegal lifecycle transition {} -> {}: {}",
            self.from.as_str(),
            self.to.as_str(),
            self.message
        )
    }
}

impl std::error::Error for LifecycleError {}

/// Tracks the lifecycle state of any entity addressed by a [`Uuid`].
#[derive(Default)]
pub struct LifecycleManager {
    states: Mutex<HashMap<Uuid, LifecycleState>>,
}

impl LifecycleManager {
    pub fn new() -> Self {
        Self {
            states: Mutex::new(HashMap::new()),
        }
    }

    /// Register a new entity in the `Created` state.
    pub fn create(&self, id: Uuid) -> LifecycleState {
        let mut states = self.states.lock().expect("lifecycle poisoned");
        states.insert(id, LifecycleState::Created);
        LifecycleState::Created
    }

    /// Transition `id` to `to`, enforcing the legal-transition table. Creating
    /// the entity on first use if it is unknown.
    pub fn transition(
        &self,
        id: Uuid,
        to: LifecycleState,
    ) -> Result<LifecycleState, LifecycleError> {
        let mut states = self.states.lock().expect("lifecycle poisoned");
        let from = *states.entry(id).or_insert(LifecycleState::Created);
        if !from.next_allowed().contains(&to) {
            return Err(LifecycleError {
                from,
                to,
                message: format!("cannot move from {} to {}", from.as_str(), to.as_str()),
            });
        }
        states.insert(id, to);
        Ok(to)
    }

    pub fn state(&self, id: Uuid) -> Option<LifecycleState> {
        let states = self.states.lock().expect("lifecycle poisoned");
        states.get(&id).copied()
    }

    /// Force every still-active entity into `Archived` (used on shutdown).
    pub fn archive_all(&self) {
        let mut states = self.states.lock().expect("lifecycle poisoned");
        for s in states.values_mut() {
            if *s != LifecycleState::Archived {
                *s = LifecycleState::Archived;
            }
        }
    }

    pub fn count(&self) -> usize {
        self.states.lock().expect("lifecycle poisoned").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn created_to_running_to_completed() {
        let mgr = LifecycleManager::new();
        let id = Uuid::new_v4();
        assert_eq!(mgr.create(id), LifecycleState::Created);
        assert_eq!(
            mgr.transition(id, LifecycleState::Running).unwrap(),
            LifecycleState::Running
        );
        assert_eq!(
            mgr.transition(id, LifecycleState::Completed).unwrap(),
            LifecycleState::Completed
        );
        assert_eq!(
            mgr.transition(id, LifecycleState::Archived).unwrap(),
            LifecycleState::Archived
        );
    }

    #[test]
    fn illegal_transition_rejected() {
        let mgr = LifecycleManager::new();
        let id = Uuid::new_v4();
        mgr.create(id);
        // Created -> Completed is not allowed directly.
        let err = mgr.transition(id, LifecycleState::Completed).unwrap_err();
        assert_eq!(err.from, LifecycleState::Created);
        assert_eq!(err.to, LifecycleState::Completed);
    }

    #[test]
    fn archived_is_terminal() {
        let mgr = LifecycleManager::new();
        let id = Uuid::new_v4();
        mgr.create(id);
        mgr.transition(id, LifecycleState::Running).unwrap();
        mgr.transition(id, LifecycleState::Cancelled).unwrap();
        mgr.transition(id, LifecycleState::Archived).unwrap();
        assert!(mgr.transition(id, LifecycleState::Running).is_err());
    }
}
