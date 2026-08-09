use crate::common::{Action, Capability, ExecutionContext, Expectation, Trace};
use crate::goal::Goal;
use crate::trace::{CognitiveTrace, TraceNode, TraceNodeKind};
use crate::verification::{
    Check, ExecutionState, Tool, ToolRegistry, ToolResult, Verification, VerifiedResult, Verifier,
};
use async_trait::async_trait;
use friday_knowledge::KnowledgeEngine;
use friday_knowledge::KnowledgeRecord;
use friday_memory::Episode;
use friday_reflection::{Insight, Outcome, OutcomeKind, ReflectionEngine};

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

        // The evidence lifecycle (OMH "Evidence Before Claims"): the action is
        // Prepared, the tool produces raw output (Observed), and only
        // verification may move it to Verified — or to Failed. The trace
        // records the full transition so the audit trail can answer "what did
        // FRIDAY *know* at each step?" not just "what did she do?".
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

        // Trace: Execution — prepared → observed (raw tool output, not yet
        // interpreted; a failed tool is still an observation).
        let exec_node = TraceNode::new(
            TraceNodeKind::Execution,
            serde_json::json!({
                "action_id": action.id,
                "instruction": action.instruction,
                "capability": action.capability.to_string(),
                "state": ExecutionState::Prepared.label(),
            }),
            serde_json::json!({
                "state": ExecutionState::Observed.label(),
                "tool_success": tool_result.success,
            }),
        )
        .with_timing(started_at, tool_finished);
        cognitive_trace.push(exec_node);

        // Trace: Verification — observed → verified | failed.
        let verified = self.verification.verify(action, &tool_result);
        let execution_state = ExecutionState::from_verified(&verified);
        let verify_node = TraceNode::new(
            TraceNodeKind::Verification,
            serde_json::json!({
                "tool_success": verified.tool_success,
                "checks": verified.checks.iter().map(|c| serde_json::json!({
                    "name": c.name,
                    "passed": c.passed,
                })).collect::<Vec<_>>(),
            }),
            serde_json::json!(
                {
                    "verified": verified.verified,
                    "state": execution_state.label(),
                }
            ),
        )
        .with_timing(tool_finished, chrono::Utc::now().timestamp_millis() as u64);
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
            execution_state,
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
        let execution_state = ExecutionState::from_verified(verified);

        Outcome {
            id: format!("outcome_{}", uuid::Uuid::new_v4()),
            kind,
            context: goal.description.clone(),
            detail: serde_json::json!({
                "expectation": expectation.description,
                "expected_success": expectation.expect_success,
                "tool_success": verified.tool_success,
                "verified": verified.verified,
                "execution_state": execution_state.label(),
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
    /// The final epistemic state of the execution (OMH "Evidence Before
    /// Claims"): `Verified` only when observed output passed verification.
    pub execution_state: ExecutionState,
    pub cognitive_trace: CognitiveTrace,
}

impl GoalReport {
    /// Outcome-based completion: `true` when the goal finished as expected
    /// (including expected-failure goals). This is *not* an epistemic claim —
    /// for that, check `execution_state == ExecutionState::Verified`, which
    /// requires observed output to have passed verification first.
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
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "Mock tool for testing"
    }
    fn capabilities(&self) -> Vec<Capability> {
        self.capabilities.clone()
    }

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
        Self {
            name: name.into(),
            should_pass: true,
        }
    }

    pub fn fail(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            should_pass: false,
        }
    }
}

impl Verifier for MockVerifier {
    fn name(&self) -> &str {
        &self.name
    }

