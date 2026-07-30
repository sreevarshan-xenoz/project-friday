use crate::goal::Goal;
use crate::common::{Action, Capability, Expectation, ExecutionContext, Trace};
use crate::verification::{Verification, Tool, ToolResult, ToolRegistry, Verifier, VerifiedResult, Check};
use crate::trace::{TraceNode, TraceNodeKind, CognitiveTrace};
use async_trait::async_trait;
use friday_knowledge::KnowledgeEngine;
use friday_reflection::{Outcome, OutcomeKind, Insight, ReflectionEngine};
use friday_memory::Episode;
use friday_knowledge::KnowledgeRecord;

// ── Goal Engine ─────────────────────────────────────────────────────

/// The Goal Engine — owns how an Action becomes an Outcome.
///
/// **Invariant**: Every execution produces exactly one Outcome.
pub struct GoalEngine {
    pub(crate) registry: ToolRegistry,
    verification: Verification,
    knowledge_engine: KnowledgeEngine,
    reflection_engine: ReflectionEngine,
}

impl GoalEngine {
    pub fn new() -> Self {
        Self {
            registry: ToolRegistry::new(),
            verification: Verification::new(),
            knowledge_engine: KnowledgeEngine::new(),
            reflection_engine: ReflectionEngine::new(),
        }
    }

    pub fn with_registry(mut self, registry: ToolRegistry) -> Self {
        self.registry = registry;
        self
    }

    pub fn with_verification(mut self, verification: Verification) -> Self {
        self.verification = verification;
        self
    }

    pub async fn execute(
        &mut self,
        goal: &Goal,
        action: &Action,
        expectation: &Expectation,
        context: &ExecutionContext,
    ) -> GoalReport {
        let started_at = chrono::Utc::now().timestamp_millis() as u64;
        let mut cognitive_trace = CognitiveTrace::new(&goal.id);

        // Trace: Execution start
        let exec_node = TraceNode::new(
            TraceNodeKind::Execution,
            serde_json::json!({
                "action_id": action.id,
                "instruction": action.instruction,
                "capability": action.capability.to_string(),
            }),
            serde_json::json!({}),
        ).with_timing(started_at, started_at);
        cognitive_trace.push(exec_node);

        let tool = self.registry.resolve(&action.capability);
        let (tool_result, tool_name) = match tool {
            Some(t) => {
                let result = t.execute(action, context).await;
                (result, t.name().to_string())
            }
            None => (
                ToolResult::error(format!("no tool for capability: {}", action.capability)),
                String::new(),
            ),
        };

        let tool_finished = chrono::Utc::now().timestamp_millis() as u64;

        // Trace: Verification
        let verified = self.verification.verify(action, &tool_result);
        let verify_node = TraceNode::new(
            TraceNodeKind::Verification,
            serde_json::json!({
                "tool_success": verified.tool_success,
                "checks": verified.checks.iter().map(|c| serde_json::json!({
                    "name": c.name,
                    "passed": c.passed,
                })).collect::<Vec<_>>(),
            }),
            serde_json::json!({
                "verified": verified.verified,
            }),
        ).with_timing(tool_finished, chrono::Utc::now().timestamp_millis() as u64);
        cognitive_trace.push(verify_node);

        // Trace: Reflection
        let outcome = self.produce_outcome(goal, expectation, &verified, started_at);
        let episodes = self.create_episodes(goal, action, &tool_name, expectation, &verified);
        let insights = self.reflect(&episodes, &outcome);

        let reflect_node = TraceNode::new(
            TraceNodeKind::Reflection,
            serde_json::json!({
                "outcome_kind": format!("{:?}", outcome.kind),
                "episodes": episodes.len(),
            }),
            serde_json::json!({
                "insights": insights.len(),
                "insight_kinds": insights.iter().map(|i| format!("{:?}", i.kind)).collect::<Vec<_>>(),
            }),
        ).with_timing(
            chrono::Utc::now().timestamp_millis() as u64,
            chrono::Utc::now().timestamp_millis() as u64,
        );
        cognitive_trace.push(reflect_node);

        GoalReport {
            goal: goal.clone(),
            action: action.clone(),
            expectation: expectation.clone(),
            tool: tool_name,
            verification: verified,
            outcome,
            insights,
            episodes,
            trace: Trace {
                started_at_ms: started_at,
                finished_at_ms: chrono::Utc::now().timestamp_millis() as u64,
            },
            cognitive_trace,
        }
    }

