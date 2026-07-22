//! Experience Retrieval tests (M12.1 + refinements).
//!
//! Five success criteria:
//! 1. Have I done something similar before?
//! 2. What happened last time?
//! 3. Which workflow succeeded?
//! 4. Why did it succeed/fail?
//! 5. How strongly should I trust this?

use std::collections::HashMap;

use uuid::Uuid;

use friday_planner::experience::indexer::ExperienceIndexer;
use friday_planner::experience::service::ExperienceService;
use friday_planner::experience::{GoalOutcome, MatchReason, ReflectionRecord, TIMELINE_VERSION};
use friday_planner::experience::TimelineStore;

fn temp_store() -> (TimelineStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timeline-retrieval.redb");
    let store = TimelineStore::open(&path).unwrap();
    (store, dir)
}

fn make_entry(
    goal_id: Uuid,
    plan_id: Uuid,
    event_kind: &str,
    capability: &str,
    metadata: serde_json::Value,
) -> friday_planner::experience::TimelineEntry {
    friday_planner::experience::TimelineEntry {
        version: TIMELINE_VERSION,
        id: Uuid::new_v4(),
        timestamp: chrono::Utc::now(),
        goal_id,
        plan_id,
        execution_id: Uuid::new_v4(),
        event_kind: event_kind.to_string(),
        capability: capability.to_string(),
        metadata,
        parent: None,
        tags: vec![],
        annotations: HashMap::new(),
    }
}

// ---------------------------------------------------------------------------
// Criterion 1: Have I done something similar before?
// ---------------------------------------------------------------------------

#[test]
fn similar_finds_goals_with_matching_titles() {
    let (store, _dir) = temp_store();
    let goal_a = Uuid::new_v4();
    let plan_a = Uuid::new_v4();

    store.append(make_entry(goal_a, plan_a, "exec.action", "filesystem.write", serde_json::json!({"title": "write readme"}))).unwrap();
    store.append(make_entry(goal_a, plan_a, "exec.result", "filesystem.write", serde_json::json!({"success": true}))).unwrap();
    store.append(make_entry(goal_a, plan_a, "goal.completed", "", serde_json::json!({}))).unwrap();

    let goal_b = Uuid::new_v4();
    store.append(make_entry(goal_b, Uuid::new_v4(), "exec.action", "filesystem.write", serde_json::json!({"title": "write documentation"}))).unwrap();
    store.append(make_entry(goal_b, Uuid::new_v4(), "goal.completed", "", serde_json::json!({}))).unwrap();

    let mut svc = ExperienceService::new();
    svc.rebuild(&store.all_entries().unwrap(), &[], &[]);

    let matches = svc.similar("write readme", 10);
    assert!(!matches.is_empty(), "should find at least one match");
    assert!(matches[0].similarity_score > 0.6, "self-match should have reasonable similarity, got {}", matches[0].similarity_score);
    // trust_score should also be populated
    assert!(matches[0].trust_score > 0.0, "trust_score should be non-zero");
}

#[test]
fn similar_returns_empty_for_unknown_goal() {
    let mut svc = ExperienceService::new();
    svc.rebuild(&[], &[], &[]);
    let matches = svc.similar("completely unrelated goal xyz", 10);
    assert!(matches.is_empty());
}

// ---------------------------------------------------------------------------
// Criterion 2: What happened last time?
// ---------------------------------------------------------------------------

#[test]
fn last_attempt_returns_most_recent_for_goal() {
    let (store, _dir) = temp_store();
    let goal_id = Uuid::new_v4();

    // First attempt: failed.
    store.append(make_entry(goal_id, Uuid::new_v4(), "exec.result", "fs.write", serde_json::json!({"success": false, "error": "disk full"}))).unwrap();

    // Second attempt: succeeded.
    store.append(make_entry(goal_id, Uuid::new_v4(), "goal.completed", "", serde_json::json!({}))).unwrap();

    let mut svc = ExperienceService::new();
    svc.rebuild(&store.all_entries().unwrap(), &[], &[]);

    let last = svc.last_attempt(goal_id).expect("should have last attempt");
    assert_eq!(last.outcome, GoalOutcome::Success);
    assert_eq!(last.match_reason, MatchReason::RecentAttempt);
    assert_eq!(last.similarity_score, 1.0, "recency match is exact");
}

#[test]
fn last_attempt_returns_none_for_unknown_goal() {
    let svc = ExperienceService::new();
    assert!(svc.last_attempt(Uuid::new_v4()).is_none());
}

// ---------------------------------------------------------------------------
// Criterion 3: Which workflow succeeded?
// ---------------------------------------------------------------------------

