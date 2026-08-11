//! Crash-safe journal for autonomous action decisions.
//!
//! A prepared record is written before dispatch. If the process dies after the
//! kernel call but before completion is recorded, recovery sees the in-flight
//! record and refuses an automatic redispatch until a policy resolves it.

use crate::autonomy::{ActionOutcome, TickAction};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JournalState {
    Prepared,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionJournalRecord {
    pub id: Uuid,
    pub action: TickAction,
    pub state: JournalState,
    pub outcome: Option<ActionOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeginAction {
    New(Uuid),
    InFlight(Uuid),
    Completed(Uuid),
}

pub trait ActionJournal: Send + Sync {
    fn begin(&self, action: &TickAction) -> Result<BeginAction, String>;
    fn complete(&self, id: Uuid, outcome: &ActionOutcome) -> Result<(), String>;
    fn records(&self) -> Result<Vec<ActionJournalRecord>, String>;
}

/// Atomic JSON journal. The rename makes each record-set replacement visible
/// as one state to a new process; prepared records are intentionally retained.
pub struct JsonActionJournal {
    path: PathBuf,
    lock: Mutex<()>,
}

impl JsonActionJournal {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    fn read(&self) -> Result<Vec<ActionJournalRecord>, String> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let bytes = fs::read(&self.path).map_err(|e| e.to_string())?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    fn write(&self, records: &[ActionJournalRecord]) -> Result<(), String> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temp = self.path.with_extension(format!("tmp-{}", Uuid::new_v4()));
        let bytes = serde_json::to_vec_pretty(records).map_err(|e| e.to_string())?;
        fs::write(&temp, bytes).map_err(|e| e.to_string())?;
        fs::rename(&temp, &self.path).map_err(|e| e.to_string())
    }
}

impl ActionJournal for JsonActionJournal {
    fn begin(&self, action: &TickAction) -> Result<BeginAction, String> {
        let _guard = self.lock.lock().map_err(|_| "journal lock poisoned")?;
        let mut records = self.read()?;
        if let Some(record) = records.iter().find(|record| {
            record.action.action == action.action && record.action.evidence == action.evidence
        }) {
            return Ok(match record.state {
                JournalState::Prepared => BeginAction::InFlight(record.id),
                JournalState::Completed => BeginAction::Completed(record.id),
            });
        }
        let id = Uuid::new_v4();
        records.push(ActionJournalRecord {
            id,
            action: action.clone(),
            state: JournalState::Prepared,
            outcome: None,
        });
        self.write(&records)?;
        Ok(BeginAction::New(id))
    }

    fn complete(&self, id: Uuid, outcome: &ActionOutcome) -> Result<(), String> {
        let _guard = self.lock.lock().map_err(|_| "journal lock poisoned")?;
        let mut records = self.read()?;
        let record = records
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or_else(|| "journal record not found".to_string())?;
        record.state = JournalState::Completed;
        record.outcome = Some(outcome.clone());
        self.write(&records)
    }

    fn records(&self) -> Result<Vec<ActionJournalRecord>, String> {
        let _guard = self.lock.lock().map_err(|_| "journal lock poisoned")?;
        self.read()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Priority;

    fn action() -> TickAction {
        TickAction::new(
            "git.status",
            "inspect",
            vec![Uuid::new_v4()],
            Priority::High,
        )
    }

    #[test]
    fn prepared_records_survive_reopen_and_block_redispatch() {
        let path = std::env::temp_dir().join(format!("friday-journal-{}.json", Uuid::new_v4()));
        let action = action();
        let journal = JsonActionJournal::new(&path);
        let id = match journal.begin(&action).unwrap() {
            BeginAction::New(id) => id,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(journal.begin(&action).unwrap(), BeginAction::InFlight(id));
        let reopened = JsonActionJournal::new(&path);
        assert_eq!(reopened.begin(&action).unwrap(), BeginAction::InFlight(id));
        let _ = fs::remove_file(path);
    }
}
