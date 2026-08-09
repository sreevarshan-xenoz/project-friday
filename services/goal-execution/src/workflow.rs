//! # Durable Workflow Execution
//!
//! The LangGraph/Temporal concepts, FRIDAY-owned: a [`Workflow`] is a durable
//! sequence of [`WorkflowStep`]s, each executed through the **evidence
//! lifecycle** ([`ExecutionState`]: Prepared → Observed → Verified | Failed)
//! by the [`GoalEngine`]. Between steps the engine records a
//! [`WorkflowCheckpoint`] — a serializable state snapshot — so a crash or an
//! interrupt (approval required, checkpoint boundary) can be resumed from the
//! last durable point. **No lost context, no restarting the reasoning chain.**
//!
//! ```text
//! Goal
//!   │
//!   ▼
//! WorkflowEngine.start()
//!   │
//!   ├── step 0 ──► GoalEngine ──► Verified ──► checkpoint + evidence[0]
//!   ├── step 1 ──► requires approval? ──Pause(ApprovalRequired)──┐
//!   │              (resume after approve)                         │
//!   ├── step 2 ──► Verified ──► checkpoint_after? ──Pause(Boundary)┐
//!   └── ...                                                       │
//!                                                                 ▼
//!                              resume() from the stored checkpoint
//! ```
//!
//! **Durability invariants**
//!
//! - A checkpoint is the **single source of truth** for a workflow's position:
//!   the `next_step` cursor, accumulated `state`, granted `approvals`, per-step
//!   `attempts`, and the full `evidence` trail (every attempt's epistemic
//!   state). Resuming never re-derives or re-runs completed work.
//! - Success is claimed only through the evidence lifecycle: a step is
//!   recorded as `Verified` exactly when the `GoalEngine` returned
//!   `ExecutionState::Verified`. A `Failed` step never appears as progress.
//! - Interrupts are **declarative data** on the step (`requires_approval`,
//!   `checkpoint_after`), matching how skills declare `readonly` — no
//!   special-case code decides when to pause.
//! - Retries are bounded per step (`max_attempts`) and counted in the
//!   checkpoint, so a resumed run can never spin unboundedly.
//!
//! This deliberately complements (not replaces) action-runtime's
//! `LifecycleState` (the process axis) and `ExecutionState` (the evidence
//! axis): the checkpoint is the *durability* axis — where in the workflow the
//! run is, and what it already knows.

use crate::common::{Action, Capability, ExecutionContext, Expectation};
use crate::execution::GoalEngine;
use crate::goal::Goal;
use crate::verification::ExecutionState;
use crate::GoalReport;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// ── Workflow Step ─────────────────────────────────────────────────

/// One unit of durable work. Each step is an action whose execution passes
/// through the evidence lifecycle via the [`GoalEngine`].
///
/// The interrupt points are **declarative data**, not code: a step that
/// requires approval pauses *before* dispatch; a step that checkpoint-after
/// pauses *after* it verifies so the caller can persist and yield control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    /// Human label — what this step is for (audit trail + pause reasons).
    pub label: String,
    /// The action to execute (its capability resolves to a tool).
    pub action: Action,
    /// What we expect to happen — verification compares against this.
    pub expectation: Expectation,
    /// Interrupt before dispatch: the engine pauses and waits for approval.
    pub requires_approval: bool,
    /// Interrupt after verification: the engine pauses at a durable boundary.
    pub checkpoint_after: bool,
    /// Maximum attempts (retries included). `1` = no retry.
    pub max_attempts: u32,
}

impl WorkflowStep {
    pub fn new(label: impl Into<String>, action: Action, expectation: Expectation) -> Self {
        Self {
            label: label.into(),
            action,
            expectation,
            requires_approval: false,
            checkpoint_after: false,
            max_attempts: 1,
        }
    }

    /// Convenience constructor for the common single-capability step.
    pub fn single(
        label: impl Into<String>,
        instruction: impl Into<String>,
        capability: Capability,
        expectation: Expectation,
    ) -> Self {
        let label = label.into();
        let action = Action::single(instruction, capability, &label);
        Self::new(label, action, expectation)
    }

    pub fn with_approval(mut self) -> Self {
        self.requires_approval = true;
        self
    }

    pub fn with_checkpoint(mut self) -> Self {
        self.checkpoint_after = true;
        self
    }

    pub fn with_max_attempts(mut self, max: u32) -> Self {
        self.max_attempts = max.max(1);
        self
    }
}

// ── Evidence Trail ────────────────────────────────────────────────

/// One attempt's durable evidence record — what FRIDAY *knows* about that
/// step, never what she merely intended. Every attempt (including retries) is
/// appended, so the checkpoint carries the full epistemic audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepEvidence {
    pub step_index: usize,
    pub label: String,
    /// Which attempt this was (1-based). >1 means a retry happened.
    pub attempt: u32,
    /// The final epistemic state of this attempt (Verified | Failed).
    pub execution_state: ExecutionState,
    pub verified: bool,
    pub tool_success: bool,
    pub outcome_kind: String,
    pub duration_ms: u64,
}

