use friday_cognitive::workspace::Topic;
use friday_knowledge::{KnowledgeEngine, PatternDetector};
use friday_memory::Episode;
use friday_reflection::{InsightKind, Outcome, OutcomeKind, ReflectionEngine};

/// Full pipeline: Episodes → Knowledge → Reflection → Insight.
#[test]
fn knowledge_to_reflection_to_insight() {
    // ── Phase 1: Observe 40 iterations of a dev workflow ───────────
    let mut episodes = Vec::new();
    for _ in 0..40 {
        let titles = vec![
            "main.rs — Visual Studio Code",
            "pull/123 — GitHub — Chrome",
            "main.rs — Visual Studio Code",
            "build — Cargo",
        ];
        for title in &titles {
            let event = friday_cognitive::WorkspaceEvent::new(
                Topic::WindowFocusChanged,
                serde_json::json!({"title": title}),
            );
            episodes.push(Episode::from_event(event));
        }
    }
    assert_eq!(episodes.len(), 160);

    // ── Phase 2: Derive knowledge from episodes ────────────────────
    let mut knowledge_engine = KnowledgeEngine::with_detector(PatternDetector::new(3, 5));
    let knowledge = knowledge_engine.analyze(&episodes);
    assert!(
        !knowledge.is_empty(),
        "Should derive knowledge from 40 workflow iterations"
    );

    let dev_workflow = &knowledge[0];
    assert!(dev_workflow.support >= 30);

    // ── Phase 3: Mission fails — reflection creates lesson ────────
    let failure = Outcome {
        id: "outcome_deploy_failed".into(),
        kind: OutcomeKind::MissionFailed,
        context: "deploy to production".into(),
        detail: serde_json::json!({"error": "cargo build failed"}),
        occurred_at_ms: 2000,
    };

    let mut reflection_engine = ReflectionEngine::new();
    let insights = reflection_engine.reflect_all(&knowledge, &failure);

    assert!(!insights.is_empty());
    let lesson = &insights[0];
    assert_eq!(lesson.kind, InsightKind::Lesson);
    assert!(lesson.statement.contains("deploy to production"));
    assert!(lesson.statement.contains("Visual Studio Code"));

    // ── Phase 4: Mission succeeds — reflection creates reinforcement ─
    let success = Outcome {
        id: "outcome_deploy_ok".into(),
        kind: OutcomeKind::MissionSucceeded,
        context: "deploy to production".into(),
        detail: serde_json::json!({}),
        occurred_at_ms: 3000,
    };

    let reinforcements = reflection_engine.reflect_all(&knowledge, &success);
    assert!(!reinforcements.is_empty());
    assert_eq!(reinforcements[0].kind, InsightKind::Reinforcement);
    assert!(reinforcements[0].statement.contains("confirmed"));

    // ── Phase 5: Verify provenance chain ───────────────────────────
    assert_eq!(lesson.knowledge_id, dev_workflow.id);
    assert_eq!(lesson.outcome_id, failure.id);
}

/// All outcome kinds produce insights.
#[test]
fn all_outcomes_produce_insights() {
    let mut knowledge_engine = KnowledgeEngine::new();
    let episodes = create_workflow_episodes(10);
    let knowledge = knowledge_engine.analyze(&episodes);

    let outcomes = vec![
        (OutcomeKind::MissionSucceeded, InsightKind::Reinforcement),
        (OutcomeKind::MissionFailed, InsightKind::Lesson),
        (OutcomeKind::TaskCompleted, InsightKind::Reinforcement),
        (OutcomeKind::TaskAbandoned, InsightKind::Lesson),
        (OutcomeKind::Error, InsightKind::Lesson),
        (OutcomeKind::UnexpectedSuccess, InsightKind::Optimization),
        (OutcomeKind::UnexpectedFailure, InsightKind::Warning),
    ];

    let mut reflection_engine = ReflectionEngine::new();
    for (outcome_kind, expected_insight_kind) in &outcomes {
        let outcome = Outcome {
            id: format!("o_{:?}", outcome_kind),
            kind: outcome_kind.clone(),
            context: "test".into(),
            detail: serde_json::json!({}),
            occurred_at_ms: 1000,
        };
        let insights = reflection_engine.reflect_all(&knowledge, &outcome);
        assert!(
            !insights.is_empty(),
            "Outcome {:?} should produce an insight",
            outcome_kind
        );
        assert_eq!(
            insights[0].kind, *expected_insight_kind,
            "Outcome {:?} should produce {:?}",
            outcome_kind, expected_insight_kind
        );
    }
}

fn create_workflow_episodes(iterations: usize) -> Vec<Episode> {
    let mut episodes = Vec::new();
    for _ in 0..iterations {
        let titles = vec![
            "main.rs — Visual Studio Code",
            "pull/123 — GitHub — Chrome",
            "main.rs — Visual Studio Code",
            "build — Cargo",
        ];
        for title in &titles {
            let event = friday_cognitive::WorkspaceEvent::new(
                Topic::WindowFocusChanged,
                serde_json::json!({"title": title}),
            );
            episodes.push(Episode::from_event(event));
        }
    }
    episodes
}