    fn verify(&self, _action: &Action, _result: &ToolResult) -> Check {
        Check {
            name: self.name.clone(),
            passed: self.should_pass,
            detail: if self.should_pass {
                "check passed".into()
            } else {
                "check failed".into()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Expectation, Goal, GoalEngine, MockTool, MockVerifier, ToolRegistry};

    // ── ExecutionState machine ─────────────────────────────────────

    #[test]
    fn state_machine_transitions_are_legal() {
        use ExecutionState::*;
        assert_eq!(Prepared.next_allowed(), &[Observed, Cancelled]);
        assert_eq!(Observed.next_allowed(), &[Verified, Failed]);
        assert!(Verified.next_allowed().is_empty());
        assert!(Failed.next_allowed().is_empty());
        assert!(Cancelled.next_allowed().is_empty());
    }

    #[test]
    fn only_verified_failed_cancelled_are_terminal() {
        use ExecutionState::*;
        assert!(!Prepared.is_terminal());
        assert!(!Observed.is_terminal());
        assert!(Verified.is_terminal());
        assert!(Failed.is_terminal());
        assert!(Cancelled.is_terminal());
    }

    #[test]
    fn labels_match_the_wire_vocabulary() {
        use ExecutionState::*;
        assert_eq!(Prepared.label(), "prepared");
        assert_eq!(Observed.label(), "observed");
        assert_eq!(Verified.label(), "verified");
        assert_eq!(Failed.label(), "failed");
        assert_eq!(Cancelled.label(), "cancelled");
    }

    #[test]
    fn state_round_trips_json() {
        for s in [
            ExecutionState::Prepared,
            ExecutionState::Observed,
            ExecutionState::Verified,
            ExecutionState::Failed,
            ExecutionState::Cancelled,
        ] {
            let json = serde_json::to_string(&s).unwrap();
            let back: ExecutionState = serde_json::from_str(&json).unwrap();
            assert_eq!(back, s);
            // The enum's own serialization must agree with the label
            // vocabulary the trace and outcome audit trail use.
            assert_eq!(json, serde_json::json!(s.label()).to_string());
        }
    }

    #[test]
    fn from_verified_is_the_single_derivation_rule() {
        use crate::verification::VerifiedResult;
        let success = VerifiedResult {
            verified: true,
            tool_success: true,
            checks: Vec::new(),
            confidence: crate::confidence::Confidence::high(),
        };
        assert_eq!(
            ExecutionState::from_verified(&success),
            ExecutionState::Verified
        );

        // Tool ran but output failed verification.
        let unverified = VerifiedResult {
            verified: false,
            tool_success: true,
            checks: Vec::new(),
            confidence: crate::confidence::Confidence::medium(),
        };
        assert_eq!(
            ExecutionState::from_verified(&unverified),
            ExecutionState::Failed
        );

        // Tool itself failed — never Verified even without checks.
        let tool_error = VerifiedResult {
            verified: false,
            tool_success: false,
            checks: Vec::new(),
            confidence: crate::confidence::Confidence::low(),
        };
        assert_eq!(
            ExecutionState::from_verified(&tool_error),
            ExecutionState::Failed
        );
    }

    // ── Evidence lifecycle through the GoalEngine ───────────────────

    async fn run_goal(registry: ToolRegistry, verification: Verification) -> GoalReport {
        let mut engine = GoalEngine::new()
            .with_registry(registry)
            .with_verification(verification);
        let goal = Goal::new("cargo build");
        let action = Action::single("cargo build", Capability::ExecuteCommand, &goal.id);
        let exp = Expectation::success("build will succeed");
        let ctx = ExecutionContext::for_goal(&goal.id);
        engine.execute(&goal, &action, &exp, &ctx).await
    }

    #[tokio::test]
    async fn success_advances_prepared_observed_verified() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(MockTool::success("cargo", "Build complete")));
        let report = run_goal(reg, Verification::new()).await;

        // Final epistemic state: verified — and only now is success claimed.
        assert_eq!(report.execution_state, ExecutionState::Verified);
        assert_eq!(
            report.outcome.detail["execution_state"],
            serde_json::json!("verified")
        );
        assert!(report.success());

        // The trace records the full transition: prepared → observed on the
        // Execution node, verified on the Verification node.
        let exec = report
            .cognitive_trace
            .nodes_of_kind(&TraceNodeKind::Execution)
            .first()
            .copied()
            .expect("execution node");
        assert_eq!(exec.input["state"], serde_json::json!("prepared"));
        assert_eq!(exec.output["state"], serde_json::json!("observed"));
        assert_eq!(exec.output["tool_success"], serde_json::json!(true));

        let verify = report
            .cognitive_trace
            .nodes_of_kind(&TraceNodeKind::Verification)
            .first()
            .copied()
            .expect("verification node");
        assert_eq!(verify.output["state"], serde_json::json!("verified"));
    }