impl StepEvidence {
    fn from_report(step_index: usize, label: &str, attempt: u32, report: &GoalReport) -> Self {
        Self {
            step_index,
            label: label.to_string(),
            attempt,
            execution_state: report.execution_state,
            verified: report.verification.verified,
            tool_success: report.verification.tool_success,
            outcome_kind: format!("{:?}", report.outcome.kind),
            duration_ms: report.trace.duration_ms(),
        }
    }
}

// ── Checkpoint ────────────────────────────────────────────────────

/// A durable checkpoint — the serializable state snapshot of a workflow.
/// Saved between steps (or on interrupt) and loaded to resume, it is the
/// single source of truth for the workflow's position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCheckpoint {
    pub workflow_id: String,
    /// The next step to run on resume (`== total steps` means complete).
    pub next_step: usize,
    /// Workflow-owned accumulated state (the workflow mutates it through the
    /// `on_step_completed` hook).
    pub state: serde_json::Value,
    /// Granted approvals: step index → approved.
    pub approvals: HashMap<usize, bool>,
    /// Attempts consumed per step (for bounded retries).
    pub attempts: HashMap<usize, u32>,
    /// The durable evidence trail — every attempt's epistemic state.
    pub evidence: Vec<StepEvidence>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl WorkflowCheckpoint {
    pub fn new(workflow_id: impl Into<String>) -> Self {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        Self {
            workflow_id: workflow_id.into(),
            next_step: 0,
            state: serde_json::json!({}),
            approvals: HashMap::new(),
            attempts: HashMap::new(),
            evidence: Vec::new(),
            created_at_ms: now,
            updated_at_ms: now,
        }
    }

    pub fn is_approved(&self, step_index: usize) -> bool {
        self.approvals.get(&step_index).copied().unwrap_or(false)
    }

    /// Record approval for a step (the authority's decision, outside the LLM).
    pub fn grant(&mut self, step_index: usize) {
        self.approvals.insert(step_index, true);
        self.updated_at_ms = chrono::Utc::now().timestamp_millis() as u64;
    }

    pub fn attempts_for(&self, step_index: usize) -> u32 {
        self.attempts.get(&step_index).copied().unwrap_or(0)
    }

    pub fn is_complete(&self, total_steps: usize) -> bool {
        self.next_step >= total_steps
    }
}

// ── Progress ──────────────────────────────────────────────────────

/// Why the engine handed control back to the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PauseReason {
    /// A step requires approval before its action may be dispatched. Resume
    /// with the approval granted (via [`WorkflowEngine::approve`]).
    ApprovalRequired { step_index: usize, label: String },
    /// A step verified at a checkpoint boundary; durable progress is recorded.
    /// Resume to continue.
    CheckpointBoundary { step_index: usize, label: String },
}

/// The outcome of running a workflow up to the next interrupt (or the end).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowProgress {
    /// The workflow paused: resume from the returned checkpoint.
    Paused {
        checkpoint: WorkflowCheckpoint,
        reason: PauseReason,
    },
    /// Every step verified — the workflow ran to completion.
    Completed { checkpoint: WorkflowCheckpoint },
    /// A step failed (evidence lifecycle ended Failed) and retries are
    /// exhausted. The checkpoint records exactly where and why.
    Failed {
        checkpoint: WorkflowCheckpoint,
        step_index: usize,
        message: String,
    },
}

// ── Workflow Trait ────────────────────────────────────────────────

/// FRIDAY-owned durable workflow interface.
///
/// A workflow declares its steps (pure data) and, optionally, how to advance
/// its own accumulated state after each verified step. Everything else —
/// evidence lifecycle, checkpoints, interrupts, retries, resume — is owned by
/// the [`WorkflowEngine`], so workflows stay tiny and swappable.
pub trait Workflow: Send + Sync {
    /// Stable id used to address checkpoints in a [`WorkflowStore`].
    fn workflow_id(&self) -> &str;

    /// The steps in order. **Indices are the resume cursor** — a workflow
    /// must return the same steps (same order, same length) across resumes.
    fn steps(&self) -> &[WorkflowStep];

    /// Hook: advance workflow-owned state after a step verifies. The state is
    /// persisted in the checkpoint, so accumulated facts survive a crash.
    fn on_step_completed(
        &mut self,
        _step: &WorkflowStep,
        _report: &GoalReport,
        _state: &mut serde_json::Value,
    ) {
    }
}

// ── Engine ────────────────────────────────────────────────────────

/// Drives a [`Workflow`] through the evidence lifecycle, checkpointing after
/// every step and pausing at declarative interrupt points.
pub struct WorkflowEngine {
    goal_engine: GoalEngine,
}

impl WorkflowEngine {
    pub fn new(goal_engine: GoalEngine) -> Self {
        Self { goal_engine }
    }

    pub fn goal_engine(&self) -> &GoalEngine {
        &self.goal_engine
    }

