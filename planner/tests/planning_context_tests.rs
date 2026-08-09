//! Planning Context tests (M13).
//!
//! Tests the ExperienceContextBuilder with a mock ExperienceProvider.

use chrono::Utc;
use uuid::Uuid;

use friday_planner::experience::provider::ExperienceProvider;
use friday_planner::experience::{
    EvidenceSource, ExperienceContextBuilder, ExperienceMatch, GoalOutcome, MatchReason,
    PlanningGoal, ReflectionRecord, TrustComponents, WorkflowSuggestion,
};

// ---------------------------------------------------------------------------
// Mock provider
// ---------------------------------------------------------------------------

struct MockProvider {
    similar_results: Vec<ExperienceMatch>,
    last_attempt_result: Option<ExperienceMatch>,
    successful_workflows_result: Vec<friday_planner::experience::indexer::GoalRecord>,
    suggestions_result: Vec<WorkflowSuggestion>,
    reflections_result: Vec<ReflectionRecord>,
}

impl MockProvider {
    fn empty() -> Self {
        Self {
            similar_results: vec![],
            last_attempt_result: None,
            successful_workflows_result: vec![],
            suggestions_result: vec![],
            reflections_result: vec![],
        }
    }
}

impl ExperienceProvider for MockProvider {
    fn similar(&self, _goal_title: &str, _max_results: usize) -> Vec<ExperienceMatch> {
        self.similar_results.clone()
    }

    fn last_attempt(&self, _goal_id: Uuid) -> Option<ExperienceMatch> {
        self.last_attempt_result.clone()
    }

    fn successful_workflows(
        &self,
        _goal_title: &str,
    ) -> Vec<friday_planner::experience::indexer::GoalRecord> {
        self.successful_workflows_result.clone()
    }

    fn suggestions(&self) -> Vec<WorkflowSuggestion> {
        self.suggestions_result.clone()
    }