#[test]
fn successful_workflows_filters_by_outcome_and_similarity() {
    let (store, _dir) = temp_store();

    // Goal A: filesystem workflow, succeeded.
    let goal_a = Uuid::new_v4();
    let plan_a = Uuid::new_v4();
    store.append(make_entry(goal_a, plan_a, "exec.action", "filesystem.write", serde_json::json!({"title": "write readme"}))).unwrap();
    store.append(make_entry(goal_a, plan_a, "exec.result", "filesystem.write", serde_json::json!({"success": true}))).unwrap();
    store.append(make_entry(goal_a, plan_a, "goal.completed", "", serde_json::json!({}))).unwrap();

    // Goal B: git workflow, failed.
    let goal_b = Uuid::new_v4();
    let plan_b = Uuid::new_v4();
    store.append(make_entry(goal_b, plan_b, "exec.action", "git.commit", serde_json::json!({"title": "commit code"}))).unwrap();
    store.append(make_entry(goal_b, plan_b, "exec.result", "git.commit", serde_json::json!({"success": false, "error": "conflict"}))).unwrap();

    let mut svc = ExperienceService::new();
    svc.rebuild(&store.all_entries().unwrap(), &[], &[]);

    let workflows = svc.successful_workflows("write readme");
    assert_eq!(workflows.len(), 1, "should only return successful goal");
    assert_eq!(workflows[0].goal_id.0, goal_a);
}

// ---------------------------------------------------------------------------
// Criterion 4: Why did it succeed/fail?
// ---------------------------------------------------------------------------

#[test]
fn explain_previous_outcome_returns_reflections() {
    let (store, _dir) = temp_store();
    let goal_id = Uuid::new_v4();
    let plan_id = Uuid::new_v4();

    let entry_id = store.append(make_entry(goal_id, plan_id, "exec.result", "filesystem.write", serde_json::json!({"success": true, "source": "mcp"}))).unwrap();
    store.append(make_entry(goal_id, plan_id, "goal.completed", "", serde_json::json!({}))).unwrap();

    let reflection = ReflectionRecord {
        id: Uuid::new_v4(),
        timeline_entry_id: entry_id,
        insight: "Action succeeded on first attempt".into(),
        category: friday_planner::experience::ReflectionCategory::Success,
        timestamp: chrono::Utc::now(),
    };
    store.save_reflection(&reflection).unwrap();

    let mut svc = ExperienceService::new();
    svc.rebuild(&store.all_entries().unwrap(), &[], &[reflection.clone()]);

    let (outcome, reflections) = svc
        .explain_previous_outcome(goal_id)
        .expect("should have outcome explanation");
    assert_eq!(outcome, GoalOutcome::Success);
    assert_eq!(reflections.len(), 1);
    assert_eq!(reflections[0].insight, "Action succeeded on first attempt");
}

#[test]
fn explain_previous_outcome_for_failed_goal() {
    let (store, _dir) = temp_store();
    let goal_id = Uuid::new_v4();
    let plan_id = Uuid::new_v4();

    let entry_id = store.append(make_entry(goal_id, plan_id, "exec.result", "fs.write", serde_json::json!({"success": false, "error": "permission denied by policy"}))).unwrap();

    let reflection = ReflectionRecord {
        id: Uuid::new_v4(),
        timeline_entry_id: entry_id,
        insight: "Permission denied by policy".into(),
        category: friday_planner::experience::ReflectionCategory::PermissionDenied,
        timestamp: chrono::Utc::now(),
    };
    store.save_reflection(&reflection).unwrap();

    let mut svc = ExperienceService::new();
    svc.rebuild(&store.all_entries().unwrap(), &[], &[reflection]);

    let (outcome, reflections) = svc
        .explain_previous_outcome(goal_id)
        .expect("should have explanation");
    assert_eq!(outcome, GoalOutcome::Failure);
    assert!(!reflections.is_empty());
    assert_eq!(reflections[0].category, friday_planner::experience::ReflectionCategory::PermissionDenied);
}

// ---------------------------------------------------------------------------
// Criterion 5: How strongly should I trust this?
// ---------------------------------------------------------------------------

#[test]
fn trust_components_high_for_recent_success() {
    let (store, _dir) = temp_store();
    let goal_id = Uuid::new_v4();

    store.append(make_entry(goal_id, Uuid::new_v4(), "goal.completed", "", serde_json::json!({}))).unwrap();

    let mut svc = ExperienceService::new();
    svc.rebuild(&store.all_entries().unwrap(), &[], &[]);

    let tc = svc.trust_components(goal_id).expect("should have trust components");
    assert!(tc.aggregated() > 0.5, "recent success should have high trust, got {}", tc.aggregated());
    assert!(tc.outcome_quality > 0.8, "success outcome quality should be high");
    assert!(tc.recency > 0.8, "recency should be high for just-now event");
}

#[test]
fn trust_components_low_for_failure() {
    let (store, _dir) = temp_store();
    let goal_id = Uuid::new_v4();

    store.append(make_entry(goal_id, Uuid::new_v4(), "exec.result", "fs.write", serde_json::json!({"success": false, "error": "not found"}))).unwrap();

    let mut svc = ExperienceService::new();
    svc.rebuild(&store.all_entries().unwrap(), &[], &[]);

    let tc = svc.trust_components(goal_id).expect("should have trust components");
    assert!(tc.aggregated() < 0.35, "failure should have low trust, got {}", tc.aggregated());
    assert!(tc.outcome_quality < 0.4, "failure outcome quality should be low");
}

