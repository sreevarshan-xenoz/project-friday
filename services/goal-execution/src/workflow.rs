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
//!   `attempts`, the full `evidence` trail (every attempt's epistemic state),
//!   and the append-only `journal`. Resuming never re-derives or re-runs
//!   completed work.
//! - Success is claimed only through the evidence lifecycle: a step is
//!   recorded as `Verified` exactly when the `GoalEngine` returned
//!   `ExecutionState::Verified`. A `Failed` step never appears as progress.
//! - Interrupts are **declarative data** on the step (`requires_approval`,
//!   `checkpoint_after`), matching how skills declare `readonly` — no
//!   special-case code decides when to pause.
//! - Retries are bounded per step (`max_attempts`) and counted in the
//!   checkpoint, so a resumed run can never spin unboundedly.
//! - **In-flight journaling (write-ahead).** Every dispatch records a journal
//!   entry with a **stable idempotency key** before it runs; when the engine
//!   holds a [`WorkflowStore`] it persists that write-ahead, so a crash
//!   mid-step resumes with an explicit, observable recovery
//!   (`checkpoint.recovered`) instead of a silent re-run. A step or stage
//!   child whose journal shows a `Verified` outcome is **never re-dispatched**
//!   (idempotent replay).
//! - **Parallel stages (map/reduce).** A [`WorkflowStep::stage`] fans out into
//!   bounded-concurrency children — the Agent Director topology made durable.
//!   Each child journals and verifies independently; a crash mid-stage resumes
//!   by replaying the Verified children and re-executing only the unfinished
//!   ones. `on_stage_completed` folds the verified outputs into state.
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
use std::sync::{Arc, Mutex};

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
    /// When `Some`, this step is a **parallel stage**: the cursor occupies one
    /// position but the engine fans out into the stage's children with bounded
    /// concurrency (see [`StageBody`]). The stage's `action` is a marker that
    /// is never dispatched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<StageBody>,
}

/// The body of a parallel stage: children executed concurrently (in waves of
/// `max_parallel`), each through the evidence lifecycle with its own retries,
/// journal entries, and evidence. A stage verifies only when **every** child
/// verifies (after each child's own bounded retries); the workflow's
/// [`Workflow::on_stage_completed`] hook folds the verified outputs into
/// checkpoint state (map/reduce).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageBody {
    /// The children to run. Each is a full [`WorkflowStep`] with its own
    /// expectation and `max_attempts`. Children may not require approval.
    pub children: Vec<WorkflowStep>,
    /// Bounded concurrency: at most this many children dispatch at once.
    pub max_parallel: usize,
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
            stage: None,
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

    /// A **parallel stage** — one cursor position that fans out into `children`
    /// with bounded concurrency (`max_parallel`, default 1 child at a time).
    /// The stage verifies only when every child verifies; child failures are
    /// retried up to each child's own `max_attempts` first.
    pub fn stage(
        label: impl Into<String>,
        children: Vec<WorkflowStep>,
        max_parallel: usize,
    ) -> Self {
        let label = label.into();
        let action = Action::single(
            format!("run parallel stage: {label}"),
            Capability::Custom("workflow.stage".into()),
            &label,
        );
        Self {
            label: label.clone(),
            action,
            expectation: crate::Expectation::success("parallel stage"),
            requires_approval: false,
            checkpoint_after: false,
            max_attempts: 1,
            stage: Some(StageBody {
                children,
                max_parallel: max_parallel.max(1),
            }),
        }
    }
}

// ── Evidence Trail ────────────────────────────────────────────────

/// One attempt's durable evidence record — what FRIDAY *knows* about that
/// step, never what she merely intended. Every attempt (including retries) is
/// appended, so the checkpoint carries the full epistemic audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepEvidence {
    pub step_index: usize,
    /// `Some(i)` when this evidence belongs to stage child `i` of a parallel
    /// stage occupying `step_index`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_index: Option<usize>,
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
    fn from_report(
        step_index: usize,
        sub_index: Option<usize>,
        label: &str,
        attempt: u32,
        report: &GoalReport,
    ) -> Self {
        Self {
            step_index,
            sub_index,
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

// ── Journal ───────────────────────────────────────────────────────

/// The lifecycle of one journaled dispatch: written **InFlight** (write-ahead)
/// before the action runs, then flipped to a terminal state once the evidence
/// lifecycle produced an outcome.
///
/// A checkpoint that still holds an `InFlight` entry was saved mid-dispatch:
/// that attempt's outcome was lost to a crash. The engine recovers it
/// explicitly (`checkpoint.recovered`), never silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JournalState {
    /// Dispatched, outcome not yet captured (write-ahead record).
    InFlight,
    /// The evidence lifecycle ended Verified for this attempt.
    Verified,
    /// The evidence lifecycle ended Failed for this attempt.
    Failed,
}

/// One append-only journal entry — the write-ahead record of a single
/// dispatch, keyed for idempotency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Stable idempotency key: the same work (workflow, step, attempt,
    /// instruction) re-derives the same key, so a `Verified` entry can never
    /// be dispatched twice across runs.
    pub key: String,
    pub step_index: usize,
    /// `Some(i)` = stage child `i` of a parallel stage at `step_index`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_index: Option<usize>,
    pub label: String,
    pub attempt: u32,
    pub state: JournalState,
    pub duration_ms: u64,
}

impl JournalEntry {
    pub fn in_flight(
        key: String,
        step_index: usize,
        sub_index: Option<usize>,
        label: &str,
        attempt: u32,
    ) -> Self {
        Self {
            key,
            step_index,
            sub_index,
            label: label.into(),
            attempt,
            state: JournalState::InFlight,
            duration_ms: 0,
        }
    }

    pub fn terminal(
        key: String,
        step_index: usize,
        sub_index: Option<usize>,
        label: &str,
        attempt: u32,
        state: JournalState,
        duration_ms: u64,
    ) -> Self {
        Self {
            key,
            step_index,
            sub_index,
            label: label.into(),
            attempt,
            state,
            duration_ms,
        }
    }
}