    fn load_reflections(&self, _goal_ids: &[Uuid]) -> Vec<ReflectionRecord> {
        self.reflections_result.clone()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_match(title: &str, outcome: GoalOutcome) -> ExperienceMatch {
    ExperienceMatch {
        similarity_score: 0.8,
        trust_score: 0.7,
        trust_components: TrustComponents {
            similarity: 0.8,
            recency: 0.9,
            outcome_quality: 0.9,
            repetition: 0.3,
        },
        match_reason: MatchReason::SimilarGoal,
        goal_id: Uuid::new_v4(),
        workflow_id: None,
        outcome,
        reflection: vec![],
        artifacts: vec![],
        commit_hash: None,
        goal_title: title.to_string(),
    }
}

fn make_reflection(insight: &str) -> ReflectionRecord {
    ReflectionRecord {
        id: Uuid::new_v4(),
        timeline_entry_id: Uuid::new_v4(),
        insight: insight.to_string(),
        category: friday_planner::experience::ReflectionCategory::Observation,
        timestamp: Utc::now(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn builder_with_empty_provider() {
    let provider = MockProvider::empty();
    let goal = PlanningGoal {
        id: Uuid::new_v4(),
        title: "Test goal".to_string(),
        description: "Test description".to_string(),
    };

    let ctx = ExperienceContextBuilder::build(
        &provider,
        goal,
        vec!["mcp:filesystem.read".to_string()],
        vec!["no-risk".to_string()],
        &[],
    );

    assert!(ctx.evidence.similar_experiences.is_empty());
    assert!(ctx.evidence.previous_outcome.is_none());
    assert!(ctx.evidence.successful_workflows.is_empty());
    assert!(ctx.evidence.workflow_suggestions.is_empty());
    assert!(ctx.evidence.replay_candidates.is_empty());
    assert!(ctx.evidence.reflections.is_empty());
    assert!(ctx.evidence.provenance.is_empty());
}

#[test]
fn builder_with_similar_experiences() {
    let provider = MockProvider {
        similar_results: vec![
            make_match("Add MCP support", GoalOutcome::Success),
            make_match("Add Git support", GoalOutcome::Success),
        ],
        ..MockProvider::empty()
    };

    let goal = PlanningGoal {
        id: Uuid::new_v4(),
        title: "Add Filesystem MCP".to_string(),
        description: "Support filesystem operations".to_string(),
    };

    let ctx = ExperienceContextBuilder::build(&provider, goal, vec![], vec![], &[]);

    assert_eq!(ctx.evidence.similar_experiences.len(), 2);
    assert_eq!(
        ctx.evidence.similar_experiences[0].goal_title,
        "Add MCP support"
    );
    assert_eq!(
        ctx.evidence.similar_experiences[1].goal_title,
        "Add Git support"
    );
}

#[test]
fn builder_with_previous_outcome() {
    let provider = MockProvider {
        last_attempt_result: Some(make_match("Add Filesystem MCP", GoalOutcome::Failure)),
        ..MockProvider::empty()
    };

    let goal = PlanningGoal {
        id: Uuid::new_v4(),
        title: "Add Filesystem MCP".to_string(),
        description: "Support filesystem operations".to_string(),
    };

    let ctx = ExperienceContextBuilder::build(&provider, goal, vec![], vec![], &[]);

    assert!(ctx.evidence.previous_outcome.is_some());
    let prev = ctx.evidence.previous_outcome.unwrap();
    assert_eq!(prev.outcome, GoalOutcome::Failure);
}

#[test]
fn builder_collects_reflections_from_similar() {
    let reflection = make_reflection("File operations need error handling");
    let mut m = make_match("Add MCP support", GoalOutcome::Success);
    m.reflection = vec![reflection];

    let provider = MockProvider {
        similar_results: vec![m],
        ..MockProvider::empty()
    };

    let goal = PlanningGoal {
        id: Uuid::new_v4(),
        title: "Add Filesystem MCP".to_string(),
        description: "Support filesystem operations".to_string(),
    };

    let ctx = ExperienceContextBuilder::build(&provider, goal, vec![], vec![], &[]);

    assert_eq!(ctx.evidence.reflections.len(), 1);
    assert_eq!(
        ctx.evidence.reflections[0].insight,
        "File operations need error handling"
    );
}

#[test]
fn builder_builds_replay_candidates() {
    let suggestion = WorkflowSuggestion {
        pattern_id: Uuid::new_v4(),
        capabilities: vec![
            "mcp:filesystem.write_file".to_string(),
            "mcp:git.git_commit".to_string(),
        ],
        reasons: vec!["Frequent pattern".to_string()],
        confidence: 0.85,
        generated_at: Utc::now(),
        frequency: 10,
        success_rate: 0.95,
        last_seen: Utc::now(),
    };

    let provider = MockProvider {
        suggestions_result: vec![suggestion],
        ..MockProvider::empty()
    };

    let goal = PlanningGoal {
        id: Uuid::new_v4(),
        title: "Add feature".to_string(),
        description: "Do something".to_string(),
    };

    let registered = vec![
        "mcp:filesystem.write_file".to_string(),
        "mcp:git.git_commit".to_string(),
    ];

    let ctx = ExperienceContextBuilder::build(&provider, goal, vec![], vec![], &registered);

    assert_eq!(ctx.evidence.replay_candidates.len(), 1);
    let candidate = &ctx.evidence.replay_candidates[0];
    assert!(candidate.applicability.applicable);
    assert_eq!(candidate.reuse_ratio, 1.0);
    assert_eq!(candidate.pattern.capabilities.len(), 2);
}

#[test]
fn builder_replay_candidate_with_missing_capabilities() {
    let suggestion = WorkflowSuggestion {
        pattern_id: Uuid::new_v4(),
        capabilities: vec![
            "mcp:filesystem.write_file".to_string(),
            "mcp:git.git_commit".to_string(),
            "mcp:github.create_pr".to_string(), // not registered
        ],
        reasons: vec!["Complex pattern".to_string()],
        confidence: 0.7,
        generated_at: Utc::now(),
        frequency: 8,
        success_rate: 0.92,
        last_seen: Utc::now(),
    };

    let provider = MockProvider {
        suggestions_result: vec![suggestion],
        ..MockProvider::empty()
    };

    let goal = PlanningGoal {
        id: Uuid::new_v4(),
        title: "Add feature".to_string(),
        description: "Do something".to_string(),
    };

    // Only register 2 of 3 capabilities
    let registered = vec![
        "mcp:filesystem.write_file".to_string(),
        "mcp:git.git_commit".to_string(),
    ];

    let ctx = ExperienceContextBuilder::build(&provider, goal, vec![], vec![], &registered);

    assert_eq!(ctx.evidence.replay_candidates.len(), 1);
    let candidate = &ctx.evidence.replay_candidates[0];
    assert!(!candidate.applicability.applicable);
    assert!(candidate.reuse_ratio < 1.0);
    // 2 of 3 steps reusable
    assert!((candidate.reuse_ratio - 0.666).abs() < 0.01);
}

#[test]
fn builder_context_is_immutable_snapshot() {
    let provider = MockProvider {
        similar_results: vec![make_match("Test", GoalOutcome::Success)],
        ..MockProvider::empty()
    };

    let goal = PlanningGoal {
        id: Uuid::new_v4(),
        title: "Test".to_string(),
        description: "Test".to_string(),
    };

    let ctx = ExperienceContextBuilder::build(
        &provider,
        goal,
        vec!["cap1".to_string()],
        vec!["constraint1".to_string()],
        &[],
    );

    // Verify immutable: can clone but original is unchanged
    let ctx2 = ctx.clone();
    assert_eq!(
        ctx.evidence.similar_experiences.len(),
        ctx2.evidence.similar_experiences.len()
    );
    assert_eq!(ctx.capabilities, ctx2.capabilities);
    assert_eq!(ctx.constraints, ctx2.constraints);
}

#[test]
fn builder_preserves_goal_and_constraints() {
    let provider = MockProvider::empty();
    let goal = PlanningGoal {
        id: Uuid::new_v4(),
        title: "Important task".to_string(),
        description: "Must be done carefully".to_string(),
    };

    let ctx = ExperienceContextBuilder::build(
        &provider,
        goal.clone(),
        vec![
            "mcp:filesystem.read".to_string(),
            "mcp:git.git_status".to_string(),
        ],
        vec!["no-risk".to_string(), "budget:100".to_string()],
        &[],
    );

    assert_eq!(ctx.goal.title, "Important task");
    assert_eq!(ctx.goal.description, "Must be done carefully");
    assert_eq!(ctx.capabilities.len(), 2);
    assert_eq!(ctx.constraints.len(), 2);
}

#[test]
fn builder_serializes_and_deserializes() {
    let provider = MockProvider {
        similar_results: vec![make_match("Test", GoalOutcome::Success)],
        ..MockProvider::empty()
    };

    let goal = PlanningGoal {
        id: Uuid::new_v4(),
        title: "Test".to_string(),
        description: "Test".to_string(),
    };

    let ctx = ExperienceContextBuilder::build(&provider, goal, vec![], vec![], &[]);

    let json = serde_json::to_string(&ctx).unwrap();
    let deserialized: friday_planner::experience::ExperienceContext =
        serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.goal.title, "Test");
    assert_eq!(deserialized.evidence.similar_experiences.len(), 1);
}

#[test]
fn evidence_source_enum_values() {
    let sources = vec![
        EvidenceSource::Retrieval,
        EvidenceSource::Replay,
        EvidenceSource::PatternMining,
        EvidenceSource::Reflection,
    ];
    assert_eq!(sources.len(), 4);
}