    #[tokio::test]
    async fn failed_tool_never_reaches_verified() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(MockTool::failure("cargo", "linker error")));
        let report = run_goal(reg, Verification::new()).await;

        assert_eq!(report.execution_state, ExecutionState::Failed);
        assert!(!report.success());
        assert_eq!(
            report.outcome.detail["execution_state"],
            serde_json::json!("failed")
        );

        let exec = report
            .cognitive_trace
            .nodes_of_kind(&TraceNodeKind::Execution)
            .first()
            .copied()
            .expect("execution node");
        assert_eq!(exec.input["state"], serde_json::json!("prepared"));
        assert_eq!(exec.output["state"], serde_json::json!("observed"));
        assert_eq!(exec.output["tool_success"], serde_json::json!(false));
        assert_eq!(
            report
                .cognitive_trace
                .nodes_of_kind(&TraceNodeKind::Verification)
                .first()
                .copied()
                .expect("verification node")
                .output["state"],
            serde_json::json!("failed")
        );
    }

    #[tokio::test]
    async fn verification_failure_blocks_success_despite_observed_tool_output() {
        // The tool genuinely succeeded (Observed), but the verification gate
        // fails — FRIDAY must not claim success on unverified output.
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(MockTool::success("cargo", "Build complete")));
        let verification = Verification::new().register(Box::new(MockVerifier::fail("checksum")));
        let report = run_goal(reg, verification).await;

        assert_eq!(report.verification.tool_success, true);
        assert_eq!(report.verification.verified, false);
        assert_eq!(report.execution_state, ExecutionState::Failed);
        assert!(!report.success());
        assert_eq!(
            report
                .cognitive_trace
                .nodes_of_kind(&TraceNodeKind::Verification)
                .first()
                .copied()
                .expect("verification node")
                .output["state"],
            serde_json::json!("failed")
        );
    }

    #[tokio::test]
    async fn expected_failure_completes_but_is_not_epistemically_verified() {
        // Documented exemption: an expected-failure goal finishes "as
        // expected" (TaskCompleted → success() == true) while the evidence
        // lifecycle stays Failed — the two axes are outcome vs epistemic.
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(MockTool::failure("cargo", "intentional error")));
        let mut engine = GoalEngine::new().with_registry(reg);
        let goal = Goal::new("expected failure");
        let action = Action::single("fail on purpose", Capability::ExecuteCommand, &goal.id);
        let exp = Expectation::failure("expected to fail");
        let ctx = ExecutionContext::for_goal(&goal.id);
        let report = engine.execute(&goal, &action, &exp, &ctx).await;

        assert_eq!(
            report.outcome.kind,
            friday_reflection::OutcomeKind::TaskCompleted
        );
        assert!(report.success());
        assert_eq!(report.execution_state, ExecutionState::Failed);
        assert!(!report.verification.verified);
    }

    #[tokio::test]
    async fn missing_tool_is_an_observed_failure_not_a_verified_claim() {
        // No tool registered: the action is Prepared, produces an observed
        // error output, and ends Failed — never Verified.
        let report = run_goal(ToolRegistry::new(), Verification::new()).await;

        assert_eq!(report.execution_state, ExecutionState::Failed);
        assert!(!report.success());
        let exec = report
            .cognitive_trace
            .nodes_of_kind(&TraceNodeKind::Execution)
            .first()
            .copied()
            .expect("execution node");
        assert_eq!(exec.input["state"], serde_json::json!("prepared"));
        assert_eq!(exec.output["state"], serde_json::json!("observed"));
    }
}