/// Derive a step dispatch's stable idempotency key. FNV-1a 64-bit — chosen
/// over `std::collections::hash_map::DefaultHasher` because it is stable
/// across runs and Rust versions, so a resume derives exactly the keys a prior
/// run wrote.
pub fn journal_key(
    workflow_id: &str,
    step_index: usize,
    sub_index: Option<usize>,
    attempt: u32,
    instruction: &str,
) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in workflow_id.bytes().chain(instruction.bytes()) {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let sub = sub_index
        .map(|s| s.to_string())
        .unwrap_or_else(|| "-".to_string());
    format!("{workflow_id}:{step_index}:{sub}:{attempt}:{hash:016x}")
}

/// One child's verified outcome after a parallel stage — what the workflow's
/// `on_stage_completed` hook folds into state (map/reduce).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageChildOutcome {
    pub sub_index: usize,
    pub label: String,
    pub verified: bool,
    /// The child's execution output. Empty for replayed children (verified in
    /// a prior run) — their durable record is the evidence trail.
    pub output: String,
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
    /// The append-only write-ahead journal (see [`JournalEntry`]). Old
    /// checkpoints without it deserialize as empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub journal: Vec<JournalEntry>,
    /// How many mid-dispatch recoveries this workflow has survived: a saved
    /// `InFlight` entry was re-executed after a crash. Durable + observable.
    #[serde(default)]
    pub recovered: u32,
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
            journal: Vec::new(),
            recovered: 0,
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

    /// Hook: fold a completed parallel stage's child outcomes into workflow
    /// state (map/reduce). Runs once per stage, after every child verified, and
    /// the result is persisted with the checkpoint. Replayed children
    /// (verified in a prior run) carry an empty `output`.
    fn on_stage_completed(
        &mut self,
        _stage: &WorkflowStep,
        _children: &[StageChildOutcome],
        _state: &mut serde_json::Value,
    ) {
    }
}

// ── Engine ────────────────────────────────────────────────────────

/// Drives a [`Workflow`] through the evidence lifecycle, checkpointing after
/// every step and pausing at declarative interrupt points. With a
/// [`WorkflowStore`] attached ([`WorkflowEngine::with_store`]) the engine
/// persists write-ahead journal entries before every dispatch and the terminal
/// outcome after, so a crash mid-run resumes with observable recovery.
/// Without a store the engine behaves exactly as before (the caller persists
/// the returned checkpoints).
pub struct WorkflowEngine {
    goal_engine: GoalEngine,
    store: Option<Arc<dyn WorkflowStore>>,
}

impl WorkflowEngine {
    pub fn new(goal_engine: GoalEngine) -> Self {
        Self {
            goal_engine,
            store: None,
        }
    }

    /// Attach a [`WorkflowStore`]: the engine then persists write-ahead
    /// journal entries before every dispatch and terminal outcomes after,
    /// plus the checkpoint at every pause/completion/failure return. Save
    /// failures mid-run fail the workflow loudly — a run must never claim
    /// progress it could not make durable.
    pub fn with_store(mut self, store: Arc<dyn WorkflowStore>) -> Self {
        self.store = Some(store);
        self
    }

    fn now_ms() -> u64 {
        chrono::Utc::now().timestamp_millis() as u64
    }

    /// Persist the checkpoint when a store is attached (no-op otherwise).
    async fn persist(&self, cp: &WorkflowCheckpoint) -> Result<(), WorkflowError> {
        match &self.store {
            Some(store) => store.save(cp).await,
            None => Ok(()),
        }
    }

    /// A mid-run durability write failed — fail the workflow loudly; the
    /// checkpoint is still handed over so nothing is lost in memory.
    fn save_failure(
        &self,
        cp: WorkflowCheckpoint,
        step_index: usize,
        e: WorkflowError,
    ) -> WorkflowProgress {
        WorkflowProgress::Failed {
            checkpoint: cp,
            step_index,
            message: format!("durability write failed: {e}"),
        }
    }

    /// Return helpers that persist before handing control back. A save error
    /// here is not fatal — the checkpoint travels with the progress and the
    /// caller's own save is the authoritative durable copy.
    async fn done_paused(&self, cp: WorkflowCheckpoint, reason: PauseReason) -> WorkflowProgress {
        let _ = self.persist(&cp).await;
        WorkflowProgress::Paused { checkpoint: cp, reason }
    }

    async fn done_failed(
        &self,
        cp: WorkflowCheckpoint,
        step_index: usize,
        message: String,
    ) -> WorkflowProgress {
        let _ = self.persist(&cp).await;
        WorkflowProgress::Failed {
            checkpoint: cp,
            step_index,
            message,
        }
    }

    async fn done_completed(&self, cp: WorkflowCheckpoint) -> WorkflowProgress {
        let _ = self.persist(&cp).await;
        WorkflowProgress::Completed { checkpoint: cp }
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
            return self
                .done_failed(
                    cp,
                    cursor,
                    format!(
                        "stale checkpoint: cursor at step {cursor} but the workflow has {len} step(s)"
                    ),
                )
                .await;
        }

