//! Dogfooding Scenarios — FRIDAY on FRIDAY.
//!
//! Simulates realistic development tasks using the experience layer,
//! exercising the full pipeline: record → retrieve → mine → suggest → replay.
//!
//! These scenarios are based on actual FRIDAY development patterns
//! (129 commits: features, tests, docs, refactors).

use uuid::Uuid;

use friday_planner::experience::recorder::record_workflow;
use friday_planner::experience::reflection::reflect_on_goal;
use friday_planner::experience::replay::{ApplicabilityChecker, ReplayEngine};
use friday_planner::experience::service::ExperienceService;
use friday_planner::experience::TimelineStore;
use friday_planner::experience::{ExperienceMetrics, GoalOutcome};

fn temp_store() -> (TimelineStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timeline-dogfood.redb");
    let store = TimelineStore::open(&path).unwrap();
    (store, dir)
}

/// Simulate a "feature: add MCP integration" task.
/// Pattern: transport.setup → protocol.handshake → adapter.register → test.write → commit
fn record_mcp_integration(store: &TimelineStore, name: &str) -> Uuid {
    let goal_id = Uuid::new_v4();
    let plan_id = Uuid::new_v4();
    let exec_id = Uuid::new_v4();

    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.action",
            "mcp:transport.setup",
            serde_json::json!({"title": name}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.result",
            "mcp:transport.setup",
            serde_json::json!({"success": true}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.action",
            "mcp:protocol.handshake",
            serde_json::json!({}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.result",
            "mcp:protocol.handshake",
            serde_json::json!({"success": true}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.action",
            "mcp:adapter.register",
            serde_json::json!({}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.result",
            "mcp:adapter.register",
            serde_json::json!({"success": true}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.action",
            "filesystem.write",
            serde_json::json!({"path": "tests/integration.rs"}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.result",
            "filesystem.write",
            serde_json::json!({"success": true}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "artifact.created",
            "filesystem.write",
            serde_json::json!({"path": "tests/integration.rs"}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.action",
            "mcp:git.git_add",
            serde_json::json!({}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.result",
            "mcp:git.git_add",
            serde_json::json!({"success": true}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.action",
            "mcp:git.git_commit",
            serde_json::json!({}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.result",
            "mcp:git.git_commit",
            serde_json::json!({"success": true, "commit_hash": format!("abc{i:04x}", i = rand())}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "goal.completed",
            "",
            serde_json::json!({"outcome": "success"}),
            None,
        )
        .unwrap();
    goal_id
}

/// Simulate a "fix: clippy warning" task.
/// Pattern: filesystem.read → filesystem.write → git.add → git.commit
fn record_clippy_fix(store: &TimelineStore, name: &str) -> Uuid {
    let goal_id = Uuid::new_v4();
    let plan_id = Uuid::new_v4();
    let exec_id = Uuid::new_v4();

    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.action",
            "mcp:filesystem.read_file",
            serde_json::json!({"title": name, "path": "src/lib.rs"}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.result",
            "mcp:filesystem.read_file",
            serde_json::json!({"success": true}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.action",
            "mcp:filesystem.write_file",
            serde_json::json!({}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.result",
            "mcp:filesystem.write_file",
            serde_json::json!({"success": true}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.action",
            "mcp:git.git_add",
            serde_json::json!({}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.result",
            "mcp:git.git_add",
            serde_json::json!({"success": true}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.action",
            "mcp:git.git_commit",
            serde_json::json!({}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.result",
            "mcp:git.git_commit",
            serde_json::json!({"success": true}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "goal.completed",
            "",
            serde_json::json!({"outcome": "success"}),
            None,
        )
        .unwrap();
    goal_id
}

/// Simulate a "docs: update ADR" task.
/// Pattern: filesystem.read → filesystem.write → git.add → git.commit
fn record_adr_update(store: &TimelineStore, name: &str) -> Uuid {
    let goal_id = Uuid::new_v4();
    let plan_id = Uuid::new_v4();
    let exec_id = Uuid::new_v4();

    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.action",
            "mcp:filesystem.read_file",
            serde_json::json!({"title": name, "path": "specs/adr/ADR-0010.md"}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.result",
            "mcp:filesystem.read_file",
            serde_json::json!({"success": true}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.action",
            "mcp:filesystem.write_file",
            serde_json::json!({}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.result",
            "mcp:filesystem.write_file",
            serde_json::json!({"success": true}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.action",
            "mcp:git.git_add",
            serde_json::json!({}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.result",
            "mcp:git.git_add",
            serde_json::json!({"success": true}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.action",
            "mcp:git.git_commit",
            serde_json::json!({}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "exec.result",
            "mcp:git.git_commit",
            serde_json::json!({"success": true}),
            None,
        )
        .unwrap();
    store
        .record(
            goal_id,
            plan_id,
            exec_id,
            "goal.completed",
            "",
            serde_json::json!({"outcome": "success"}),
            None,
        )
        .unwrap();
    goal_id
}

/// Simple pseudo-random for commit hashes (no external dependency).
fn rand() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    nanos ^ (nanos >> 16)
}

// ===========================================================================
// Dogfood 1: Simulate 10 realistic FRIDAY development tasks
// ===========================================================================

#[test]
fn dogfood_1_simulate_realistic_development_history() {
    let (store, _dir) = temp_store();

    // Simulate a realistic development session:
    // - 3 MCP integrations (M9, M10, future M13)
    // - 5 clippy fixes
    // - 2 ADR updates
    let mut all_goal_ids = Vec::new();

    // MCP integrations (repeated workflow).
    for i in 0..3 {
        let gid = record_mcp_integration(&store, &format!("Add MCP server #{i}"));
        all_goal_ids.push(gid);
    }

    // Clippy fixes (repeated workflow).
    for i in 0..5 {
        let gid = record_clippy_fix(&store, &format!("Fix clippy warning #{i}"));
        all_goal_ids.push(gid);
    }

    // ADR updates (repeated workflow).
    for i in 0..2 {
        let gid = record_adr_update(&store, &format!("Write ADR #{i}"));
        all_goal_ids.push(gid);
    }

    // Record workflows for each goal.
    for gid in &all_goal_ids {
        record_workflow(&store, *gid, "dev-task").unwrap();
    }

    // Build experience service.
    let reflections = reflect_on_goal(&store, all_goal_ids[0]).unwrap();
    let mut svc = ExperienceService::new();
    svc.rebuild(
        &store.all_entries().unwrap(),
        &store.load_workflows().unwrap(),
        &reflections,
    );

    // Verify indexing.
    assert_eq!(svc.goal_count(), 10, "should index all 10 goals");

    // Mine patterns.
    let patterns = svc.patterns();
    assert!(
        !patterns.is_empty(),
        "should find patterns from repeated workflows"
    );

    // The read→write→add→commit pattern should appear (clippy fixes + ADR updates).
    let rwac_patterns: Vec<_> = patterns
        .iter()
        .filter(|p| {
            p.capabilities
                .contains(&"mcp:filesystem.read_file".to_string())
                && p.capabilities.contains(&"mcp:git.git_commit".to_string())
        })
        .collect();
    assert!(
        !rwac_patterns.is_empty(),
        "should find read-write-add-commit pattern"
    );

    // The transport→handshake→adapter→write→add→commit pattern should appear (MCP integrations).
    let mcp_patterns: Vec<_> = patterns
        .iter()
        .filter(|p| p.capabilities.contains(&"mcp:transport.setup".to_string()))
        .collect();
    assert!(
        !mcp_patterns.is_empty(),
        "should find MCP integration pattern"
    );

    // Suggestions should be generated.
    let suggestions = svc.suggestions();
    assert!(
        !suggestions.is_empty(),
        "should generate suggestions from patterns"
    );

    // Metrics.
    let mut metrics = ExperienceMetrics::new();
    metrics.record_patterns(patterns);
    metrics.record_suggestions(&suggestions);
    let summary = metrics.summary();
    assert!(summary.patterns_mined > 0);
    assert!(summary.suggestions_generated > 0);

    println!("=== Dogfood 1: Realistic Development History ===");
    println!("Goals indexed: {}", svc.goal_count());
    println!("Patterns mined: {}", summary.patterns_mined);
    println!("Suggestions generated: {}", summary.suggestions_generated);
    for s in &suggestions {
        println!(
            "  Suggestion: {:?} (confidence: {:.2})",
            s.capabilities, s.confidence
        );
        println!("    Reasons: {:?}", s.reasons);
    }
}

// ===========================================================================
// Dogfood 2: Query experience for a new task
// ===========================================================================

#[test]
fn dogfood_2_query_for_new_task() {
    let (store, _dir) = temp_store();

    // Build history from past development.
    for i in 0..5 {
        record_clippy_fix(&store, &format!("Fix clippy #{i}"));
    }
    for i in 0..3 {
        record_mcp_integration(&store, &format!("Add MCP #{i}"));
    }

    let mut svc = ExperienceService::new();
    svc.rebuild(&store.all_entries().unwrap(), &[], &[]);

    // Query 1: "Add a new feature" — should find MCP integration patterns.
    let matches = svc.similar("Add a new feature", 5);
    println!("=== Dogfood 2: Query for New Task ===");
    println!("Query: 'Add a new feature'");
    println!("Matches: {}", matches.len());
    for m in &matches {
        println!(
            "  Title: '{}' (similarity: {:.3}, trust: {:.3})",
            m.goal_title, m.similarity_score, m.trust_score
        );
    }

    // Query 2: "Fix a warning" — should find clippy fix patterns.
    let matches = svc.similar("Fix a warning", 5);
    println!("\nQuery: 'Fix a warning'");
    println!("Matches: {}", matches.len());
    for m in &matches {
        println!(
            "  Title: '{}' (similarity: {:.3}, trust: {:.3})",
            m.goal_title, m.similarity_score, m.trust_score
        );
    }

    // Query 3: "Write documentation" — should find ADR patterns if they exist.
    // (We didn't record ADRs in this test, so matches may be low.)
    let matches = svc.similar("Write documentation", 5);
    println!("\nQuery: 'Write documentation'");
    println!("Matches: {}", matches.len());
    for m in &matches {
        println!(
            "  Title: '{}' (similarity: {:.3}, trust: {:.3})",
            m.goal_title, m.similarity_score, m.trust_score
        );
    }
}

// ===========================================================================
// Dogfood 3: Replay value for repeated workflows
// ===========================================================================

#[test]
fn dogfood_3_replay_value_for_repeated_workflows() {
    // Simulate: original workflow was transport→handshake→adapter→write→add→commit.
    // New task: transport→handshake→adapter→write→add→commit (exact reuse).
    let original = vec![
        "mcp:transport.setup".into(),
        "mcp:protocol.handshake".into(),
        "mcp:adapter.register".into(),
        "filesystem.write".into(),
        "mcp:git.git_add".into(),
        "mcp:git.git_commit".into(),
    ];

    // Exact reuse scenario.
    let report = ReplayEngine::report(
        Some(Uuid::new_v4()),
        None,
        &original,
        &original,
        GoalOutcome::Success,
    );

    println!("=== Dogfood 3: Replay Value ===");
    println!("Scenario: Exact reuse of MCP integration workflow");
    println!("  Reused: {}", report.reused_steps.len());
    println!("  Adapted: {}", report.adapted_steps.len());
    println!("  Skipped: {}", report.skipped_steps.len());
    println!("  New: {}", report.new_steps.len());
    println!("  Reuse ratio: {:.2}", report.reuse_ratio());

    assert_eq!(
        report.reuse_ratio(),
        1.0,
        "exact reuse should have ratio 1.0"
    );
    assert!(report.adapted_steps.is_empty());
    assert!(report.skipped_steps.is_empty());
    assert!(report.new_steps.is_empty());

    // Adaptation scenario: skip handshake (already connected), add push.
    let adapted = vec![
        "mcp:transport.setup".into(),
        "mcp:adapter.register".into(),
        "filesystem.write".into(),
        "mcp:git.git_add".into(),
        "mcp:git.git_commit".into(),
        "mcp:git.git_push".into(),
    ];

    let report = ReplayEngine::report(
        Some(Uuid::new_v4()),
        None,
        &original,
        &adapted,
        GoalOutcome::Success,
    );

    println!("\nScenario: Adapted (skip handshake, add push)");
    println!("  Reused: {}", report.reused_steps.len());
    println!("  Adapted: {}", report.adapted_steps.len());
    println!("  Skipped: {}", report.skipped_steps.len());
    println!("  New: {}", report.new_steps.len());
    println!("  Reuse ratio: {:.2}", report.reuse_ratio());

    assert!(
        report.reuse_ratio() > 0.5,
        "adapted workflow should still have high reuse"
    );
    assert!(report
        .skipped_steps
        .contains(&"mcp:protocol.handshake".to_string()));
    assert!(report.new_steps.contains(&"mcp:git.git_push".to_string()));
}

// ===========================================================================
// Dogfood 4: Applicability check for MCP integration
// ===========================================================================

#[test]
fn dogfood_4_applicability_for_mcp_integration() {
    use friday_planner::experience::Workflow;

    // Workflow requires full MCP stack.
    let workflow = Workflow {
        id: Uuid::new_v4(),
        name: "mcp:integration".into(),
        actions: vec![
            "transport".into(),
            "handshake".into(),
            "adapter".into(),
            "write".into(),
            "add".into(),
            "commit".into(),
        ],
        capabilities: vec![
            "mcp:transport.setup".into(),
            "mcp:protocol.handshake".into(),
            "mcp:adapter.register".into(),
            "filesystem.write".into(),
            "mcp:git.git_add".into(),
            "mcp:git.git_commit".into(),
        ],
        created_at: chrono::Utc::now(),
        source_goal_id: Uuid::new_v4(),
    };

    // All capabilities registered.
    let checker = ApplicabilityChecker::new(
        vec![
            "mcp:transport.setup".into(),
            "mcp:protocol.handshake".into(),
            "mcp:adapter.register".into(),
            "filesystem.write".into(),
            "mcp:git.git_add".into(),
            "mcp:git.git_commit".into(),
        ],
        vec![],
    );
    let result = checker.check_workflow(&workflow);
    assert!(result.applicable, "all capabilities registered");

    println!("=== Dogfood 4: Applicability Check ===");
    println!(
        "All capabilities registered: applicable={}",
        result.applicable
    );

    // Only filesystem and git registered (no MCP transport).
    let checker = ApplicabilityChecker::new(
        vec![
            "filesystem.write".into(),
            "mcp:git.git_add".into(),
            "mcp:git.git_commit".into(),
        ],
        vec![],
    );
    let result = checker.check_workflow(&workflow);
    assert!(!result.applicable, "missing MCP transport capabilities");
    assert_eq!(result.missing_capabilities.len(), 3);

    println!("Missing MCP capabilities: applicable={}", result.applicable);
    println!("  Missing: {:?}", result.missing_capabilities);
}

// ===========================================================================
// Dogfood 5: Trust decomposition for repeated success
// ===========================================================================

#[test]
fn dogfood_5_trust_for_repeated_success() {
    let (store, _dir) = temp_store();

    // Record 10 successful clippy fixes (same workflow, same outcome).
    let mut goal_ids = Vec::new();
    for i in 0..10 {
        let gid = record_clippy_fix(&store, &format!("Fix clippy #{i}"));
        goal_ids.push(gid);
    }

    let mut svc = ExperienceService::new();
    svc.rebuild(&store.all_entries().unwrap(), &[], &[]);

    // Query for a clippy fix.
    let matches = svc.similar("Fix clippy warning", 5);
    assert!(!matches.is_empty(), "should find clippy fix patterns");

    println!("=== Dogfood 5: Trust for Repeated Success ===");
    println!("Query: 'Fix clippy warning'");
    for m in &matches {
        println!("  Title: '{}'", m.goal_title);
        println!("    Similarity: {:.3}", m.similarity_score);
        println!("    Trust: {:.3}", m.trust_score);
        println!("    Trust components:");
        println!("      similarity: {:.3}", m.trust_components.similarity);
        println!("      recency: {:.3}", m.trust_components.recency);
        println!(
            "      outcome_quality: {:.3}",
            m.trust_components.outcome_quality
        );
        println!("      repetition: {:.3}", m.trust_components.repetition);
    }

    // Trust should be high for repeated successful tasks.
    let top_match = &matches[0];
    assert!(
        top_match.trust_score > 0.5,
        "repeated success should have high trust"
    );
    assert!(
        top_match.trust_components.repetition > 0.3,
        "repetition should be significant"
    );
    assert!(
        top_match.trust_components.outcome_quality > 0.8,
        "success should have high outcome quality"
    );
}
