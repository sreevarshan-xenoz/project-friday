use crate::workspace::WorkspaceEvent;
use chrono::Utc;
use std::path::{Path, PathBuf};

pub struct EventRecorder {
    base_path: PathBuf,
    current_file: Option<std::fs::File>,
    current_date: chrono::NaiveDate,
}

impl EventRecorder {
    pub fn new(base_path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let base_path = base_path.into();
        std::fs::create_dir_all(&base_path)?;

        let today = Utc::now().date_naive();
        let file_path = base_path.join(format!("{}.jsonl", today));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;

        Ok(Self {
            base_path,
            current_file: Some(file),
            current_date: today,
        })
    }

    pub fn append(&mut self, event: &WorkspaceEvent) -> std::io::Result<()> {
        self.rotate_if_needed()?;

        if let Some(ref mut file) = self.current_file {
            let mut line = serde_json::to_vec(event)?;
            line.push(b'\n');
            use std::io::Write;
            file.write_all(&line)?;
            file.flush()?;
        }

        Ok(())
    }

    fn rotate_if_needed(&mut self) -> std::io::Result<()> {
        let today = Utc::now().date_naive();
        if today != self.current_date {
            let file_path = self.base_path.join(format!("{}.jsonl", today));
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file_path)?;

            self.current_file = Some(file);
            self.current_date = today;
        }
        Ok(())
    }

    pub fn read_date(&self, date: chrono::NaiveDate) -> std::io::Result<Vec<WorkspaceEvent>> {
        let file_path = self.base_path.join(format!("{}.jsonl", date));
        self.read_file(&file_path)
    }

    pub fn read_recent(&self, n: usize) -> std::io::Result<Vec<WorkspaceEvent>> {
        let mut files = self.list_date_files()?;
        files.reverse();

        let events = files.first()
            .map(|f| self.read_file(f))
            .transpose()?
            .unwrap_or_default();

        if events.len() > n {
            let skip = events.len() - n;
            Ok(events.into_iter().skip(skip).collect())
        } else {
            Ok(events)
        }
    }

    fn read_file(&self, path: &Path) -> std::io::Result<Vec<WorkspaceEvent>> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let events = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        Ok(events)
    }

    fn list_date_files(&self) -> std::io::Result<Vec<PathBuf>> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&self.base_path)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .map(|ext| ext == "jsonl")
                    .unwrap_or(false)
            })
            .collect();
        files.sort();
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{EngineId, Priority, Topic};
    use serde_json::json;
    use uuid::Uuid;

    fn test_event(topic: Topic) -> WorkspaceEvent {
        WorkspaceEvent::with_full(
            Uuid::new_v4(),
            1690000000000,
            Uuid::nil(),
            topic,
            EngineId::Runtime,
            None,
            Priority::Normal,
            json!({"test": true}),
        )
    }

    #[test]
    fn append_and_read_recent() {
        let dir = std::env::temp_dir().join(format!("friday_test_{}", Uuid::new_v4()));
        let mut recorder = EventRecorder::new(&dir).unwrap();

        for i in 0..5u64 {
            let mut event = test_event(Topic::Custom(format!("event.{}", i)));
            event.timestamp_ms = 1690000000000 + i;
            recorder.append(&event).unwrap();
        }

        let recent = recorder.read_recent(3).unwrap();
        assert_eq!(recent.len(), 3);
        let timestamps: Vec<u64> = recent.iter().map(|e| e.timestamp_ms).collect();
        assert_eq!(timestamps, vec![1690000000002, 1690000000003, 1690000000004]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_date_returns_empty_for_missing_file() {
        let dir = std::env::temp_dir().join(format!("friday_test_{}", Uuid::new_v4()));
        let recorder = EventRecorder::new(&dir).unwrap();

        let date = chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        let events = recorder.read_date(date).unwrap();
        assert!(events.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn jsonl_file_is_created() {
        let dir = std::env::temp_dir().join(format!("friday_test_{}", Uuid::new_v4()));
        let mut recorder = EventRecorder::new(&dir).unwrap();

        let event = test_event(Topic::TickCompleted);
        recorder.append(&event).unwrap();

        let today = Utc::now().date_naive();
        let file_path = dir.join(format!("{}.jsonl", today));
        assert!(file_path.exists());

        let content = std::fs::read_to_string(&file_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);

        let parsed: WorkspaceEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed.topic, Topic::TickCompleted);

        std::fs::remove_dir_all(&dir).ok();
    }
}