    /// Run a workflow from a fresh checkpoint until it completes, pauses, or
    /// fails.
    pub async fn start(&mut self, workflow: &mut dyn Workflow) -> WorkflowProgress {
        let checkpoint = WorkflowCheckpoint::new(workflow.workflow_id());
        self.run_until_interrupt(workflow, checkpoint).await
    }

    /// Resume from a durable checkpoint. Already-verified steps are never
    /// re-run: the evidence trail and `next_step` cursor are the truth.
    ///
    /// A checkpoint whose `workflow_id` does not match the workflow being
    /// resumed is rejected as `Failed` — resuming foreign state silently would
    /// run the wrong steps from the wrong cursor.
    pub async fn resume(
        &mut self,
        workflow: &mut dyn Workflow,
        checkpoint: WorkflowCheckpoint,
    ) -> WorkflowProgress {
        if checkpoint.workflow_id != workflow.workflow_id() {
            let foreign_id = checkpoint.workflow_id.clone();
            return WorkflowProgress::Failed {
                checkpoint,
                step_index: 0,
                message: format!(
                    "checkpoint belongs to workflow '{foreign_id}' but resuming workflow '{}'",
                    workflow.workflow_id()
                ),
            };
        }
        self.run_until_interrupt(workflow, checkpoint).await
    }

    /// Record the authority's approval for a step, then resume.
    pub async fn approve_and_resume(
        &mut self,
        workflow: &mut dyn Workflow,
        mut checkpoint: WorkflowCheckpoint,
        step_index: usize,
    ) -> WorkflowProgress {
        checkpoint.grant(step_index);
        self.run_until_interrupt(workflow, checkpoint).await
    }

    /// Grant approval on a checkpoint without resuming (e.g. persist first).
    pub fn approve(&self, checkpoint: &mut WorkflowCheckpoint, step_index: usize) {
        checkpoint.grant(step_index);
    }

    async fn run_until_interrupt(
        &mut self,
        workflow: &mut dyn Workflow,
        mut cp: WorkflowCheckpoint,
    ) -> WorkflowProgress {
        // A stale cursor past the end of a *shrunken* workflow must not fall
        // through to Completed with evidence that no longer matches.
        if cp.next_step > workflow.steps().len() {
            let cursor = cp.next_step;
            let len = workflow.steps().len();
            return WorkflowProgress::Failed {
                checkpoint: cp,
                step_index: cursor,
                message: format!(
                    "stale checkpoint: cursor at step {cursor} but the workflow has {len} step(s)"
                ),
            };
        }

        while let Some(step) = workflow.steps().get(cp.next_step).cloned() {
            let index = cp.next_step;
            let label = step.label.clone();

            // A step whose attempt budget was consumed in a prior run must not
            // execute again — retries are counted in the checkpoint across
            // resumes, not just within one run.
            if cp.attempts_for(index) >= step.max_attempts {
                return WorkflowProgress::Failed {
                    checkpoint: cp,
                    step_index: index,
                    message: format!(
                        "step '{}' exhausted its {} attempt(s) in a prior run",
                        label, step.max_attempts
                    ),
                };
            }

            // Interrupt 1: approval gate — pause *before* dispatch. The action
            // is Prepared but never observed until an authority approves.
            if step.requires_approval && !cp.is_approved(index) {
                return WorkflowProgress::Paused {
                    checkpoint: cp,
                    reason: PauseReason::ApprovalRequired {
                        step_index: index,
                        label,
                    },
                };
            }

            // Execute through the evidence lifecycle, retrying within bounds.
            // Every attempt is appended to the evidence trail — a failed
            // attempt is an honest observation, never silently overwritten by
            // the eventual success.
            let goal = Goal::new(&step.label);
            let ctx = ExecutionContext::for_goal(&goal.id);
            let report = loop {
                let attempt = cp.attempts_for(index) + 1;
                cp.attempts.insert(index, attempt);
                let report = self
                    .goal_engine
                    .execute(&goal, &step.action, &step.expectation, &ctx)
                    .await;
                cp.evidence
                    .push(StepEvidence::from_report(index, &label, attempt, &report));
                if report.execution_state == ExecutionState::Verified
                    || attempt >= step.max_attempts
                {
                    break report;
                }
            };
            let attempts = cp.attempts_for(index);
            cp.updated_at_ms = chrono::Utc::now().timestamp_millis() as u64;

            if report.execution_state == ExecutionState::Verified {
                // Advance workflow-owned state (durable in the checkpoint).
                workflow.on_step_completed(&step, &report, &mut cp.state);
                cp.next_step += 1;

                // Interrupt 2: durable checkpoint boundary after verification.
                if step.checkpoint_after {
                    return WorkflowProgress::Paused {
                        checkpoint: cp,
                        reason: PauseReason::CheckpointBoundary {
                            step_index: index,
                            label,
                        },
                    };
                }
            } else {
                // Evidence says Failed and retries are exhausted — the
                // workflow is stuck exactly here.
                return WorkflowProgress::Failed {
                    checkpoint: cp,
                    step_index: index,
                    message: format!(
                        "step '{}' failed after {} attempt(s): {}",
                        label,
                        attempts,
                        report
                            .outcome
                            .detail
                            .get("execution_state")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unverified")
                    ),
                };
            }
        }

        WorkflowProgress::Completed { checkpoint: cp }
    }
}

