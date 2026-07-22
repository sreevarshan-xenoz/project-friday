//! Experience Layer integration tests: Timeline, Recorder, Reflection.
//!
//! These are hermetic (no external processes) and verify the ADR-0008 invariants:
//! 1. Append-only timeline (no updates, no deletes)
//! 2. Experience stores semantics (capability identifiers)
//! 3. Reflection produces separate records (never edits timeline)
//! 4. Recorder derives workflows from completed goals

use uuid::Uuid;

use friday_planner::experience::TimelineStore;
use friday_planner::experience::recorder::{extract_workflow, record_workflow};
use friday_planner::experience::reflection::reflect_on_goal;

fn temp_store() -> (TimelineStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timeline-test.redb");
    let store = TimelineStore::open(&path).unwrap();
    (store, dir)
}

#[test]
fn timeline_append_only() {
    let (store, _dir) = temp_store();
    let goal_id = Uuid::new_v4();
    let plan_id = Uuid::new_v4();
    let exec_id = Uuid::new_v4();

    // Append three entries.
    let e1 = store
        .record(goal_id, plan_id, exec_id, "exec.action", "filesystem.write", serde_json::json!({"path": "/tmp/a"}), None)
        .unwrap();
    let e2 = store
        .record(goal_id, plan_id, exec_id, "exec.result", "filesystem.write", serde_json::json!({"success": true}), Some(e1.id))
        .unwrap();
    store
        .record(goal_id, plan_id, exec_id, "artifact.created", "filesystem.write", serde_json::json!({"path": "/tmp/a"}), Some(e2.id))
        .unwrap();

    // All three are retrievable in order.
    let entries = store.entries_for_goal(goal_id).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].id, e1.id);
    assert_eq!(entries[1].parent, Some(e1.id));
    assert_eq!(entries[2].parent, Some(e2.id));

    // Count works.
    assert_eq!(store.count_for_goal(goal_id).unwrap(), 3);

    // Different goal_id returns empty.
    let other = store.entries_for_goal(Uuid::new_v4()).unwrap();
    assert!(other.is_empty());
}

#[test]
fn timeline_stores_semantics() {
    let (store, _dir) = temp_store();
    let goal_id = Uuid::new_v4();

    let entry = store
        .record(goal_id, Uuid::new_v4(), Uuid::new_v4(), "exec.action", "git.commit", serde_json::json!({"message": "test"}), None)
        .unwrap();

    // The capability is the semantic identifier, not a raw UI event.
    assert_eq!(entry.capability, "git.commit");
    assert_eq!(entry.event_kind, "exec.action");

    let loaded = store.entries_for_goal(goal_id).unwrap();
    assert_eq!(loaded[0].capability, "git.commit");
}

#[test]
fn recorder_extracts_workflow() {
    let (store, _dir) = temp_store();
    let goal_id = Uuid::new_v4();
    let plan_id = Uuid::new_v4();
    let exec_id = Uuid::new_v4();

    // Simulate a successful goal: write → commit.
    store.record(goal_id, plan_id, exec_id, "exec.action", "mcp:filesystem.write_file", serde_json::json!({}), None).unwrap();
    store.record(goal_id, plan_id, exec_id, "exec.result", "mcp:filesystem.write_file", serde_json::json!({"success": true}), None).unwrap();
    store.record(goal_id, plan_id, exec_id, "exec.action", "mcp:git.git_commit", serde_json::json!({}), None).unwrap();
    store.record(goal_id, plan_id, exec_id, "exec.result", "mcp:git.git_commit", serde_json::json!({"success": true}), None).unwrap();
    store.record(goal_id, plan_id, exec_id, "goal.completed", "", serde_json::json!({}), None).unwrap();

    let workflow = extract_workflow(&store, goal_id, "test-goal").unwrap().unwrap();

    // Actions are the short form (after the colon).
    assert_eq!(workflow.actions, vec!["filesystem.write_file", "git.git_commit"]);
    // Capabilities are the full form.
    assert_eq!(workflow.capabilities, vec!["mcp:filesystem.write_file", "mcp:git.git_commit"]);
    assert_eq!(workflow.source_goal_id, goal_id);
    assert!(workflow.name.contains("test-goal"));
}

#[test]
fn recorder_deduplicates_consecutive() {
    let (store, _dir) = temp_store();
    let goal_id = Uuid::new_v4();

    // Two consecutive identical capabilities should be deduplicated.
    store.record(goal_id, Uuid::new_v4(), Uuid::new_v4(), "exec.action", "filesystem.write", serde_json::json!({}), None).unwrap();
    store.record(goal_id, Uuid::new_v4(), Uuid::new_v4(), "exec.result", "filesystem.write", serde_json::json!({}), None).unwrap();
    store.record(goal_id, Uuid::new_v4(), Uuid::new_v4(), "exec.action", "filesystem.write", serde_json::json!({}), None).unwrap();
    store.record(goal_id, Uuid::new_v4(), Uuid::new_v4(), "exec.result", "filesystem.write", serde_json::json!({}), None).unwrap();

    let workflow = extract_workflow(&store, goal_id, "dupes").unwrap().unwrap();
    assert_eq!(workflow.actions, vec!["filesystem.write"]);
}