        while let Some(step) = workflow.steps().get(cp.next_step).cloned() {
            let index = cp.next_step;
            let label = step.label.clone();

            // Idempotent replay: a step that already verified in a prior run
            // (its Verified journal entry + evidence are durable) is never
            // re-dispatched — the cursor simply advances. This is the payoff
            // of the write-ahead journal.
            let already_verified = cp.journal.iter().any(|e| {
                e.step_index == index
                    && e.sub_index.is_none()
                    && e.state == JournalState::Verified
            });
            if already_verified {
                cp.next_step += 1;
                if step.checkpoint_after {
                    return self
                        .done_paused(
                            cp,
                            PauseReason::CheckpointBoundary {
                                step_index: index,
                                label,
                            },
                        )
                        .await;
                }
                continue;
            }

            // A parallel stage occupies one cursor position but fans out into
            // children (bounded concurrency, per-child journal + evidence).
            if let Some(stage) = step.stage.clone() {
                match self
                    .run_stage(workflow, &mut cp, index, &label, &stage)
                    .await
                {
                    Ok(children) => {
                        workflow.on_stage_completed(&step, &children, &mut cp.state);
                        cp.next_step += 1;
                        cp.updated_at_ms = Self::now_ms();
                        // Eagerly persist the fold + cursor advance together:
                        // a fully-verified stage is never re-entered on resume
                        // (its children are all Verified), so the fold must be
                        // durable atomically with the cursor — exactly-once,
                        // never applied twice by a replay.
                        if let Err(e) = self.persist(&cp).await {
                            return self.save_failure(cp, index, e);
                        }
                        if step.checkpoint_after {
                            return self
                                .done_paused(
                                    cp,
                                    PauseReason::CheckpointBoundary {
                                        step_index: index,
                                        label,
                                    },
                                )
                                .await;
                        }
                    }
                    Err(message) => return self.done_failed(cp, index, message).await,
                }
                continue;
            }

            // A step whose attempt budget was consumed in a prior run must not
            // execute again — retries are counted in the checkpoint across
            // resumes, not just within one run.
            if cp.attempts_for(index) >= step.max_attempts {
                return self
                    .done_failed(
                        cp,
                        index,
                        format!(
                            "step '{}' exhausted its {} attempt(s) in a prior run",
                            label, step.max_attempts
                        ),
                    )
                    .await;
            }

            // Interrupt 1: approval gate — pause *before* dispatch. The action
            // is Prepared but never observed until an authority approves.
            if step.requires_approval && !cp.is_approved(index) {
                return self
                    .done_paused(
                        cp,
                        PauseReason::ApprovalRequired {
                            step_index: index,
                            label,
                        },
                    )
                    .await;
            }

            // Mid-agent recovery: an InFlight entry saved by a prior run means
            // that dispatch's outcome was lost to a crash — recover it
            // explicitly (once per step) instead of re-running silently. The
            // attempt it consumed stays consumed: an outcome we cannot
            // reproduce must not let a side-effecting step exceed its budget.
            if cp
                .journal
                .iter()
                .any(|e| {
                    e.step_index == index
                        && e.sub_index.is_none()
                        && e.state == JournalState::InFlight
                })
            {
                cp.recovered += 1;
            }

            // Execute through the evidence lifecycle, retrying within bounds.
            // Every attempt is journaled write-ahead (InFlight before dispatch,
            // terminal after) and appended to the evidence trail — a failed
            // attempt is an honest observation, never silently overwritten by
            // the eventual success.
            let goal = Goal::new(&step.label);
            let ctx = ExecutionContext::for_goal(&goal.id);
            let report = loop {
                let attempt = cp.attempts_for(index) + 1;
                let key = journal_key(
                    workflow.workflow_id(),
                    index,
                    None,
                    attempt,
                    &step.action.instruction,
                );
                cp.attempts.insert(index, attempt);
                cp.journal.push(JournalEntry::in_flight(
                    key.clone(),
                    index,
                    None,
                    &label,
                    attempt,
                ));
                if let Err(e) = self.persist(&cp).await {
                    return self.save_failure(cp, index, e);
                }
                let report = self
                    .goal_engine
                    .execute(&goal, &step.action, &step.expectation, &ctx)
                    .await;
                let state = if report.execution_state == ExecutionState::Verified {
                    JournalState::Verified
                } else {
                    JournalState::Failed
                };
                // Flip the write-ahead entry to its terminal state (by key — a
                // prior run's orphaned InFlight for an earlier attempt must
                // never be touched).
                if let Some(entry) = cp.journal.iter_mut().find(|e| e.key == key) {
                    entry.state = state;
                    entry.duration_ms = report.trace.duration_ms();
                }
                cp.evidence
                    .push(StepEvidence::from_report(index, None, &label, attempt, &report));
                cp.updated_at_ms = Self::now_ms();
                // Fold workflow-owned state and advance the cursor BEFORE the
                // terminal persist: the Verified journal entry, the fold, and
                // the cursor advance land in the store as ONE atomic save. A
                // durable Verified entry therefore always carries its fold — a
                // crash can never leave a Verified entry whose state fold the
                // idempotent-replay path would silently skip.
                if report.execution_state == ExecutionState::Verified {
                    workflow.on_step_completed(&step, &report, &mut cp.state);
                    cp.next_step += 1;
                }
                if let Err(e) = self.persist(&cp).await {
                    return self.save_failure(cp, index, e);
                }
                if report.execution_state == ExecutionState::Verified
                    || attempt >= step.max_attempts
                {
                    break report;
                }
            };
            let attempts = cp.attempts_for(index);

            if report.execution_state == ExecutionState::Verified {
                // Interrupt 2: durable checkpoint boundary after verification
                // (the fold + cursor advance were already persisted above).
                if step.checkpoint_after {
                    return self
                        .done_paused(
                            cp,
                            PauseReason::CheckpointBoundary {
                                step_index: index,
                                label,
                            },
                        )
                        .await;
                }
            } else {
                // Evidence says Failed and retries are exhausted — the
                // workflow is stuck exactly here.
                return self
                    .done_failed(
                        cp,
                        index,
                        format!(
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
                    )
                    .await;
            }
        }

        self.done_completed(cp).await
    }

