//! Timeline-aware Reflection: produces insights from execution history.
//!
//! Per ADR-0008 invariant 5: **Reflection never edits history.** It reads
//! timeline entries and produces [`ReflectionRecord`] entries as separate
//! artifacts.
//!
//! This module extends the existing rule-based reflector with timeline-aware
//! analysis. The existing [`crate::execution::reflection::Reflector`] trait
//! handles orchestration-level decisions (retry/replan/abort); this module
//! handles Experience Layer insights (why something succeeded/failed, what
//! was produced, what patterns emerged).

use chrono::Utc;
use uuid::Uuid;

use super::timeline::TimelineStore;
use super::{ReflectionCategory, ReflectionRecord, TimelineEntry};

/// Analyze a single timeline entry and produce a reflection record.
pub fn reflect_on_entry(entry: &TimelineEntry) -> Option<ReflectionRecord> {
    match entry.event_kind.as_str() {
        "exec.result" => reflect_on_result(entry),
        "artifact.created" => reflect_on_artifact(entry),
        "git.commit.created" => reflect_on_git_commit(entry),
        "git.branch.changed" => reflect_on_git_branch(entry),
        _ => None,
    }
}

/// Analyze all entries for a goal and produce reflection records. Appends them
/// to the store without modifying the timeline.
pub fn reflect_on_goal(
    store: &TimelineStore,
    goal_id: Uuid,
) -> anyhow::Result<Vec<ReflectionRecord>> {
    let entries = store.entries_for_goal(goal_id)?;
    let mut records = Vec::new();

    for entry in &entries {
        if let Some(record) = reflect_on_entry(entry) {
            store.save_reflection(&record)?;
            records.push(record);
        }
    }

    Ok(records)
}

fn reflect_on_result(entry: &TimelineEntry) -> Option<ReflectionRecord> {
    let success = entry
        .metadata
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let error = entry.metadata.get("error").and_then(|v| v.as_str());

    let category = if success {
        ReflectionCategory::Success
    } else if error.map(|e| e.contains("permission")).unwrap_or(false) {
        ReflectionCategory::PermissionDenied
    } else {
        ReflectionCategory::Failure
    };

    let insight = if success {
        format!(
            "Action '{}' succeeded via {}",
            entry.capability,
            entry
                .metadata
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown"),
        )
    } else if let Some(err) = error {
        format!("Action '{}' failed: {}", entry.capability, err,)
    } else {
        format!("Action '{}' failed with unknown error", entry.capability)
    };

    Some(ReflectionRecord {
        id: Uuid::new_v4(),
        timeline_entry_id: entry.id,
        insight,
        category,
        timestamp: Utc::now(),
    })
}

fn reflect_on_artifact(entry: &TimelineEntry) -> Option<ReflectionRecord> {
    let path = entry
        .metadata
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    Some(ReflectionRecord {
        id: Uuid::new_v4(),
        timeline_entry_id: entry.id,
        insight: format!("Artifact produced: {}", path),
        category: ReflectionCategory::ArtifactProduced,
        timestamp: Utc::now(),
    })
}

fn reflect_on_git_commit(entry: &TimelineEntry) -> Option<ReflectionRecord> {
    let hash = entry
        .metadata
        .get("commit_hash")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    Some(ReflectionRecord {
        id: Uuid::new_v4(),
        timeline_entry_id: entry.id,
        insight: format!("Git commit created: {}", hash),
        category: ReflectionCategory::GitOperation,
        timestamp: Utc::now(),
    })
}

fn reflect_on_git_branch(entry: &TimelineEntry) -> Option<ReflectionRecord> {
    let branch = entry
        .metadata
        .get("branch")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    Some(ReflectionRecord {
        id: Uuid::new_v4(),
        timeline_entry_id: entry.id,
        insight: format!("Git branch changed to: {}", branch),
        category: ReflectionCategory::GitOperation,
        timestamp: Utc::now(),
    })
}