#[test]
fn recorder_persists_workflow() {
    let (store, _dir) = temp_store();
    let goal_id = Uuid::new_v4();

    store.record(goal_id, Uuid::new_v4(), Uuid::new_v4(), "exec.action", "git.commit", serde_json::json!({}), None).unwrap();
    store.record(goal_id, Uuid::new_v4(), Uuid::new_v4(), "exec.result", "git.commit", serde_json::json!({}), None).unwrap();

    let workflow = record_workflow(&store, goal_id, "persist-test").unwrap().unwrap();

    // Reload from disk.
    let loaded = store.load_workflows().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, workflow.id);
    assert_eq!(loaded[0].actions, vec!["git.commit"]);
}

#[test]
fn reflection_produces_records_not_edits() {
    let (store, _dir) = temp_store();
    let goal_id = Uuid::new_v4();
    let plan_id = Uuid::new_v4();
    let exec_id = Uuid::new_v4();

    let entry = store
        .record(goal_id, plan_id, exec_id, "exec.result", "filesystem.write", serde_json::json!({"success": true, "source": "mcp"}), None)
        .unwrap();

    // Run reflection.
    let records = reflect_on_goal(&store, goal_id).unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].insight.contains("succeeded"));
    assert_eq!(records[0].timeline_entry_id, entry.id);

    // The timeline entry was NOT modified.
    let entries = store.entries_for_goal(goal_id).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, entry.id);

    // Reflection records are stored separately.
    let loaded = store.load_reflections().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, records[0].id);
}

#[test]
fn reflection_categorizes_outcomes() {
    let (store, _dir) = temp_store();
    let goal_id = Uuid::new_v4();

    // Success.
    store.record(goal_id, Uuid::new_v4(), Uuid::new_v4(), "exec.result", "fs.write", serde_json::json!({"success": true, "source": "mcp"}), None).unwrap();
    // Failure.
    store.record(goal_id, Uuid::new_v4(), Uuid::new_v4(), "exec.result", "fs.write", serde_json::json!({"success": false, "error": "file not found"}), None).unwrap();
    // Permission denied.
    store.record(goal_id, Uuid::new_v4(), Uuid::new_v4(), "exec.result", "fs.write", serde_json::json!({"success": false, "error": "permission denied by policy"}), None).unwrap();
    // Artifact.
    store.record(goal_id, Uuid::new_v4(), Uuid::new_v4(), "artifact.created", "fs.write", serde_json::json!({"path": "/tmp/x"}), None).unwrap();
    // Git commit.
    store.record(goal_id, Uuid::new_v4(), Uuid::new_v4(), "git.commit.created", "git.commit", serde_json::json!({"commit_hash": "abc1234"}), None).unwrap();

    let records = reflect_on_goal(&store, goal_id).unwrap();
    assert_eq!(records.len(), 5);

    let categories: Vec<_> = records.iter().map(|r| r.category.clone()).collect();
    assert!(categories.contains(&friday_planner::experience::ReflectionCategory::Success));
    assert!(categories.contains(&friday_planner::experience::ReflectionCategory::Failure));
    assert!(categories.contains(&friday_planner::experience::ReflectionCategory::PermissionDenied));
    assert!(categories.contains(&friday_planner::experience::ReflectionCategory::ArtifactProduced));
    assert!(categories.contains(&friday_planner::experience::ReflectionCategory::GitOperation));
}

#[test]
fn empty_goal_produces_no_workflow() {
    let (store, _dir) = temp_store();
    let goal_id = Uuid::new_v4();

    // Only goal.completed, no actions.
    store.record(goal_id, Uuid::new_v4(), Uuid::new_v4(), "goal.completed", "", serde_json::json!({}), None).unwrap();

    let workflow = extract_workflow(&store, goal_id, "empty").unwrap();
    assert!(workflow.is_none());
}

#[test]
fn all_entries_returns_everything() {
    let (store, _dir) = temp_store();

    store.record(Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), "exec.action", "a", serde_json::json!({}), None).unwrap();
    store.record(Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), "exec.action", "b", serde_json::json!({}), None).unwrap();
    store.record(Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4(), "exec.action", "c", serde_json::json!({}), None).unwrap();

    let all = store.all_entries().unwrap();
    assert_eq!(all.len(), 3);
}