#[test]
fn trust_components_none_for_unknown_goal() {
    let svc = ExperienceService::new();
    assert!(svc.trust_components(Uuid::new_v4()).is_none());
}

// ---------------------------------------------------------------------------
// TrustComponents aggregation
// ---------------------------------------------------------------------------

#[test]
fn trust_components_weighted_average() {
    use friday_planner::experience::TrustComponents;

    let tc = TrustComponents {
        similarity: 0.8,
        recency: 0.9,
        outcome_quality: 0.7,
        repetition: 0.5,
    };
    // 0.8*0.3 + 0.7*0.4 + 0.9*0.2 + 0.5*0.1 = 0.24 + 0.28 + 0.18 + 0.05 = 0.75
    let score = tc.aggregated();
    assert!((score - 0.75).abs() < 0.01, "expected ~0.75, got {score}");
}

// ---------------------------------------------------------------------------
// Indexer rebuild
// ---------------------------------------------------------------------------

#[test]
fn indexer_rebuild_from_timeline() {
    let mut indexer = ExperienceIndexer::new();
    let goal_id = Uuid::new_v4();
    let plan_id = Uuid::new_v4();

    let entries = vec![
        make_entry(goal_id, plan_id, "exec.action", "filesystem.write", serde_json::json!({"title": "write readme"})),
        make_entry(goal_id, plan_id, "exec.result", "filesystem.write", serde_json::json!({"success": true})),
        make_entry(goal_id, plan_id, "goal.completed", "", serde_json::json!({})),
    ];

    indexer.build(&entries, &[], &[]);
    assert_eq!(indexer.goal_count(), 1);

    let record = indexer.get_record(goal_id).unwrap();
    assert_eq!(record.outcome, GoalOutcome::Success);
    assert_eq!(record.entries.len(), 3);
}

#[test]
fn indexer_semantic_similarity() {
    let mut indexer = ExperienceIndexer::new();

    let goal_a = Uuid::new_v4();
    let goal_b = Uuid::new_v4();

    let entries = vec![
        make_entry(goal_a, Uuid::new_v4(), "exec.action", "filesystem.write", serde_json::json!({"title": "write readme file"})),
        make_entry(goal_a, Uuid::new_v4(), "goal.completed", "", serde_json::json!({})),
        make_entry(goal_b, Uuid::new_v4(), "exec.action", "filesystem.read", serde_json::json!({"title": "read config file"})),
        make_entry(goal_b, Uuid::new_v4(), "goal.completed", "", serde_json::json!({})),
    ];

    indexer.build(&entries, &[], &[]);

    let results = indexer.similar("write readme", 10);
    assert!(!results.is_empty());
    assert_eq!(results[0].0 .0, goal_a);
}

// ---------------------------------------------------------------------------
// Artifact and commit_hash extraction in matches
// ---------------------------------------------------------------------------

#[test]
fn similar_match_extracts_artifacts() {
    let (store, _dir) = temp_store();
    let goal_id = Uuid::new_v4();
    let plan_id = Uuid::new_v4();

    store.append(make_entry(goal_id, plan_id, "exec.action", "filesystem.write", serde_json::json!({"title": "write docs"}))).unwrap();
    store.append(make_entry(goal_id, plan_id, "artifact.created", "filesystem.write", serde_json::json!({"path": "/tmp/docs.md"}))).unwrap();
    store.append(make_entry(goal_id, plan_id, "goal.completed", "", serde_json::json!({}))).unwrap();

    let mut svc = ExperienceService::new();
    svc.rebuild(&store.all_entries().unwrap(), &[], &[]);

    let matches = svc.similar("write docs", 10);
    assert!(!matches.is_empty());
    assert_eq!(matches[0].artifacts, vec!["/tmp/docs.md"]);
}

#[test]
fn similar_match_extracts_commit_hash() {
    let (store, _dir) = temp_store();
    let goal_id = Uuid::new_v4();
    let plan_id = Uuid::new_v4();

    store.append(make_entry(goal_id, plan_id, "exec.action", "git.commit", serde_json::json!({"title": "commit changes"}))).unwrap();
    store.append(make_entry(goal_id, plan_id, "git.commit.created", "git.commit", serde_json::json!({"commit_hash": "abc1234"}))).unwrap();
    store.append(make_entry(goal_id, plan_id, "goal.completed", "", serde_json::json!({}))).unwrap();

    let mut svc = ExperienceService::new();
    svc.rebuild(&store.all_entries().unwrap(), &[], &[]);

    let matches = svc.similar("commit changes", 10);
    assert!(!matches.is_empty());
    assert_eq!(matches[0].commit_hash.as_deref(), Some("abc1234"));
}