    /// Run one parallel stage: bounded-concurrency fan-out where every child
    /// journals and verifies through the evidence lifecycle independently, then
    /// the verified outputs are folded by `on_stage_completed`. Write-ahead
    /// persistence is per-wave: the wave's InFlight entries are saved before
    /// the children dispatch and the terminal entries after, so a crash
    /// mid-stage resumes by replaying Verified children and re-executing only
    /// the unfinished ones.
    async fn run_stage(
        &mut self,
        workflow: &mut dyn Workflow,
        cp: &mut WorkflowCheckpoint,
        index: usize,
        label: &str,
        stage: &StageBody,
    ) -> Result<Vec<StageChildOutcome>, String> {
        // Approval is a before-dispatch pause — impossible to honor inside a
        // concurrent fan-out. Reject loudly rather than silently bypassing the
        // authority.
        if let Some(child) = stage.children.iter().find(|c| c.requires_approval) {
            return Err(format!(
                "stage '{label}' child '{}' requires approval — approval-gated \
                 steps cannot be stage children",
                child.label
            ));
        }

        let wf_id = workflow.workflow_id().to_string();
        let mut outcomes: Vec<StageChildOutcome> = Vec::new();
        for wave_start in (0..stage.children.len()).step_by(stage.max_parallel) {
            let wave_end = (wave_start + stage.max_parallel).min(stage.children.len());
            let mut jobs: Vec<StageChildJob> = Vec::new();
            for sub in wave_start..wave_end {
                let child = &stage.children[sub];
                // Idempotent replay: a child that verified in a prior run is
                // never re-dispatched — its evidence is already durable.
                if cp.journal.iter().any(|e| {
                    e.step_index == index
                        && e.sub_index == Some(sub)
                        && e.state == JournalState::Verified
                }) {
                    outcomes.push(StageChildOutcome {
                        sub_index: sub,
                        label: child.label.clone(),
                        verified: true,
                        output: String::new(),
                    });
                    continue;
                }
                // Mid-agent recovery: a saved InFlight entry means this
                // child's dispatch outcome was lost to a crash — re-execute
                // explicitly.
                if cp.journal.iter().any(|e| {
                    e.step_index == index
                        && e.sub_index == Some(sub)
                        && e.state == JournalState::InFlight
                }) {
                    cp.recovered += 1;
                }
                let attempt = child_attempt_count(cp, index, sub) + 1;
                cp.journal.push(JournalEntry::in_flight(
                    journal_key(
                        &wf_id,
                        index,
                        Some(sub),
                        attempt,
                        &child.action.instruction,
                    ),
                    index,
                    Some(sub),
                    &child.label,
                    attempt,
                ));
                jobs.push(StageChildJob {
                    sub,
                    child: child.clone(),
                    start_attempt: attempt,
                });
            }
            // Write-ahead: the whole wave's dispatches are durable before any
            // child actually runs.
            if let Err(e) = self.persist(cp).await {
                return Err(format!("durability write failed: {e}"));
            }

            let futures: Vec<_> = jobs
                .iter()
                .map(|job| self.run_stage_child(&wf_id, index, job))
                .collect();
            let results = futures::future::join_all(futures).await;

            for (sub, child_label, entries, evidence, report) in results {
                for entry in entries {
                    // Flip the wave's InFlight entry (same key) to terminal;
                    // retry attempts beyond the first have no InFlight and are
                    // appended.
                    if let Some(existing) = cp.journal.iter_mut().find(|e| e.key == entry.key) {
                        existing.state = entry.state;
                        existing.duration_ms = entry.duration_ms;
                    } else {
                        cp.journal.push(entry);
                    }
                }
                for ev in evidence {
                    cp.evidence.push(ev);
                }
                cp.updated_at_ms = Self::now_ms();
                if report.execution_state != ExecutionState::Verified {
                    let attempted =
                        child_attempt_count(cp, index, sub) as usize;
                    return Err(format!(
                        "stage '{label}' child '{child_label}' failed after \
                         {attempted} attempt(s): {}",
                        report
                            .outcome
                            .detail
                            .get("execution_state")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unverified")
                    ));
                }
                outcomes.push(StageChildOutcome {
                    sub_index: sub,
                    label: child_label,
                    verified: true,
                    output: report.execution_output(),
                });
            }
            // Terminal for the whole wave: every child's outcome is durable.
            if let Err(e) = self.persist(cp).await {
                return Err(format!("durability write failed: {e}"));
            }
        }
        Ok(outcomes)
    }

    /// Execute one stage child through the evidence lifecycle with its own
    /// bounded retries, returning its journal entries, evidence, and final
    /// report. Runs concurrently within a wave; it never touches the shared
    /// checkpoint (the caller applies the results).
    async fn run_stage_child(
        &self,
        wf_id: &str,
        stage_index: usize,
        job: &StageChildJob,
    ) -> (usize, String, Vec<JournalEntry>, Vec<StepEvidence>, GoalReport) {
        let goal = Goal::new(&job.child.label);
        let ctx = ExecutionContext::for_goal(&goal.id);
        let mut attempt = job.start_attempt;
        let mut entries = Vec::new();
        let mut evidence = Vec::new();
        let report = loop {
            let key = journal_key(
                wf_id,
                stage_index,
                Some(job.sub),
                attempt,
                &job.child.action.instruction,
            );
            let report = self
                .goal_engine
                .execute(&goal, &job.child.action, &job.child.expectation, &ctx)
                .await;
            let state = if report.execution_state == ExecutionState::Verified {
                JournalState::Verified
            } else {
                JournalState::Failed
            };
            entries.push(JournalEntry::terminal(
                key,
                stage_index,
                Some(job.sub),
                &job.child.label,
                attempt,
                state,
                report.trace.duration_ms(),
            ));
            evidence.push(StepEvidence::from_report(
                stage_index,
                Some(job.sub),
                &job.child.label,
                attempt,
                &report,
            ));
            if report.execution_state == ExecutionState::Verified
                || attempt >= job.child.max_attempts
            {
                break report;
            }
            attempt += 1;
        };
        (job.sub, job.child.label.clone(), entries, evidence, report)
    }
}

/// One child of a stage wave: its position, the step to run, and the attempt
/// its write-ahead InFlight entry was recorded for.
struct StageChildJob {
    sub: usize,
    child: WorkflowStep,
    start_attempt: u32,
}