// ── Stores ────────────────────────────────────────────────────────

/// Durable persistence for checkpoints — the "resume after a crash" seam.
#[async_trait]
pub trait WorkflowStore: Send + Sync {
    async fn save(&self, checkpoint: &WorkflowCheckpoint) -> Result<(), WorkflowError>;
    async fn load(&self, workflow_id: &str) -> Result<Option<WorkflowCheckpoint>, WorkflowError>;
    async fn delete(&self, workflow_id: &str) -> Result<(), WorkflowError>;
    async fn list(&self) -> Result<Vec<String>, WorkflowError>;
}

/// In-memory store — tests and short-lived runtimes.
#[derive(Default)]
pub struct InMemoryWorkflowStore {
    checkpoints: Mutex<HashMap<String, WorkflowCheckpoint>>,
}

impl InMemoryWorkflowStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl WorkflowStore for InMemoryWorkflowStore {
    async fn save(&self, checkpoint: &WorkflowCheckpoint) -> Result<(), WorkflowError> {
        self.checkpoints
            .lock()
            .expect("workflow store poisoned")
            .insert(checkpoint.workflow_id.clone(), checkpoint.clone());
        Ok(())
    }

    async fn load(&self, workflow_id: &str) -> Result<Option<WorkflowCheckpoint>, WorkflowError> {
        Ok(self
            .checkpoints
            .lock()
            .expect("workflow store poisoned")
            .get(workflow_id)
            .cloned())
    }

    async fn delete(&self, workflow_id: &str) -> Result<(), WorkflowError> {
        self.checkpoints
            .lock()
            .expect("workflow store poisoned")
            .remove(workflow_id);
        Ok(())
    }

    async fn list(&self) -> Result<Vec<String>, WorkflowError> {
        Ok(self
            .checkpoints
            .lock()
            .expect("workflow store poisoned")
            .keys()
            .cloned()
            .collect())
    }
}

/// JSON-file store: one file per workflow (`<workflow_id>.json`) under `dir`.
/// Writes are atomic (temp file + rename) so a crash mid-save never corrupts
/// the last durable checkpoint.
pub struct JsonWorkflowStore {
    dir: PathBuf,
}

impl JsonWorkflowStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn path(&self, workflow_id: &str) -> PathBuf {
        self.dir.join(format!("{}.json", sanitize_id(workflow_id)))
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<(), WorkflowError> {
        std::fs::create_dir_all(&self.dir).map_err(|e| WorkflowError::Io(e.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes).map_err(|e| WorkflowError::Io(e.to_string()))?;
        std::fs::rename(&tmp, path).map_err(|e| WorkflowError::Io(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl WorkflowStore for JsonWorkflowStore {
    async fn save(&self, checkpoint: &WorkflowCheckpoint) -> Result<(), WorkflowError> {
        let json = serde_json::to_vec_pretty(checkpoint).map_err(WorkflowError::from)?;
        self.write_atomic(&self.path(&checkpoint.workflow_id), &json)
    }

    async fn load(&self, workflow_id: &str) -> Result<Option<WorkflowCheckpoint>, WorkflowError> {
        let path = self.path(workflow_id);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path).map_err(|e| WorkflowError::Io(e.to_string()))?;
        let cp = serde_json::from_slice(&bytes).map_err(WorkflowError::from)?;
        Ok(Some(cp))
    }

    async fn delete(&self, workflow_id: &str) -> Result<(), WorkflowError> {
        let path = self.path(workflow_id);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| WorkflowError::Io(e.to_string()))?;
        }
        Ok(())
    }

    async fn list(&self) -> Result<Vec<String>, WorkflowError> {
        let mut ids = Vec::new();
        if !self.dir.exists() {
            return Ok(ids);
        }
        for entry in std::fs::read_dir(&self.dir).map_err(|e| WorkflowError::Io(e.to_string()))? {
            let entry = entry.map_err(|e| WorkflowError::Io(e.to_string()))?;
            if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = entry.path().file_stem().and_then(|s| s.to_str()) {
                    ids.push(stem.to_string());
                }
            }
        }
        Ok(ids)
    }
}

/// Filenames must be filesystem-safe: keep alphanumerics, `-`, `_`.
fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ── Errors ────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum WorkflowError {
    Io(String),
    Json(String),
}

impl From<serde_json::Error> for WorkflowError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e.to_string())
    }
}

impl fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "workflow store io: {msg}"),
            Self::Json(msg) => write!(f, "workflow checkpoint json: {msg}"),
        }
    }
}