    fn produce_outcome(
        &self,
        goal: &Goal,
        expectation: &Expectation,
        verified: &VerifiedResult,
        started_at: u64,
    ) -> Outcome {
        let kind = if (verified.verified && expectation.expect_success)
            || (!verified.verified && !expectation.expect_success)
        {
            OutcomeKind::TaskCompleted
        } else if verified.verified && !expectation.expect_success {
            OutcomeKind::UnexpectedSuccess
        } else {
            OutcomeKind::Error
        };

        let duration_ms = chrono::Utc::now().timestamp_millis() as u64 - started_at;

        Outcome {
            id: format!("outcome_{}", uuid::Uuid::new_v4()),
            kind,
            context: goal.description.clone(),
            detail: serde_json::json!({
                "expectation": expectation.description,
                "expected_success": expectation.expect_success,
                "tool_success": verified.tool_success,
                "verified": verified.verified,
                "checks": verified.checks,
                "duration_ms": duration_ms,
            }),
            occurred_at_ms: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    fn create_episodes(
        &self,
        goal: &Goal,
        action: &Action,
        tool_name: &str,
        expectation: &Expectation,
        verified: &VerifiedResult,
    ) -> Vec<Episode> {
        let mut episodes = Vec::new();

        episodes.push(Episode::from_event(friday_cognitive::WorkspaceEvent::new(
            friday_cognitive::workspace::Topic::GoalCreated,
            serde_json::json!({
                "goal_id": goal.id,
                "description": goal.description,
                "action_id": action.id,
                "instruction": action.instruction,
                "capability": action.capability.to_string(),
            }),
        )));

        episodes.push(Episode::from_event(friday_cognitive::WorkspaceEvent::new(
            friday_cognitive::workspace::Topic::ToolExecuted,
            serde_json::json!({
                "goal_id": goal.id,
                "action_id": action.id,
                "tool": tool_name,
                "tool_success": verified.tool_success,
                "verified": verified.verified,
            }),
        )));

        episodes.push(Episode::from_event(friday_cognitive::WorkspaceEvent::new(
            friday_cognitive::workspace::Topic::OutcomeObserved,
            serde_json::json!({
                "goal_id": goal.id,
                "action_id": action.id,
                "verified": verified.verified,
                "expected_success": expectation.expect_success,
            }),
        )));

        episodes
    }

    fn reflect(&mut self, episodes: &[Episode], outcome: &Outcome) -> Vec<Insight> {
        let knowledge = self.knowledge_engine.analyze(episodes);

        if knowledge.is_empty() {
            let synthetic = KnowledgeRecord {
                id: format!("k_goal_{}", outcome.id),
                kind: friday_knowledge::KnowledgeKind::Workflow,
                label: outcome.context.clone(),
                description: format!("Goal execution: {}", outcome.context),
                pattern: vec![outcome.context.clone()],
                support: 1,
                confidence: 1.0,
                derived_at_ms: chrono::Utc::now().timestamp_millis() as u64,
            };
            return self.reflection_engine.reflect_all(&[synthetic], outcome);
        }

        self.reflection_engine.reflect_all(&knowledge, outcome)
    }

    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    pub fn reflection_engine(&self) -> &ReflectionEngine {
        &self.reflection_engine
    }
}

impl Default for GoalEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── Goal Report ─────────────────────────────────────────────────────

/// A complete audit of one goal execution.
#[derive(Debug, Clone)]
pub struct GoalReport {
    pub goal: Goal,
    pub action: Action,
    pub expectation: Expectation,
    pub tool: String,
    pub verification: VerifiedResult,
    pub outcome: Outcome,
    pub insights: Vec<Insight>,
    pub episodes: Vec<Episode>,
    pub trace: Trace,
    pub cognitive_trace: CognitiveTrace,
}

impl GoalReport {
    pub fn success(&self) -> bool {
        self.outcome.kind == OutcomeKind::TaskCompleted
            || self.outcome.kind == OutcomeKind::MissionSucceeded
    }

    pub fn unexpected(&self) -> bool {
        self.outcome.kind == OutcomeKind::UnexpectedSuccess
            || self.outcome.kind == OutcomeKind::UnexpectedFailure
    }
}

// ── Mock Tool ───────────────────────────────────────────────────────

pub struct MockTool {
    name: String,
    capabilities: Vec<Capability>,
    should_succeed: bool,
    output: String,
}

impl MockTool {
    pub fn success(name: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            capabilities: vec![Capability::ExecuteCommand],
            should_succeed: true,
            output: output.into(),
        }
    }

    pub fn failure(name: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            capabilities: vec![Capability::ExecuteCommand],
            should_succeed: false,
            output: error.into(),
        }
    }

    pub fn with_capabilities(mut self, caps: Vec<Capability>) -> Self {
        self.capabilities = caps;
        self
    }
}

#[async_trait]
impl Tool for MockTool {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { "Mock tool for testing" }
    fn capabilities(&self) -> Vec<Capability> { self.capabilities.clone() }

    async fn execute(&self, _action: &Action, _context: &ExecutionContext) -> ToolResult {
        if self.should_succeed {
            ToolResult::success(&self.output)
        } else {
            ToolResult::error(&self.output)
        }
    }
}

// ── Mock Verifier ───────────────────────────────────────────────────

pub struct MockVerifier {
    name: String,
    should_pass: bool,
}

impl MockVerifier {
    pub fn pass(name: impl Into<String>) -> Self {
        Self { name: name.into(), should_pass: true }
    }

    pub fn fail(name: impl Into<String>) -> Self {
        Self { name: name.into(), should_pass: false }
    }
}

impl Verifier for MockVerifier {
    fn name(&self) -> &str { &self.name }

    fn verify(&self, _action: &Action, _result: &ToolResult) -> Check {
        Check {
            name: self.name.clone(),
            passed: self.should_pass,
            detail: if self.should_pass { "check passed".into() } else { "check failed".into() },
        }
    }
}