/// Attempts a stage child has consumed = journal entries recorded for it
/// (InFlight entries count as consumed — a crash-burned attempt is never
/// silently re-used).
fn child_attempt_count(cp: &WorkflowCheckpoint, stage_index: usize, sub: usize) -> u32 {
    cp.journal
        .iter()
        .filter(|e| e.step_index == stage_index && e.sub_index == Some(sub))
        .count() as u32
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
        /// Hook folds stage child counts into checkpoint state.
        stage_accumulate: bool,
    }

    impl TestWorkflow {
        fn new(id: &str, steps: Vec<WorkflowStep>) -> Self {
            Self {
                id: id.to_string(),
                steps,
                accumulate: false,
                stage_accumulate: false,
            }
        }
        fn accumulating(mut self) -> Self {
            self.accumulate = true;
            self
        }
        fn stage_accumulating(mut self) -> Self {
            self.stage_accumulate = true;
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
        fn on_stage_completed(
            &mut self,
            _stage: &WorkflowStep,
            children: &[StageChildOutcome],
            state: &mut serde_json::Value,
        ) {
            if self.stage_accumulate {
                let n = state["stage_completed"].as_u64().unwrap_or(0);
                state["stage_completed"] = serde_json::json!(n + children.len() as u64);
                state["stage_verified"] = serde_json::json!(
                    children.iter().filter(|c| c.verified).count()
                );
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

    /// Records every instruction it runs, tracks concurrent executions, and
    /// optionally sleeps so a parallel stage's bounded concurrency is
    /// observable.
    #[derive(Clone)]
    struct TrackTool {
        calls: Arc<Mutex<Vec<String>>>,
        running: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
        sleep_ms: u64,
    }

    #[async_trait]
    impl Tool for TrackTool {
        fn name(&self) -> &str {
            "track"
        }
        fn description(&self) -> &str {
            "records + tracks concurrency"
        }
        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::ExecuteCommand]
        }
        async fn execute(&self, action: &Action, _c: &ExecutionContext) -> ToolResult {
            self.calls.lock().unwrap().push(action.instruction.clone());
            let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            if self.sleep_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(self.sleep_ms)).await;
            }
            self.running.fetch_sub(1, Ordering::SeqCst);
            ToolResult::success("done")
        }
    }

    /// Reads the durable store *during* its own execution — proving the engine
    /// persisted the write-ahead InFlight journal entry before dispatching the
    /// tool (and that the terminal outcome is not yet there).
    struct StoreInspectingTool {
        store: Arc<InMemoryWorkflowStore>,
        saw_in_flight: Arc<AtomicUsize>,
        saw_terminal: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Tool for StoreInspectingTool {
        fn name(&self) -> &str {
            "store-inspect"
        }
        fn description(&self) -> &str {
            "asserts write-ahead durability from inside the run"
        }
        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::ExecuteCommand]
        }
        async fn execute(&self, action: &Action, _c: &ExecutionContext) -> ToolResult {
            // Derive this call's own step from the instruction ("run a" → 0,
            // "run b" → 1) so the probe inspects the write-ahead of the step
            // actually executing, not a sibling's already-terminal record.
            let step = action.instruction.trim().rsplit(' ').next().unwrap_or("");
            let own_index = match step {
                "a" => 0usize,
                _ => 1usize,
            };
            if let Ok(Some(cp)) = self.store.load("wf-store-probe").await {
                if cp
                    .journal
                    .iter()
                    .any(|e| e.step_index == own_index && e.state == JournalState::InFlight)
                {
                    self.saw_in_flight.fetch_add(1, Ordering::SeqCst);
                }
                if cp
                    .journal
                    .iter()
                    .any(|e| e.step_index == own_index && e.state == JournalState::Verified)
                {
                    self.saw_terminal.fetch_add(1, Ordering::SeqCst);
                }
            }
            ToolResult::success("done")
        }
    }

    /// Snapshots every checkpoint the engine persists — proving *when* things
    /// become durable. Used to assert the atomicity invariant: a durable
    /// `Verified` journal entry always co-exists with its state fold and
    /// advanced cursor (they are saved in one write), never before.
    struct RecordingStore {
        inner: InMemoryWorkflowStore,
        saves: Mutex<Vec<WorkflowCheckpoint>>,
    }

    impl RecordingStore {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                inner: InMemoryWorkflowStore::new(),
                saves: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl WorkflowStore for RecordingStore {
        async fn save(&self, checkpoint: &WorkflowCheckpoint) -> Result<(), WorkflowError> {
            self.saves.lock().unwrap().push(checkpoint.clone());
            self.inner.save(checkpoint).await
        }
        async fn load(&self, workflow_id: &str) -> Result<Option<WorkflowCheckpoint>, WorkflowError> {
            self.inner.load(workflow_id).await
        }
        async fn delete(&self, workflow_id: &str) -> Result<(), WorkflowError> {
            self.inner.delete(workflow_id).await
        }
        async fn list(&self) -> Result<Vec<String>, WorkflowError> {
            self.inner.list().await
        }
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
            sub_index: None,
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

    // ── Parallel stages (map/reduce) ─────────────────────────────

    #[tokio::test]
    async fn parallel_stage_runs_children_with_bounded_concurrency_and_folds() {
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut wf = TestWorkflow::new(
            "wf-stage",
            vec![
                ok_step("pre"),
                WorkflowStep::stage(
                    "fan",
                    vec![ok_step("c0"), ok_step("c1"), ok_step("c2"), ok_step("c3")],
                    2,
                ),
                ok_step("post"),
            ],
        )
        .accumulating()
        .stage_accumulating();
        let mut engine = engine_with(vec![Box::new(TrackTool {
            calls: calls.clone(),
            running,
            peak: peak.clone(),
            sleep_ms: 60,
        })]);

        match engine.start(&mut wf).await {
            WorkflowProgress::Completed { checkpoint } => {
                // pre + 4 children + post = 6 verified steps, one cursor each.
                assert_eq!(checkpoint.next_step, 3);
                assert_eq!(checkpoint.evidence.len(), 6);
                // Stage children carry sub_index 0..=3.
                let child_sub: Vec<Option<usize>> = checkpoint
                    .evidence
                    .iter()
                    .filter(|e| e.step_index == 1)
                    .map(|e| e.sub_index)
                    .collect();
                assert_eq!(
                    child_sub,
                    vec![Some(0), Some(1), Some(2), Some(3)],
                    "every child is evidenced individually"
                );
                assert!(checkpoint
                    .evidence
                    .iter()
                    .all(|e| e.execution_state == ExecutionState::Verified));
                // map/reduce: on_stage_completed folded all four children.
                assert_eq!(checkpoint.state["stage_completed"], serde_json::json!(4));
                assert_eq!(checkpoint.state["stage_verified"], serde_json::json!(4));
                // The fold of pre/post is the sequential hook.
                assert_eq!(checkpoint.state["completed"], serde_json::json!(2));
            }
            other => panic!("expected Completed, got {other:?}"),
        }

        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 6, "every step + child ran once: {calls:?}");
        assert_eq!(
            peak.load(Ordering::SeqCst),
            2,
            "never more than max_parallel children at once"
        );
    }

    #[tokio::test]
    async fn stage_child_failure_fails_the_workflow_at_the_stage() {
        let mut wf = TestWorkflow::new(
            "wf-stage-fail",
            vec![WorkflowStep::stage(
                "fan",
                vec![ok_step("c0"), ok_step("boom"), ok_step("c2")],
                3,
            )],
        );
        let mut engine = engine_with(vec![Box::new(SelectiveFailTool::fail("boom"))]);

        match engine.start(&mut wf).await {
            WorkflowProgress::Failed {
                checkpoint,
                step_index,
                message,
            } => {
                assert_eq!(step_index, 0, "failure is reported at the stage");
                assert!(message.contains("boom"), "message: {message}");
                // c0 verified, boom failed — c2 never ran. The honest partial
                // trail survives.
                let subs: Vec<Option<usize>> =
                    checkpoint.evidence.iter().map(|e| e.sub_index).collect();
                assert_eq!(subs, vec![Some(0), Some(1)]);
                assert_eq!(
                    checkpoint.evidence[0].execution_state,
                    ExecutionState::Verified
                );
                assert_eq!(
                    checkpoint.evidence[1].execution_state,
                    ExecutionState::Failed
                );
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stage_child_retries_recover_within_the_stage() {
        let mut wf = TestWorkflow::new(
            "wf-stage-retry",
            vec![WorkflowStep::stage(
                "fan",
                vec![ok_step("c0"), ok_step("flaky").with_max_attempts(3), ok_step("c2")],
                3,
            )],
        );
        let mut engine = engine_with(vec![Box::new(SelectiveFailTool::fail_first("flaky", 1))]);

        match engine.start(&mut wf).await {
            WorkflowProgress::Completed { checkpoint } => {
                // flaky: attempt 1 failed, attempt 2 verified — both journaled.
                let flaky: Vec<_> = checkpoint
                    .evidence
                    .iter()
                    .filter(|e| e.sub_index == Some(1))
                    .collect();
                assert_eq!(flaky.len(), 2);
                assert_eq!(flaky[0].execution_state, ExecutionState::Failed);
                assert_eq!(flaky[1].execution_state, ExecutionState::Verified);
                assert_eq!(checkpoint.evidence.len(), 4, "c0 + flaky×2 + c2");
                assert_eq!(checkpoint.next_step, 1);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn approval_gated_stage_child_is_rejected() {
        let mut wf = TestWorkflow::new(
            "wf-stage-approval",
            vec![WorkflowStep::stage(
                "fan",
                vec![ok_step("risky").with_approval()],
                1,
            )],
        );
        let mut engine = engine_with(vec![Box::new(AlwaysOkTool)]);

        match engine.start(&mut wf).await {
            WorkflowProgress::Failed { message, .. } => {
                assert!(
                    message.contains("requires approval"),
                    "approval inside a fan-out is impossible: {message}"
                );
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    // ── Idempotent replay + mid-agent recovery (the journal) ─────

    fn verified_child_evidence(sub: usize, label: &str) -> StepEvidence {
        StepEvidence {
            step_index: 0,
            sub_index: Some(sub),
            label: label.into(),
            attempt: 1,
            execution_state: ExecutionState::Verified,
            verified: true,
            tool_success: true,
            outcome_kind: "TaskCompleted".into(),
            duration_ms: 3,
        }
    }

    #[tokio::test]
    async fn stage_crash_resume_replays_verified_children_and_recovers_in_flight_ones() {
        // Simulated crash mid-stage: the write-ahead save recorded child 0 as
        // Verified (durable) and child 1 as InFlight (outcome lost).
        let wf_id = "wf-stage-crash";
        let mut cp = WorkflowCheckpoint::new(wf_id);
        cp.journal.push(JournalEntry::terminal(
            journal_key(wf_id, 0, Some(0), 1, "run c0"),
            0,
            Some(0),
            "c0",
            1,
            JournalState::Verified,
            3,
        ));
        cp.journal.push(JournalEntry::in_flight(
            journal_key(wf_id, 0, Some(1), 1, "run c1"),
            0,
            Some(1),
            "c1",
            1,
        ));
        cp.evidence.push(verified_child_evidence(0, "c0"));

        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut wf = TestWorkflow::new(
            wf_id,
            vec![WorkflowStep::stage("fan", vec![ok_step("c0"), ok_step("c1")], 2)],
        );
        let mut engine = engine_with(vec![Box::new(TrackTool {
            calls: calls.clone(),
            running: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
            sleep_ms: 0,
        })]);

        match engine.resume(&mut wf, cp).await {
            WorkflowProgress::Completed { checkpoint } => {
                // Only the unfinished child re-ran — the verified one was
                // replayed from the journal, never re-dispatched.
                let ran = calls.lock().unwrap().clone();
                assert_eq!(ran, vec!["run c1".to_string()], "replay must not re-dispatch");
                // No duplicate evidence for the replayed child.
                assert_eq!(checkpoint.evidence.len(), 2);
                assert_eq!(checkpoint.evidence[0].sub_index, Some(0));
                assert_eq!(checkpoint.evidence[1].sub_index, Some(1));
                assert_eq!(checkpoint.evidence[1].attempt, 2);
                // The recovery is observable and durable.
                assert_eq!(checkpoint.recovered, 1);
                // The crashed child's InFlight entry stays as an honest record,
                // its re-execution terminal beside it.
                let inflight = checkpoint
                    .journal
                    .iter()
                    .filter(|e| e.state == JournalState::InFlight)
                    .count();
                assert_eq!(inflight, 1, "the crashed dispatch remains recorded");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn verified_step_replay_never_redispatches() {
        let wf_id = "wf-replay";
        let mut cp = WorkflowCheckpoint::new(wf_id);
        cp.attempts.insert(0, 1);
        cp.journal.push(JournalEntry::terminal(
            journal_key(wf_id, 0, None, 1, "run a"),
            0,
            None,
            "a",
            1,
            JournalState::Verified,
            2,
        ));
        cp.evidence.push(StepEvidence {
            step_index: 0,
            sub_index: None,
            label: "a".into(),
            attempt: 1,
            execution_state: ExecutionState::Verified,
            verified: true,
            tool_success: true,
            outcome_kind: "TaskCompleted".into(),
            duration_ms: 2,
        });

        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut wf = TestWorkflow::new(wf_id, vec![ok_step("a"), ok_step("b")]);
        let mut engine = engine_with(vec![Box::new(TrackTool {
            calls: calls.clone(),
            running: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
            sleep_ms: 0,
        })]);

        match engine.resume(&mut wf, cp).await {
            WorkflowProgress::Completed { checkpoint } => {
                let ran = calls.lock().unwrap().clone();
                assert_eq!(ran, vec!["run b".to_string()], "'a' was replayed, never re-run");
                assert_eq!(checkpoint.evidence.len(), 2, "no duplicate evidence");
                assert_eq!(checkpoint.evidence[0].attempt, 1);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn in_flight_recovery_reexecutes_with_observable_counter() {
        // A crash mid-dispatch of the FIRST attempt: the write-ahead recorded
        // InFlight, the outcome was lost. Recovery re-executes as the next
        // attempt (the crashed attempt stays consumed, so the step needs a
        // retry budget of 2).
        let wf_id = "wf-recover";
        let mut cp = WorkflowCheckpoint::new(wf_id);
        cp.attempts.insert(0, 1);
        cp.journal.push(JournalEntry::in_flight(
            journal_key(wf_id, 0, None, 1, "run a"),
            0,
            None,
            "a",
            1,
        ));

        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut wf = TestWorkflow::new(wf_id, vec![ok_step("a").with_max_attempts(2)]);
        let mut engine = engine_with(vec![Box::new(TrackTool {
            calls: calls.clone(),
            running: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
            sleep_ms: 0,
        })]);

        match engine.resume(&mut wf, cp).await {
            WorkflowProgress::Completed { checkpoint } => {
                let ran = calls.lock().unwrap().clone();
                assert_eq!(ran, vec!["run a".to_string()], "recovery re-executes the step");
                assert_eq!(checkpoint.recovered, 1, "recovery is observable");
                assert_eq!(checkpoint.evidence.len(), 1);
                assert_eq!(checkpoint.evidence[0].attempt, 2);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn crashed_single_attempt_step_fails_honestly_on_resume() {
        // A step with max_attempts 1 whose first dispatch crashed (InFlight,
        // outcome unknown) has consumed its only attempt — it must fail loudly,
        // never silently re-run past its budget.
        let wf_id = "wf-recover-tight";
        let mut cp = WorkflowCheckpoint::new(wf_id);
        cp.attempts.insert(0, 1);
        cp.journal.push(JournalEntry::in_flight(
            journal_key(wf_id, 0, None, 1, "run a"),
            0,
            None,
            "a",
            1,
        ));

        let mut wf = TestWorkflow::new(wf_id, vec![ok_step("a")]);
        let mut engine = engine_with(vec![Box::new(AlwaysOkTool)]);
        match engine.resume(&mut wf, cp).await {
            WorkflowProgress::Failed { message, .. } => {
                assert!(message.contains("exhausted"), "message: {message}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    // ── Engine-owned persistence (write-ahead via the store) ──────

    #[tokio::test]
    async fn engine_with_store_persists_write_ahead_before_dispatch_and_terminal_after() {
        let store = Arc::new(InMemoryWorkflowStore::new());
        let saw_in_flight = Arc::new(AtomicUsize::new(0));
        let saw_terminal = Arc::new(AtomicUsize::new(0));
        let mut wf = TestWorkflow::new("wf-store-probe", vec![ok_step("a"), ok_step("b")]);
        let mut engine = engine_with(vec![Box::new(StoreInspectingTool {
            store: store.clone(),
            saw_in_flight: saw_in_flight.clone(),
            saw_terminal: saw_terminal.clone(),
        })])
        .with_store(store.clone());

        match engine.start(&mut wf).await {
            WorkflowProgress::Completed { checkpoint } => {
                assert_eq!(checkpoint.evidence.len(), 2);
                // While each step ran, the store already held ITS InFlight entry
                // (write-ahead) and never its terminal outcome.
                assert_eq!(saw_in_flight.load(Ordering::SeqCst), 2, "write-ahead seen per step");
                assert_eq!(saw_terminal.load(Ordering::SeqCst), 0, "outcome not yet durable");
                // The durable checkpoint reflects the terminal state.
                let saved = store.load("wf-store-probe").await.unwrap().unwrap();
                assert_eq!(saved.evidence.len(), 2);
                assert!(saved.journal.iter().all(|e| e.state != JournalState::InFlight));
                assert_eq!(
                    saved
                        .journal
                        .iter()
                        .filter(|e| e.state == JournalState::Verified)
                        .count(),
                    2
                );
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    // ── Journal keys + serialization ─────────────────────────────

    #[test]
    fn journal_key_is_stable_and_distinct() {
        let a = journal_key("wf", 0, None, 1, "run x");
        assert_eq!(a, journal_key("wf", 0, None, 1, "run x"), "stable across calls");
        assert_ne!(a, journal_key("wf", 0, None, 1, "run y"), "instruction");
        assert_ne!(a, journal_key("wf", 0, None, 2, "run x"), "attempt");
        assert_ne!(a, journal_key("wf", 1, None, 1, "run x"), "step index");
        assert_ne!(a, journal_key("wf2", 0, None, 1, "run x"), "workflow id");
        assert_ne!(a, journal_key("wf", 0, Some(0), 1, "run x"), "stage vs plain");
        assert_ne!(
            journal_key("wf", 0, Some(0), 1, "run x"),
            journal_key("wf", 0, Some(1), 1, "run x"),
            "stage child position"
        );
    }

    #[test]
    fn checkpoint_with_journal_round_trips_serde() {
        let mut cp = WorkflowCheckpoint::new("wf-journal");
        cp.recovered = 2;
        cp.journal.push(JournalEntry::in_flight("k1".into(), 0, None, "a", 1));
        cp.journal.push(JournalEntry::terminal(
            "k2".into(),
            1,
            Some(0),
            "b",
            2,
            JournalState::Verified,
            4,
        ));

        let json = serde_json::to_string(&cp).unwrap();
        let back: WorkflowCheckpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(back.recovered, 2);
        assert_eq!(back.journal.len(), 2);
        assert_eq!(back.journal[0].state, JournalState::InFlight);
        assert_eq!(back.journal[1].state, JournalState::Verified);
        assert_eq!(back.journal[1].sub_index, Some(0));
    }

    #[test]
    fn legacy_checkpoints_and_steps_without_journal_or_stage_still_deserialize() {
        // A checkpoint written before journaling existed must load with
        // defaults — never panic or drop the cursor.
        let legacy = r#"{"workflow_id":"legacy","next_step":2,"state":{},"approvals":{},"attempts":{},"evidence":[],"created_at_ms":1,"updated_at_ms":1}"#;
        let cp: WorkflowCheckpoint = serde_json::from_str(legacy).unwrap();
        assert_eq!(cp.next_step, 2);
        assert!(cp.journal.is_empty());
        assert_eq!(cp.recovered, 0);

        // A WorkflowStep written before stages existed loads as a plain step.
        let step = serde_json::to_value(ok_step("a")).unwrap();
        let mut obj = step.as_object().unwrap().clone();
        obj.remove("stage");
        let back: WorkflowStep = serde_json::from_value(serde_json::Value::Object(obj)).unwrap();
        assert!(back.stage.is_none());
        assert_eq!(back.label, "a");

        // A stage round-trips its body.
        let staged = WorkflowStep::stage("fan", vec![ok_step("c0")], 3);
        let back: WorkflowStep = serde_json::from_str(&serde_json::to_string(&staged).unwrap()).unwrap();
        let body = back.stage.expect("stage survives serialization");
        assert_eq!(body.max_parallel, 3);
        assert_eq!(body.children.len(), 1);
    }

    // ── Atomicity of Verified entry + state fold + cursor ─────────

    #[tokio::test]
    async fn durable_verified_entry_always_carries_its_fold_and_cursor() {
        // The fold-loss invariant: a durable Verified journal entry for a
        // sequential step must always co-exist with its state fold and the
        // cursor advanced past it — the engine saves them in ONE atomic write.
        // If this ever regressed (terminal persisted before the fold), a crash
        // would strand a Verified entry whose fold a resume silently skips.
        let store = RecordingStore::new();
        let mut wf = TestWorkflow::new("wf-atomic", vec![ok_step("a"), ok_step("b")])
            .accumulating();
        let mut engine = engine_with(vec![Box::new(AlwaysOkTool)]).with_store(store.clone());

        match engine.start(&mut wf).await {
            WorkflowProgress::Completed { checkpoint } => {
                assert_eq!(checkpoint.state["completed"], serde_json::json!(2));
            }
            other => panic!("expected Completed, got {other:?}"),
        }

        let saves = store.saves.lock().unwrap().clone();
        assert!(
            saves.len() >= 4,
            "write-ahead + terminal per step, got {}",
            saves.len()
        );
        for (i, cp) in saves.iter().enumerate() {
            // The durable fold count must never trail the durable Verified
            // entries: at every save, completed >= cursor means each Verified
            // step's fold was already captured in state.
            let folded = cp.state["completed"].as_u64().unwrap_or(0);
            assert!(
                folded >= cp.next_step as u64,
                "save {i}: state has {folded} folds but cursor at {} — fold trailing durable progress",
                cp.next_step
            );
            for entry in cp
                .journal
                .iter()
                .filter(|e| e.sub_index.is_none() && e.state == JournalState::Verified)
            {
                assert!(
                    cp.next_step > entry.step_index,
                    "save {i}: Verified entry at step {} but cursor at {} — fold would be lost",
                    entry.step_index,
                    cp.next_step
                );
            }
        }
    }

    #[tokio::test]
    async fn stage_fold_is_applied_exactly_once_after_crash_before_eager_persist() {
        // The stage analogue of the sequential fold-loss: a crash after the
        // last wave's terminal persist but before the eager fold persist
        // leaves ALL children Verified (durable) with the cursor still at the
        // stage. Resume must replay every child (zero dispatches) and fold
        // exactly once — never twice.
        let wf_id = "wf-stage-fold-crash";
        let mut cp = WorkflowCheckpoint::new(wf_id);
        for sub in 0..4 {
            cp.journal.push(JournalEntry::terminal(
                journal_key(wf_id, 0, Some(sub), 1, &format!("run c{sub}")),
                0,
                Some(sub),
                &format!("c{sub}"),
                1,
                JournalState::Verified,
                3,
            ));
            cp.evidence.push(verified_child_evidence(sub, &format!("c{sub}")));
        }

        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut wf = TestWorkflow::new(
            wf_id,
            vec![WorkflowStep::stage(
                "fan",
                vec![ok_step("c0"), ok_step("c1"), ok_step("c2"), ok_step("c3")],
                2,
            )],
        )
        .stage_accumulating();
        let mut engine = engine_with(vec![Box::new(TrackTool {
            calls: calls.clone(),
            running: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
            sleep_ms: 0,
        })]);

        match engine.resume(&mut wf, cp).await {
            WorkflowProgress::Completed { checkpoint } => {
                let ran = calls.lock().unwrap().clone();
                assert!(ran.is_empty(), "no child may be re-dispatched: {ran:?}");
                assert_eq!(
                    checkpoint.state["stage_completed"],
                    serde_json::json!(4),
                    "fold applied exactly once — not twice"
                );
                assert_eq!(checkpoint.state["stage_verified"], serde_json::json!(4));
                assert_eq!(checkpoint.recovered, 0, "replay is not a recovery");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn legacy_checkpoint_without_journal_resumes_and_runs() {
        // A checkpoint written before journaling existed must not only
        // deserialize — it must RUN: the cursor is the truth. Strip the
        // journal + recovered fields from a real checkpoint and resume it.
        let mut cp = WorkflowCheckpoint::new("legacy-run");
        cp.next_step = 1;
        cp.state = serde_json::json!({ "completed": 1 });
        cp.attempts.insert(0, 1);
        cp.evidence.push(StepEvidence {
            step_index: 0,
            sub_index: None,
            label: "a".into(),
            attempt: 1,
            execution_state: ExecutionState::Verified,
            verified: true,
            tool_success: true,
            outcome_kind: "TaskCompleted".into(),
            duration_ms: 1,
        });
        let mut json = serde_json::to_value(&cp).unwrap();
        if let Some(obj) = json.as_object_mut() {
            obj.remove("journal");
            obj.remove("recovered");
        }
        let legacy: WorkflowCheckpoint = serde_json::from_value(json).unwrap();
        assert!(legacy.journal.is_empty());
        assert_eq!(legacy.recovered, 0);

        // And it RUNS: resume through the cursor (step 1), never re-running 0.
        let mut wf = TestWorkflow::new("legacy-run", vec![ok_step("a"), ok_step("b")])
            .accumulating();
        let mut engine = engine_with(vec![Box::new(AlwaysOkTool)]);
        match engine.resume(&mut wf, legacy).await {
            WorkflowProgress::Completed { checkpoint } => {
                assert_eq!(checkpoint.next_step, 2);
                assert_eq!(checkpoint.state["completed"], serde_json::json!(2));
                assert_eq!(checkpoint.evidence.len(), 2, "step 0 carried over, step 1 fresh");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }
}