impl std::error::Error for WorkflowError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verification::{Tool, ToolRegistry, ToolResult};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // ── Test doubles ─────────────────────────────────────────────

    struct AlwaysOkTool;
    #[async_trait]
    impl Tool for AlwaysOkTool {
        fn name(&self) -> &str {
            "ok"
        }
        fn description(&self) -> &str {
            "always succeeds"
        }
        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::ExecuteCommand]
        }
        async fn execute(&self, _a: &Action, _c: &ExecutionContext) -> ToolResult {
            ToolResult::success("done")
        }
    }

    /// Fails only calls whose instruction contains `marker`; everything else
    /// succeeds. The registry resolves by capability, so failures must be
    /// instruction-targeted or every step in the workflow would fail.
    #[derive(Clone)]
    struct SelectiveFailTool {
        marker: String,
        remaining_failures: Arc<AtomicUsize>,
    }

    impl SelectiveFailTool {
        fn fail(marker: &str) -> Self {
            Self {
                marker: marker.to_string(),
                remaining_failures: Arc::new(AtomicUsize::new(usize::MAX)),
            }
        }
        /// Fail only the first `n` matching calls, then succeed (flaky).
        fn fail_first(marker: &str, n: usize) -> Self {
            Self {
                marker: marker.to_string(),
                remaining_failures: Arc::new(AtomicUsize::new(n)),
            }
        }
    }

    #[async_trait]
    impl Tool for SelectiveFailTool {
        fn name(&self) -> &str {
            "selective-fail"
        }
        fn description(&self) -> &str {
            "fails instructions containing its marker"
        }
        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::ExecuteCommand]
        }
        async fn execute(&self, action: &Action, _c: &ExecutionContext) -> ToolResult {
            if action.instruction.contains(&self.marker)
                && self.remaining_failures.load(Ordering::SeqCst) > 0
            {
                self.remaining_failures.fetch_sub(1, Ordering::SeqCst);
                ToolResult::error(format!("boom: {}", self.marker))
            } else {
                ToolResult::success("done")
            }
        }
    }

    fn ok_step(label: &str) -> WorkflowStep {
        WorkflowStep::single(
            label,
            format!("run {label}"),
            Capability::ExecuteCommand,
            crate::Expectation::success("will succeed"),
        )
    }

    struct TestWorkflow {
        id: String,
        steps: Vec<WorkflowStep>,
        /// Hook accumulates a counter into checkpoint state.
        accumulate: bool,
    }

    impl TestWorkflow {
        fn new(id: &str, steps: Vec<WorkflowStep>) -> Self {
            Self {
                id: id.to_string(),
                steps,
                accumulate: false,
            }
        }
        fn accumulating(mut self) -> Self {
            self.accumulate = true;
            self
        }
    }

    impl Workflow for TestWorkflow {
        fn workflow_id(&self) -> &str {
            &self.id
        }
        fn steps(&self) -> &[WorkflowStep] {
            &self.steps
        }
        fn on_step_completed(
            &mut self,
            step: &WorkflowStep,
            _report: &GoalReport,
            state: &mut serde_json::Value,
        ) {
            if self.accumulate {
                let n = state["completed"].as_u64().unwrap_or(0);
                state["completed"] = serde_json::json!(n + 1);
                state["last"] = serde_json::json!(step.label);
            }
        }
    }

    fn engine_with(tools: Vec<Box<dyn Tool>>) -> WorkflowEngine {
        let mut reg = ToolRegistry::new();
        for t in tools {
            reg.register(t);
        }
        WorkflowEngine::new(crate::GoalEngine::new().with_registry(reg))
    }

    // ── Completion ───────────────────────────────────────────────

    #[tokio::test]
    async fn completes_all_steps_with_verified_evidence() {
        let mut engine = engine_with(vec![Box::new(AlwaysOkTool)]);
        let mut wf = TestWorkflow::new(
            "wf-complete",
            vec![ok_step("a"), ok_step("b"), ok_step("c")],
        );

        let progress = engine.start(&mut wf).await;
        match progress {
            WorkflowProgress::Completed { checkpoint } => {
                assert_eq!(checkpoint.next_step, 3);
                assert_eq!(checkpoint.evidence.len(), 3);
                assert!(checkpoint
                    .evidence
                    .iter()
                    .all(|e| e.execution_state == ExecutionState::Verified));
                assert!(checkpoint.evidence.iter().all(|e| e.attempt == 1));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn empty_workflow_completes_immediately() {
        let mut engine = engine_with(vec![Box::new(AlwaysOkTool)]);
        let mut wf = TestWorkflow::new("wf-empty", vec![]);
        match engine.start(&mut wf).await {
            WorkflowProgress::Completed { checkpoint } => {
                assert!(checkpoint.evidence.is_empty());
                assert_eq!(checkpoint.next_step, 0);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    // ── Approval interrupt ───────────────────────────────────────

    #[tokio::test]
    async fn approval_gate_pauses_before_dispatch_and_resumes() {
        let mut engine = engine_with(vec![Box::new(AlwaysOkTool)]);
        let mut wf = TestWorkflow::new(
            "wf-approval",
            vec![ok_step("a"), ok_step("risky").with_approval(), ok_step("c")],
        );

        // Runs step 0, pauses BEFORE step 1 (risky): evidence has only step 0.
        match engine.start(&mut wf).await {
            WorkflowProgress::Paused {
                checkpoint,
                reason: PauseReason::ApprovalRequired { step_index, label },
            } => {
                assert_eq!(step_index, 1);
                assert_eq!(label, "risky");
                assert_eq!(checkpoint.next_step, 1);
                assert_eq!(checkpoint.evidence.len(), 1);
                assert!(!checkpoint.is_approved(1));
            }
            other => panic!("expected approval pause, got {other:?}"),
        }

        // Without approval, resuming pauses again without running the step.
        let paused = engine.start(&mut wf).await;
        let checkpoint = match paused {
            WorkflowProgress::Paused { checkpoint, .. } => checkpoint,
            other => panic!("expected pause, got {other:?}"),
        };
        assert_eq!(checkpoint.evidence.len(), 1, "risky step must not have run");

        // Approve → resume → everything verifies.
        match engine.approve_and_resume(&mut wf, checkpoint, 1).await {
            WorkflowProgress::Completed { checkpoint } => {
                assert_eq!(checkpoint.evidence.len(), 3);
                assert_eq!(
                    checkpoint.evidence[1].execution_state,
                    ExecutionState::Verified
                );
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn approval_survives_checkpoint_serialization() {
        let mut engine = engine_with(vec![Box::new(AlwaysOkTool)]);
        let mut wf = TestWorkflow::new(
            "wf-approval-json",
            vec![ok_step("a").with_approval(), ok_step("b")],
        );

        let checkpoint = match engine.start(&mut wf).await {
            WorkflowProgress::Paused { checkpoint, .. } => checkpoint,
            other => panic!("expected pause, got {other:?}"),
        };
        assert!(!checkpoint.is_approved(0));

        let mut approved = checkpoint.clone();
        approved.grant(0);
        let json = serde_json::to_string(&approved).unwrap();
        let back: WorkflowCheckpoint = serde_json::from_str(&json).unwrap();
        assert!(back.is_approved(0));

        match engine.approve_and_resume(&mut wf, back, 0).await {
            WorkflowProgress::Completed { checkpoint } => {
                assert_eq!(checkpoint.evidence.len(), 2);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    // ── Checkpoint boundary interrupt ────────────────────────────

    #[tokio::test]
    async fn boundary_pauses_after_step_and_resumes() {
        let mut engine = engine_with(vec![Box::new(AlwaysOkTool)]);
        let mut wf = TestWorkflow::new(
            "wf-boundary",
            vec![ok_step("a"), ok_step("b").with_checkpoint(), ok_step("c")],
        );

        // Step 0 verifies, step 1 verifies, then pauses at the boundary.
        let checkpoint = match engine.start(&mut wf).await {
            WorkflowProgress::Paused {
                checkpoint,
                reason: PauseReason::CheckpointBoundary { step_index, label },
            } => {
                assert_eq!(step_index, 1);
                assert_eq!(label, "b");
                assert_eq!(checkpoint.next_step, 2);
                assert_eq!(checkpoint.evidence.len(), 2);
                checkpoint
            }
            other => panic!("expected boundary pause, got {other:?}"),
        };

        match engine.resume(&mut wf, checkpoint).await {
            WorkflowProgress::Completed { checkpoint } => {
                assert_eq!(checkpoint.evidence.len(), 3);
                assert!(checkpoint.is_complete(3));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    // ── Resume semantics ─────────────────────────────────────────

    #[tokio::test]
    async fn resume_never_reruns_verified_steps() {
        let mut engine = engine_with(vec![Box::new(AlwaysOkTool)]);
        let mut wf = TestWorkflow::new(
            "wf-resume",
            vec![ok_step("a").with_checkpoint(), ok_step("b"), ok_step("c")],
        );

        // Run to the boundary after step 0 (the "crash" point).
        let saved = match engine.start(&mut wf).await {
            WorkflowProgress::Paused { checkpoint, .. } => checkpoint,
            other => panic!("expected pause, got {other:?}"),
        };
        assert_eq!(saved.next_step, 1);
        assert_eq!(saved.evidence.len(), 1);

        // A *fresh* engine resumes from the stored checkpoint (crash + restart).
        let mut engine2 = engine_with(vec![Box::new(AlwaysOkTool)]);
        let mut wf2 = TestWorkflow::new("wf-resume", wf.steps().to_vec());
        match engine2.resume(&mut wf2, saved).await {
            WorkflowProgress::Completed { checkpoint } => {
                assert_eq!(checkpoint.evidence.len(), 3);
                // Step 0 was verified in the prior run — evidence carries it.
                assert_eq!(
                    checkpoint.evidence[0].execution_state,
                    ExecutionState::Verified
                );
                assert_eq!(checkpoint.evidence[0].attempt, 1);
                // The resumed steps are b (1) and c (2).
                assert_eq!(checkpoint.evidence[1].label, "b");
                assert_eq!(checkpoint.evidence[2].label, "c");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    // ── Failure + retry ──────────────────────────────────────────

    #[tokio::test]
    async fn failed_step_without_retry_fails_workflow() {
        let mut engine = engine_with(vec![Box::new(SelectiveFailTool::fail("boom"))]);
        let mut wf =
            TestWorkflow::new("wf-fail", vec![ok_step("a"), ok_step("boom"), ok_step("c")]);

        match engine.start(&mut wf).await {
            WorkflowProgress::Failed {
                checkpoint,
                step_index,
                message,
            } => {
                assert_eq!(step_index, 1);
                assert!(message.contains("boom"));
                assert_eq!(checkpoint.next_step, 1, "cursor stops at the failed step");
                assert_eq!(checkpoint.evidence.len(), 2);
                assert_eq!(
                    checkpoint.evidence[1].execution_state,
                    ExecutionState::Failed
                );
                assert_eq!(checkpoint.evidence[1].attempt, 1);
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn bounded_retry_recovers_after_transient_failure() {
        let mut engine = engine_with(vec![Box::new(SelectiveFailTool::fail_first("flaky", 2))]);
        let mut wf = TestWorkflow::new(
            "wf-retry",
            vec![
                ok_step("a"),
                ok_step("flaky").with_max_attempts(3),
                ok_step("c"),
            ],
        );

        match engine.start(&mut wf).await {
            WorkflowProgress::Completed { checkpoint } => {
                // Step 1: attempt 1 failed, attempt 2 failed, attempt 3 verified.
                let flaky: Vec<_> = checkpoint
                    .evidence
                    .iter()
                    .filter(|e| e.step_index == 1)
                    .collect();
                assert_eq!(flaky.len(), 3);
                assert_eq!(flaky[0].execution_state, ExecutionState::Failed);
                assert_eq!(flaky[1].execution_state, ExecutionState::Failed);
                assert_eq!(flaky[2].execution_state, ExecutionState::Verified);
                assert_eq!(flaky[2].attempt, 3);
                assert_eq!(checkpoint.attempts_for(1), 3);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn exhausted_retries_fail_with_attempt_trail() {
        let mut engine = engine_with(vec![Box::new(SelectiveFailTool::fail("doomed"))]);
        let mut wf = TestWorkflow::new(
            "wf-exhaust",
            vec![ok_step("a"), ok_step("doomed").with_max_attempts(2)],
        );

        match engine.start(&mut wf).await {
            WorkflowProgress::Failed {
                checkpoint,
                step_index,
                ..
            } => {
                assert_eq!(step_index, 1);
                let doomed: Vec<_> = checkpoint
                    .evidence
                    .iter()
                    .filter(|e| e.step_index == 1)
                    .collect();
                assert_eq!(doomed.len(), 2, "both attempts recorded");
                assert!(doomed
                    .iter()
                    .all(|e| e.execution_state == ExecutionState::Failed));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resume_after_failure_consumes_no_extra_attempt() {
        let mut engine = engine_with(vec![Box::new(SelectiveFailTool::fail("doomed"))]);
        let mut wf = TestWorkflow::new(
            "wf-resume-fail",
            vec![ok_step("a"), ok_step("doomed").with_max_attempts(2)],
        );

        let failed = match engine.start(&mut wf).await {
            WorkflowProgress::Failed { checkpoint, .. } => checkpoint,
            other => panic!("expected Failed, got {other:?}"),
        };
        assert_eq!(failed.attempts_for(1), 2);

        // Resuming the failed checkpoint must NOT burn another attempt: the
        // budget is counted in the checkpoint across resumes.
        match engine.resume(&mut wf, failed).await {
            WorkflowProgress::Failed {
                checkpoint,
                message,
                ..
            } => {
                assert!(message.contains("prior run"), "got: {message}");
                assert_eq!(checkpoint.attempts_for(1), 2, "no extra attempt consumed");
                // Step 0 (1 entry) + doomed attempts 1+2 (2 entries) = 3;
                // the resume added nothing.
                assert_eq!(checkpoint.evidence.len(), 3, "no extra evidence entry");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn foreign_checkpoint_is_rejected_not_resumed() {
        let mut engine = engine_with(vec![Box::new(AlwaysOkTool)]);
        let mut wf = TestWorkflow::new("wf-foreign", vec![ok_step("a")]);

        // A checkpoint for a *different* workflow must not be resumed.
        let foreign = WorkflowCheckpoint::new("someone-else");
        match engine.resume(&mut wf, foreign).await {
            WorkflowProgress::Failed { message, .. } => {
                assert!(message.contains("someone-else"), "got: {message}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stale_cursor_into_shrunken_workflow_is_failed_not_completed() {
        let mut engine = engine_with(vec![Box::new(AlwaysOkTool)]);
        let mut wf = TestWorkflow::new("wf-shrunk", vec![ok_step("a")]);

        // A checkpoint whose cursor points past the end of a now-smaller
        // workflow must fail loudly instead of silently claiming completion.
        let mut stale = WorkflowCheckpoint::new("wf-shrunk");
        stale.next_step = 5;
        match engine.resume(&mut wf, stale).await {
            WorkflowProgress::Failed { message, .. } => {
                assert!(message.contains("stale checkpoint"), "got: {message}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    // ── State accumulation hook ──────────────────────────────────

    #[tokio::test]
    async fn on_step_completed_accumulates_durable_state() {
        let mut engine = engine_with(vec![Box::new(AlwaysOkTool)]);
        let mut wf = TestWorkflow::new("wf-state", vec![ok_step("x"), ok_step("y"), ok_step("z")])
            .accumulating();

        match engine.start(&mut wf).await {
            WorkflowProgress::Completed { checkpoint } => {
                assert_eq!(checkpoint.state["completed"], serde_json::json!(3));
                assert_eq!(checkpoint.state["last"], serde_json::json!("z"));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    // ── Stores ──────────────────────────────────────────────────

    #[tokio::test]
    async fn in_memory_store_round_trips() {
        let store = InMemoryWorkflowStore::new();
        let mut wf =
            TestWorkflow::new("wf-store", vec![ok_step("a").with_approval(), ok_step("b")]);
        let mut engine = engine_with(vec![Box::new(AlwaysOkTool)]);
        let checkpoint = match engine.start(&mut wf).await {
            WorkflowProgress::Paused { checkpoint, .. } => checkpoint,
            other => panic!("expected pause, got {other:?}"),
        };

        store.save(&checkpoint).await.unwrap();
        assert_eq!(store.list().await.unwrap(), vec!["wf-store".to_string()]);
        let loaded = store.load("wf-store").await.unwrap().unwrap();
        assert_eq!(loaded.next_step, checkpoint.next_step);
        assert_eq!(loaded.evidence.len(), checkpoint.evidence.len());

        store.delete("wf-store").await.unwrap();
        assert!(store.load("wf-store").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn json_store_persists_and_loads_atomically() {
        let dir = std::env::temp_dir().join(format!("friday-wf-{}", uuid::Uuid::new_v4()));
        let store = JsonWorkflowStore::new(&dir);
        let mut engine = engine_with(vec![Box::new(AlwaysOkTool)]);
        let mut wf = TestWorkflow::new(
            "wf-json",
            vec![ok_step("a").with_checkpoint(), ok_step("b")],
        );

        let checkpoint = match engine.start(&mut wf).await {
            WorkflowProgress::Paused { checkpoint, .. } => checkpoint,
            other => panic!("expected pause, got {other:?}"),
        };

        store.save(&checkpoint).await.unwrap();
        assert!(store.path("wf-json").exists());
        assert_eq!(store.list().await.unwrap(), vec!["wf-json".to_string()]);

        // Fresh engine + fresh store instance resume from disk (crash + restart).
        let store2 = JsonWorkflowStore::new(&dir);
        let loaded = store2.load("wf-json").await.unwrap().unwrap();
        assert_eq!(loaded.evidence.len(), 1);
        let mut engine2 = engine_with(vec![Box::new(AlwaysOkTool)]);
        let mut wf2 = TestWorkflow::new("wf-json", wf.steps().to_vec());
        match engine2.resume(&mut wf2, loaded).await {
            WorkflowProgress::Completed { checkpoint } => {
                assert_eq!(checkpoint.evidence.len(), 2);
            }
            other => panic!("expected Completed, got {other:?}"),
        }

        store2.delete("wf-json").await.unwrap();
        assert!(store2.load("wf-json").await.unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn json_store_missing_workflow_returns_none() {
        let dir = std::env::temp_dir().join(format!("friday-wf-none-{}", uuid::Uuid::new_v4()));
        let store = JsonWorkflowStore::new(&dir);
        assert!(store.load("absent").await.unwrap().is_none());
        assert!(store.list().await.unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoint_round_trips_serde() {
        let mut cp = WorkflowCheckpoint::new("wf-ser");
        cp.grant(0);
        cp.attempts.insert(1, 2);
        cp.state = serde_json::json!({ "facts": ["a", "b"] });
        cp.evidence.push(StepEvidence {
            step_index: 0,
            label: "x".into(),
            attempt: 1,
            execution_state: ExecutionState::Verified,
            verified: true,
            tool_success: true,
            outcome_kind: "TaskCompleted".into(),
            duration_ms: 5,
        });

        let json = serde_json::to_string_pretty(&cp).unwrap();
        let back: WorkflowCheckpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(back.workflow_id, "wf-ser");
        assert!(back.is_approved(0));
        assert_eq!(back.attempts_for(1), 2);
        assert_eq!(back.evidence[0].execution_state, ExecutionState::Verified);
        assert_eq!(back.state["facts"], serde_json::json!(["a", "b"]));
    }

    #[test]
    fn sanitize_id_makes_filesystem_safe_names() {
        assert_eq!(sanitize_id("wf/one:two"), "wf_one_two");
        assert_eq!(sanitize_id("plain-id_1"), "plain-id_1");
    }
}
