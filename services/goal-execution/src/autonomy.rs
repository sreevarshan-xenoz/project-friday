//! Production wiring from the cognitive autonomy seam into the Action Kernel.
//!
//! This module deliberately lives above `friday-cognitive`: the cognitive crate
//! remains usable without the action runtime, while deployed FRIDAY instances
//! can inject the real permission/audit/timeout/retry boundary.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::intent::{DesiredState, IntentLayer};
use crate::planning::{PlanSource, Planning};
use crate::world::{FileMeta, StateTransition, VerifiedObservation, WorldModel, WorldState};
use async_trait::async_trait;
use friday_actions::{ActionRequest, AgentKernel, ExecutionContext};
use friday_agent_runtime::{
    default_gate, AgentContext, AgentDirector, AgentDriver, AgentResult, AgentRole, AgentStep,
    ResultGate,
};
use friday_cognitive::{
    ActionExecution, ActionExecutor, ActionOutcome, ActionVerifier, AutonomousTick,
    DecisionFunction, DecisionState, GlobalWorkspace, TickAction,
};
use serde_json::{json, Value};
use uuid::Uuid;

/// Dispatches cognitive decisions through the real `AgentKernel`.
///
/// The executor owns the parameter envelope because `TickAction` intentionally
/// contains only the evidence-backed action intent. Production callers should
/// construct one executor per bounded action context rather than accepting
/// arbitrary parameters from the decision text.
pub struct KernelActionExecutor {
    kernel: Arc<AgentKernel>,
    parameters: Value,
    cwd: PathBuf,
    timeout: Option<Duration>,
}

impl KernelActionExecutor {
    pub fn new(kernel: Arc<AgentKernel>, parameters: Value, cwd: PathBuf) -> Self {
        Self {
            kernel,
            parameters,
            cwd,
            timeout: None,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn kernel(&self) -> &Arc<AgentKernel> {
        &self.kernel
    }
}

#[async_trait]
impl ActionExecutor for KernelActionExecutor {
    async fn execute(&self, action: &TickAction) -> ActionExecution {
        let Some((tool, operation)) = action.action.split_once('.') else {
            return ActionExecution {
                action: action.action.clone(),
                success: false,
                detail: "action must use tool.operation form".to_string(),
                output: Value::Null,
                error: Some("malformed_action".to_string()),
            };
        };

        let mut request = ActionRequest::new(tool, operation, self.parameters.clone());
        request.timeout = self.timeout;
        request.permissions.push(action.action.clone());

        let execution_id = Uuid::new_v4();
        let trace_id = Uuid::new_v4();
        let context = ExecutionContext::builder()
            .execution_id(execution_id)
            .trace_id(trace_id)
            .cwd(self.cwd.clone())
            .build();
        let result = self.kernel.run_action(request, context).await;
        let mut output = result.output;
        if result.success {
            if let Some(observation) = interpret_world_observation(
                &action.action,
                &output,
                &self.cwd,
                execution_id,
                &action.action,
            ) {
                if let Some(object) = output.as_object_mut() {
                    object.insert(
                        "world_observation".into(),
                        serde_json::to_value(observation).unwrap(),
                    );
                }
            }
        }
        let detail = json!({
            "execution_id": execution_id,
            "output": output,
            "error": result.error,
            "metadata": result.metadata,
        })
        .to_string();

        ActionExecution {
            action: action.action.clone(),
            success: result.success,
            detail,
            output,
            error: result.error,
        }
    }
}

/// Decision adapter that makes the existing LifeOS intent layer the sole
/// source of autonomous actions for a host.
///
/// It reads the host's authoritative `WorldModel`, asks `IntentLayer` for a
/// gap-driven plan, and emits at most one evidence-backed action. A closed gap
/// returns `None`; it never falls back to a second decision-maker.
pub struct IntentDecision {
    world_model: Arc<Mutex<WorldModel>>,
    desired: DesiredState,
    intent: IntentLayer,
    planner: Planning,
    last_evidence: Mutex<Option<Uuid>>,
}

impl IntentDecision {
    pub fn new(world_model: Arc<Mutex<WorldModel>>, desired: DesiredState) -> Self {
        Self {
            world_model,
            desired,
            intent: IntentLayer::new(),
            planner: Planning::new(),
            last_evidence: Mutex::new(None),
        }
    }
}

impl DecisionFunction for IntentDecision {
    fn decide(
        &self,
        workspace: &GlobalWorkspace,
        _state: &mut DecisionState,
    ) -> Option<TickAction> {
        let evidence = workspace
            .get_latest("world.updated")
            .or_else(|| workspace.get_latest("sensor.screen"))?;
        {
            let mut last = self.last_evidence.lock().expect("intent evidence poisoned");
            if *last == Some(evidence.id) {
                return None;
            }
            *last = Some(evidence.id);
        }

        let model = self.world_model.lock().expect("world model poisoned");
        let ranked = self.intent.plan(&self.planner, &model, &self.desired)?;
        let action = ranked.plan.actions.first()?;
        if ranked.plan.source != PlanSource::GapDriven {
            return None;
        }
        Some(TickAction::new(
            action.capability.to_string(),
            format!(
                "gap-driven intent: {}; {}",
                ranked.rationale, ranked.plan.strategy
            ),
            vec![evidence.id],
            friday_cognitive::Priority::High,
        ))
    }
}

/// Executes one intent action through a bounded Director child.
///
/// The Director remains the only delegation/retry boundary. The host receives a
/// normal `ActionExecution` so the existing verifier, journal, and WorldModel
/// feedback path remain unchanged. When the run is
/// [`DirectorReport::all_verified`], the verified child's structured
/// observation is interpreted into the same `stdout` / `world_observation`
/// envelope the kernel executor produces — verifiers and the WorldModel can
/// consume both paths identically.
pub struct DirectorActionExecutor {
    director: Arc<AgentDirector>,
    role: AgentRole,
    gate: ResultGate,
    cwd: PathBuf,
}

impl DirectorActionExecutor {
    pub fn new(director: Arc<AgentDirector>) -> Self {
        Self {
            director,
            role: AgentRole::Researcher,
            gate: default_gate(),
            cwd: std::env::current_dir().unwrap_or_default(),
        }
    }

    pub fn with_role(mut self, role: AgentRole) -> Self {
        self.role = role;
        self
    }

    pub fn with_gate(mut self, gate: ResultGate) -> Self {
        self.gate = gate;
        self
    }

    /// The working directory interpreted world observations are grounded in.
    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = cwd;
        self
    }
}

#[async_trait]
impl ActionExecutor for DirectorActionExecutor {
    async fn execute(&self, action: &TickAction) -> ActionExecution {
        let child = self.director.spawn(self.role, &action.action);
        let report = self
            .director
            .verify(vec![child], Arc::clone(&self.gate))
            .await;
        let verified = report.all_verified();
        let mut output = serde_json::json!({
            "director": true,
            "verified": verified,
            "children": report.entries.len(),
            "verified_children": report.verified(),
        });
        if verified {
            if let Some(observation) = report.verified_observation() {
                if let Some(object) = output.as_object_mut() {
                    // Mirror the kernel's stdout envelope so existing
                    // verifiers consume the director path identically.
                    if let Some(stdout) = observation.get("stdout") {
                        object.insert("stdout".into(), stdout.clone());
                    }
                    if let Some(world) = interpret_world_observation(
                        &action.action,
                        observation,
                        &self.cwd,
                        Uuid::new_v4(),
                        &action.action,
                    ) {
                        object.insert(
                            "world_observation".into(),
                            serde_json::to_value(world).unwrap(),
                        );
                    }
                }
            }
        }
        ActionExecution {
            action: action.action.clone(),
            success: verified,
            detail: report.summary_lines().join("; "),
            output,
            error: if verified {
                None
            } else {
                Some("director_child_unverified".into())
            },
        }
    }
}

/// The director child that runs the *exact intent action* through the real
/// Action Kernel.
///
/// The child's goal (its `AgentSpec.goal`) is the delegated action in
/// `tool.operation` form — e.g. a Director spawn of `git.status` produces a
/// child whose sole task is running `git.status`. The kernel's raw output is
/// attached as the child's structured observation so the parent can fold a
/// verified world observation into the WorldModel.
pub struct KernelChildDriver {
    kernel: Arc<AgentKernel>,
    parameters: Value,
    cwd: PathBuf,
    timeout: Option<Duration>,
}

impl KernelChildDriver {
    pub fn new(kernel: Arc<AgentKernel>, parameters: Value, cwd: PathBuf) -> Self {
        Self {
            kernel,
            parameters,
            cwd,
            timeout: None,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

#[async_trait]
impl AgentDriver for KernelChildDriver {
    async fn run(&self, ctx: AgentContext) -> AgentResult {
        let goal = ctx.spec.goal.clone();
        let Some((tool, operation)) = goal.split_once('.') else {
            return AgentResult::failed(
                ctx.spec.id.clone(),
                format!("malformed action goal: {goal}"),
                vec![],
                "malformed_action",
            );
        };
        if let Err(reason) = ctx.check() {
            return AgentResult::aborted(
                ctx.spec.id.clone(),
                reason,
                "aborted before executing the intent action",
                vec![],
                0,
            );
        }

        let start = std::time::Instant::now();
        let mut request = ActionRequest::new(tool, operation, self.parameters.clone());
        request.timeout = self.timeout;
        request.permissions.push(goal.clone());

        let execution_id = Uuid::new_v4();
        let trace_id = Uuid::new_v4();
        let cwd = ctx.spec.cwd.clone().unwrap_or_else(|| self.cwd.clone());
        let context = ExecutionContext::builder()
            .execution_id(execution_id)
            .trace_id(trace_id)
            .cwd(cwd.clone())
            .build();
        let result = self.kernel.run_action(request, context).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        if !result.success {
            let error = result
                .error
                .clone()
                .unwrap_or_else(|| "action_failed".to_string());
            return AgentResult::failed(
                ctx.spec.id.clone(),
                format!("{goal} failed: {error}"),
                vec![AgentStep::failed(1, goal, error.clone(), duration_ms)],
                error,
            );
        }

        let mut observation = result.output;
        if let Some(world) =
            interpret_world_observation(&goal, &observation, &cwd, execution_id, &goal)
        {
            if let Some(object) = observation.as_object_mut() {
                object.insert(
                    "world_observation".into(),
                    serde_json::to_value(world).unwrap(),
                );
            }
        }
        let mut completed = AgentResult::succeeded(
            ctx.spec.id.clone(),
            format!("{goal} completed"),
            vec![AgentStep::ok(1, goal, duration_ms)],
        );
        completed.duration_ms = duration_ms;
        completed.with_observation(observation)
    }
}

/// Production host wiring for the autonomous cognitive tick.
///
/// This owns the long-lived workspace, decision loop, real Action Kernel
/// adapter, verifier, and write-ahead journal as one runtime boundary. The
/// cognitive crate stays dependency-light; deployed hosts use this type to
/// supply the concrete execution policy and durable state.
pub struct AutonomyHost {
    workspace: GlobalWorkspace,
    tick: AutonomousTick,
    executor: Arc<dyn ActionExecutor>,
    verifier: Arc<dyn ActionVerifier>,
    journal: Arc<dyn friday_cognitive::ActionJournal>,
    /// Optional sole owner of runtime world state. The tick never keeps a
    /// second world-state copy; verified observations are folded here.
    world_model: Option<Arc<Mutex<WorldModel>>>,
}

impl AutonomyHost {
    pub fn new(
        workspace: GlobalWorkspace,
        tick: AutonomousTick,
        executor: Arc<dyn ActionExecutor>,
        verifier: Arc<dyn ActionVerifier>,
        journal: Arc<dyn friday_cognitive::ActionJournal>,
    ) -> Self {
        Self {
            workspace,
            tick,
            executor,
            verifier,
            journal,
            world_model: None,
        }
    }

    /// Attach the host's single WorldModel owner for verified feedback.
    pub fn with_world_model(mut self, world_model: Arc<Mutex<WorldModel>>) -> Self {
        self.world_model = Some(world_model);
        self
    }

    pub fn world_model(&self) -> Option<Arc<Mutex<WorldModel>>> {
        self.world_model.clone()
    }

    pub fn workspace(&self) -> &GlobalWorkspace {
        &self.workspace
    }

    /// Replay the latest completed journal record into the workspace after a
    /// process restart. This restores the observable trace before a new
    /// decision is admitted, so recovery does not depend on volatile evidence
    /// UUIDs from the previous process.
    pub fn recover(&self) -> Result<bool, String> {
        let Some(record) = self
            .journal
            .records()?
            .into_iter()
            .rev()
            .find(|record| record.outcome.is_some())
        else {
            return Ok(false);
        };
        let outcome = record.outcome.expect("checked above");
        self.workspace.publish_from(
            friday_cognitive::Topic::ActionDecided,
            friday_cognitive::EngineId::Automation,
            serde_json::to_value(&outcome.action).map_err(|error| error.to_string())?,
        );
        self.workspace.publish_from(
            friday_cognitive::Topic::ToolExecuted,
            friday_cognitive::EngineId::Automation,
            serde_json::json!({
                "action": outcome.action.action,
                "success": outcome.execution.success,
                "detail": outcome.execution.detail,
                "evidence": outcome.action.evidence,
                "recovered": true,
            }),
        );
        self.workspace.publish_from(
            friday_cognitive::Topic::OutcomeObserved,
            friday_cognitive::EngineId::Automation,
            serde_json::json!({
                "action": outcome.action.action,
                "success": outcome.execution.success,
                "verified": outcome.verified,
                "evidence": outcome.action.evidence,
                "recovered": true,
            }),
        );
        if outcome.verified {
            if let Some(world_model) = &self.world_model {
                if let Some(transition) = apply_verified_action_outcome(
                    &mut world_model.lock().expect("world model poisoned"),
                    &outcome,
                ) {
                    self.workspace.publish_from(
                        friday_cognitive::Topic::Custom("world.updated".into()),
                        friday_cognitive::EngineId::WorldModel,
                        serde_json::json!({
                            "action_id": transition.action_id,
                            "goal_id": transition.goal_id,
                            "diff": transition.diff,
                            "verified": true,
                            "recovered": true,
                        }),
                    );
                }
            }
        }
        Ok(true)
    }

    /// Run one crash-safe observe → decide → execute → verify continuation,
    /// then fold an explicit verified observation into the host WorldModel.
    pub async fn tick(&self) -> Option<ActionOutcome> {
        let outcome = self
            .tick
            .execute_and_verify_durable(
                &self.workspace,
                self.executor.as_ref(),
                self.verifier.as_ref(),
                self.journal.as_ref(),
            )
            .await?;

        if outcome.verified {
            if let Some(world_model) = &self.world_model {
                if let Some(transition) = apply_verified_action_outcome(
                    &mut world_model.lock().expect("world model poisoned"),
                    &outcome,
                ) {
                    self.workspace.publish_from(
                        friday_cognitive::Topic::Custom("world.updated".into()),
                        friday_cognitive::EngineId::WorldModel,
                        serde_json::json!({
                            "action_id": transition.action_id,
                            "goal_id": transition.goal_id,
                            "diff": transition.diff,
                            "verified": true,
                        }),
                    );
                }
            }
        }
        Some(outcome)
    }
}

/// Interpret an action result into a world observation.
///
/// An explicit `world_observation` payload in the output wins (it is already a
/// `VerifiedObservation`). Otherwise the known command adapters ground an
/// observation in the command's actual output — currently only `git.status`
/// via its verified porcelain output. Unknown actions yield no observation;
/// the adapter never infers effects for mutating commands.
fn interpret_world_observation(
    action: &str,
    output: &Value,
    cwd: &std::path::Path,
    execution_id: Uuid,
    goal_id: &str,
) -> Option<VerifiedObservation> {
    match output.get("world_observation") {
        Some(value) => serde_json::from_value(value.clone()).ok(),
        None if action == "git.status" => git_status_observation(
            output["stdout"].as_str().unwrap_or_default(),
            cwd,
            execution_id,
            goal_id,
        ),
        None => None,
    }
}

/// Build a world observation from Git's verified porcelain output.
///
/// This adapter intentionally handles only `git.status`: the observation is
/// grounded in the command's actual output and does not infer effects for
/// mutating commands.
fn git_status_observation(
    stdout: &str,
    cwd: &std::path::Path,
    execution_id: Uuid,
    goal_id: &str,
) -> Option<VerifiedObservation> {
    let mut state = WorldState::new().with_working_directory(cwd.to_string_lossy());
    state.files.insert(
        ".git".into(),
        FileMeta {
            path: ".git".into(),
            exists: true,
            size_bytes: None,
            is_directory: true,
            modified_at_ms: None,
        },
    );
    for line in stdout.lines().filter(|line| line.len() >= 4) {
        let path = line[3..].trim();
        if path.is_empty() {
            continue;
        }
        state.files.insert(
            path.into(),
            FileMeta {
                path: path.into(),
                exists: true,
                size_bytes: None,
                is_directory: false,
                modified_at_ms: None,
            },
        );
    }
    Some(VerifiedObservation {
        action_id: execution_id.to_string(),
        goal_id: goal_id.to_string(),
        state,
        preconditions: vec![],
    })
}

/// Convert an action outcome's explicit observation payload into a world
/// transition. Missing, malformed, or unverified observations are refused.
pub fn apply_verified_action_outcome(
    model: &mut WorldModel,
    outcome: &ActionOutcome,
) -> Option<StateTransition> {
    if !outcome.verified {
        return None;
    }
    let value = outcome.execution.output.get("world_observation")?;
    let observation: VerifiedObservation = serde_json::from_value(value.clone()).ok()?;
    Some(model.apply_verified_observation(observation))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::causal::{CausalCondition, CausalEdge, CausalEffect};
    use crate::common::Capability;
    use crate::world::{VerifiedObservation, WorldModel, WorldState};
    use async_trait::async_trait;
    use friday_actions::GitAdapter;
    use friday_agent_runtime::{AgentDirector, AgentResult, AgentStep, FnDriver};
    use friday_cognitive::{
        ActionJournal, ActionVerifier, AutonomousTick, DecisionFunction, DecisionState, EngineId,
        GlobalWorkspace, JsonActionJournal, Priority, Topic,
    };
    use std::fs;
    use std::process::Command;

    struct GitStatusDecision;

    impl DecisionFunction for GitStatusDecision {
        fn decide(
            &self,
            workspace: &GlobalWorkspace,
            _state: &mut DecisionState,
        ) -> Option<TickAction> {
            let evidence = workspace.get_latest("sensor.screen")?;
            Some(TickAction::new(
                "git.status",
                "inspect the repository after a screen observation",
                vec![evidence.id],
                Priority::High,
            ))
        }
    }

    struct GitStatusVerifier;

    #[async_trait]
    impl ActionVerifier for GitStatusVerifier {
        async fn verify(
            &self,
            _workspace: &GlobalWorkspace,
            _action: &TickAction,
            execution: &friday_cognitive::ActionExecution,
        ) -> bool {
            execution.success
                && execution.output["stdout"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("untracked.txt")
        }
    }

    #[test]
    fn verified_observation_updates_world_but_failed_outcome_does_not() {
        let action = TickAction::new("filesystem.delete", "delete file", vec![], Priority::High);
        let observation = VerifiedObservation {
            action_id: "exec-1".into(),
            goal_id: "goal-1".into(),
            state: WorldState::new().with_variable("file_exists", serde_json::json!(false)),
            preconditions: vec![],
        };
        let output = serde_json::json!({
            "world_observation": serde_json::to_value(&observation).unwrap()
        });
        let mut model = WorldModel::new(
            WorldState::new().with_variable("file_exists", serde_json::json!(true)),
        );
        let verified = ActionOutcome {
            action: action.clone(),
            execution: ActionExecution {
                action: action.action.clone(),
                success: true,
                detail: "deleted".into(),
                output,
                error: None,
            },
            verified: true,
        };
        assert!(apply_verified_action_outcome(&mut model, &verified).is_some());
        assert_eq!(
            model.state().variables["file_exists"],
            serde_json::json!(false)
        );
        assert_eq!(model.history().len(), 1);

        let failed = ActionOutcome {
            verified: false,
            ..verified
        };
        assert!(apply_verified_action_outcome(&mut model, &failed).is_none());
        assert_eq!(
            model.history().len(),
            1,
            "failed action cannot create state"
        );
    }

    #[test]
    fn malformed_observation_is_refused_without_world_mutation() {
        let action = TickAction::new("filesystem.delete", "delete file", vec![], Priority::High);
        let outcome = ActionOutcome {
            action: action.clone(),
            execution: ActionExecution {
                action: action.action,
                success: true,
                detail: "ok".into(),
                output: serde_json::json!({ "world_observation": { "bad": true } }),
                error: None,
            },
            verified: true,
        };
        let mut model = WorldModel::new(WorldState::new());
        assert!(apply_verified_action_outcome(&mut model, &outcome).is_none());
        assert!(model.history().is_empty());
    }

    #[test]
    fn stale_runtime_projection_requires_reobservation_before_replanning() {
        let mut model = WorldModel::new(WorldState::new().with_variable("version", json!(1)));
        model.causal_mut().add_edge(CausalEdge::new(
            "advance-version",
            Capability::Custom("test.advance".into()),
            CausalCondition::Always,
            vec![CausalEffect::VariableSet {
                key: "version".into(),
                value: json!(2),
            }],
        ));

        let stale = model.simulate(&Capability::Custom("test.advance".into()));
        assert!(stale.applicable);
        // An external observation supersedes the state used by the plan.
        model.record_transition(
            "external-change",
            "reobserve",
            WorldState::new().with_variable("version", json!(3)),
            &[],
        );
        assert!(model
            .apply_simulation("stale-action", "stale-goal", &stale)
            .is_none());

        // Re-observation is authoritative; a fresh simulation is the only
        // projection allowed to proceed after the stale plan is refused.
        let fresh = model.simulate(&Capability::Custom("test.advance".into()));
        assert!(model
            .apply_simulation("fresh-action", "fresh-goal", &fresh)
            .is_some());
        assert_eq!(model.state().variables["version"], json!(2));
        assert_eq!(model.history().len(), 2);
    }

    struct FeedbackDecision;

    impl DecisionFunction for FeedbackDecision {
        fn decide(
            &self,
            workspace: &GlobalWorkspace,
            _state: &mut DecisionState,
        ) -> Option<TickAction> {
            if let Some(event) = workspace.get_latest("world.updated") {
                return Some(TickAction::new(
                    "feedback.next",
                    format!("continue after world feedback: {}", event.id),
                    vec![event.id],
                    Priority::Normal,
                ));
            }
            let evidence = workspace.get_latest("sensor.screen")?;
            Some(TickAction::new(
                "feedback.first",
                "start the deterministic feedback loop",
                vec![evidence.id],
                Priority::High,
            ))
        }
    }

    struct FeedbackExecutor;

    #[async_trait]
    impl ActionExecutor for FeedbackExecutor {
        async fn execute(&self, action: &TickAction) -> ActionExecution {
            let value = if action.action == "feedback.first" {
                1
            } else {
                2
            };
            let observation = VerifiedObservation {
                action_id: action.action.clone(),
                goal_id: "feedback-loop".into(),
                state: WorldState::new().with_variable("feedback_step", json!(value)),
                preconditions: vec![],
            };
            ActionExecution {
                action: action.action.clone(),
                success: true,
                detail: "deterministic feedback result".into(),
                output: json!({
                    "world_observation": serde_json::to_value(observation).unwrap(),
                }),
                error: None,
            }
        }
    }

    struct FeedbackVerifier;

    #[async_trait]
    impl ActionVerifier for FeedbackVerifier {
        async fn verify(
            &self,
            _workspace: &GlobalWorkspace,
            _action: &TickAction,
            execution: &ActionExecution,
        ) -> bool {
            execution.success && execution.output.get("world_observation").is_some()
        }
    }

    #[tokio::test]
    async fn two_ticks_consume_verified_world_feedback() {
        let workspace = GlobalWorkspace::new();
        workspace.publish_from(
            Topic::Screen,
            EngineId::Perception,
            json!({ "change": "content", "content_hash": "feedback" }),
        );
        let model = Arc::new(Mutex::new(WorldModel::new(WorldState::new())));
        let journal_path =
            std::env::temp_dir().join(format!("friday-feedback-{}.json", Uuid::new_v4()));
        let host = AutonomyHost::new(
            workspace,
            AutonomousTick::new(Arc::new(FeedbackDecision)),
            Arc::new(FeedbackExecutor),
            Arc::new(FeedbackVerifier),
            Arc::new(JsonActionJournal::new(&journal_path)),
        )
        .with_world_model(Arc::clone(&model));

        let first = host.tick().await.expect("first tick should execute");
        assert!(first.verified);
        assert_eq!(
            model.lock().unwrap().state().variables["feedback_step"],
            json!(1)
        );
        assert_eq!(host.workspace().history(Some("world.updated")).len(), 1);

        let second = host
            .tick()
            .await
            .expect("second tick should consume feedback");
        assert_eq!(second.action.action, "feedback.next");
        assert!(second.verified);
        assert_eq!(
            model.lock().unwrap().state().variables["feedback_step"],
            json!(2)
        );
        assert_eq!(model.lock().unwrap().history().len(), 2);
        assert_eq!(host.workspace().history(Some("outcome.observed")).len(), 2);
        assert_eq!(host.workspace().history(Some("world.updated")).len(), 2);
        let _ = fs::remove_file(journal_path);
    }

    #[tokio::test]
    async fn director_executor_delegates_intent_action_and_gates_child_result() {
        let director = Arc::new(AgentDirector::new(Arc::new(FnDriver::new(|ctx| {
            AgentResult::succeeded(
                ctx.spec.id,
                "child completed the intent action",
                vec![AgentStep::ok(1, "filesystem.create_target", 1)],
            )
        }))));
        let executor = DirectorActionExecutor::new(Arc::clone(&director));
        let action = TickAction::new(
            "filesystem.create_target",
            "gap-driven intent",
            vec![Uuid::new_v4()],
            Priority::High,
        );
        let result = executor.execute(&action).await;
        assert!(result.success);
        assert_eq!(result.output["director"], true);
        assert_eq!(result.output["children"], 1);
        assert_eq!(result.output["verified_children"], 1);
    }

    #[tokio::test]
    async fn director_executor_refuses_failed_child_result() {
        let director = Arc::new(AgentDirector::new(Arc::new(FnDriver::new(|ctx| {
            AgentResult::failed(
                ctx.spec.id,
                "child failed",
                vec![AgentStep::failed(
                    1,
                    "filesystem.create_target",
                    "denied",
                    1,
                )],
                "denied",
            )
        }))));
        let executor = DirectorActionExecutor::new(director);
        let action = TickAction::new(
            "filesystem.create_target",
            "gap-driven intent",
            vec![Uuid::new_v4()],
            Priority::High,
        );
        let result = executor.execute(&action).await;
        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("director_child_unverified"));
    }

    #[tokio::test]
    async fn director_child_runs_exact_intent_action_and_exposes_world_observation() {
        let dir = std::env::temp_dir().join(format!("friday-director-real-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let repo = dir.to_string_lossy().to_string();
        Command::new("git")
            .args(["init", "--quiet", &repo])
            .status()
            .unwrap();
        fs::write(dir.join("untracked.txt"), "director evidence\n").unwrap();

        let kernel = Arc::new(AgentKernel::new().with_max_retries(0));
        kernel.register_adapter(Box::new(GitAdapter::new())).await;

        // The exact intent action (`git.status`) is delegated to a bounded
        // director child that runs it through the real kernel.
        let director = Arc::new(AgentDirector::new(Arc::new(KernelChildDriver::new(
            Arc::clone(&kernel),
            json!({ "repo": repo }),
            dir.clone(),
        ))));
        let executor = DirectorActionExecutor::new(director).with_cwd(dir.clone());
        let action = TickAction::new(
            "git.status",
            "gap-driven intent",
            vec![Uuid::new_v4()],
            Priority::High,
        );

        let result = executor.execute(&action).await;
        assert!(result.success, "director path result: {result:?}");
        assert_eq!(result.output["director"], true);
        assert_eq!(result.output["verified_children"], 1);
        // The kernel envelope is mirrored, so kernel-shaped verifiers work.
        assert!(result.output["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("untracked.txt"));
        // And the child's structured observation became a world observation.
        let observation: VerifiedObservation =
            serde_json::from_value(result.output["world_observation"].clone()).unwrap();
        assert!(observation.state.file_exists("untracked.txt"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn intent_decision_drives_gap_action_and_stops_when_complete() {
        let model = Arc::new(Mutex::new(WorldModel::new(WorldState::new())));
        model.lock().unwrap().causal_mut().add_edge(CausalEdge::new(
            "create-target",
            Capability::Custom("filesystem.create_target".into()),
            CausalCondition::Always,
            vec![CausalEffect::FileCreated {
                path: "target.txt".into(),
            }],
        ));
        let desired = DesiredState::FileExists {
            path: "target.txt".into(),
        };
        let decision = IntentDecision::new(Arc::clone(&model), desired);
        let workspace = GlobalWorkspace::new();
        workspace.publish_from(
            Topic::Screen,
            EngineId::Perception,
            json!({ "change": "content", "content_hash": "intent" }),
        );
        let mut state = DecisionState::new();
        let first = decision
            .decide(&workspace, &mut state)
            .expect("open intent gap should produce an action");
        assert_eq!(first.action, "filesystem.create_target");
        assert!(first.reason.contains("gap-driven"));

        let sim = model
            .lock()
            .unwrap()
            .simulate(&Capability::Custom("filesystem.create_target".into()));
        model
            .lock()
            .unwrap()
            .apply_simulation("create-target", "intent-goal", &sim)
            .expect("verified transition should close target gap");
        workspace.publish_from(
            Topic::Custom("world.updated".into()),
            EngineId::WorldModel,
            json!({ "verified": true, "path": "target.txt" }),
        );
        assert!(
            decision.decide(&workspace, &mut state).is_none(),
            "completed intent must stop pursuing work"
        );
    }

    struct IntentLoopExecutor;

    #[async_trait]
    impl ActionExecutor for IntentLoopExecutor {
        async fn execute(&self, action: &TickAction) -> ActionExecution {
            let observation = VerifiedObservation {
                action_id: action.action.clone(),
                goal_id: "closed-loop".into(),
                state: WorldState::new().with_file(
                    "target.txt",
                    FileMeta {
                        path: "target.txt".into(),
                        exists: true,
                        size_bytes: None,
                        is_directory: false,
                        modified_at_ms: None,
                    },
                ),
                preconditions: vec![],
            };
            ActionExecution {
                action: action.action.clone(),
                success: true,
                detail: "deterministic permitted action".into(),
                output: json!({
                    "world_observation": serde_json::to_value(observation).unwrap(),
                }),
                error: None,
            }
        }
    }

    struct IntentLoopVerifier;

    #[async_trait]
    impl ActionVerifier for IntentLoopVerifier {
        async fn verify(
            &self,
            _workspace: &GlobalWorkspace,
            _action: &TickAction,
            execution: &ActionExecution,
        ) -> bool {
            execution.success && execution.output.get("world_observation").is_some()
        }
    }

    #[tokio::test]
    async fn closed_cognitive_loop_replans_then_stops_after_verified_completion() {
        let model = Arc::new(Mutex::new(WorldModel::new(WorldState::new())));
        model.lock().unwrap().causal_mut().add_edge(CausalEdge::new(
            "create-target",
            Capability::Custom("filesystem.create_target".into()),
            CausalCondition::Always,
            vec![CausalEffect::FileCreated {
                path: "target.txt".into(),
            }],
        ));
        let workspace = GlobalWorkspace::new();
        workspace.publish_from(
            Topic::Screen,
            EngineId::Perception,
            json!({ "change": "content", "content_hash": "closed-loop" }),
        );
        let journal_path =
            std::env::temp_dir().join(format!("friday-closed-loop-{}.json", Uuid::new_v4()));
        let host = AutonomyHost::new(
            workspace,
            AutonomousTick::new(Arc::new(IntentDecision::new(
                Arc::clone(&model),
                DesiredState::FileExists {
                    path: "target.txt".into(),
                },
            ))),
            Arc::new(IntentLoopExecutor),
            Arc::new(IntentLoopVerifier),
            Arc::new(JsonActionJournal::new(&journal_path)),
        )
        .with_world_model(Arc::clone(&model));

        let first = host.tick().await.expect("gap should produce one action");
        assert!(first.verified);
        assert_eq!(first.action.action, "filesystem.create_target");
        assert!(model.lock().unwrap().state().file_exists("target.txt"));
        assert_eq!(model.lock().unwrap().history().len(), 1);
        assert_eq!(host.workspace().history(Some("world.updated")).len(), 1);

        assert!(
            host.tick().await.is_none(),
            "completed intent must stop the autonomous loop"
        );
        assert_eq!(host.workspace().history(Some("tool.executed")).len(), 1);
        assert_eq!(
            JsonActionJournal::new(&journal_path)
                .records()
                .unwrap()
                .len(),
            1
        );
        let _ = fs::remove_file(journal_path);
    }

    #[tokio::test]
    async fn real_tick_executes_verifies_and_recovers_git_result() {
        let dir = std::env::temp_dir().join(format!("friday-autonomy-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let repo = dir.to_string_lossy().to_string();
        Command::new("git")
            .args(["init", "--quiet", &repo])
            .status()
            .unwrap();
        fs::write(dir.join("untracked.txt"), "real evidence\n").unwrap();

        let kernel = Arc::new(AgentKernel::new().with_max_retries(0));
        kernel.register_adapter(Box::new(GitAdapter::new())).await;
        let executor =
            KernelActionExecutor::new(Arc::clone(&kernel), json!({ "repo": repo }), dir.clone());
        let verifier = GitStatusVerifier;
        let workspace = GlobalWorkspace::new();
        workspace.publish_from(
            Topic::Screen,
            EngineId::Perception,
            json!({ "change": "content", "content_hash": "real" }),
        );
        let journal_path = dir.join("autonomy-journal.json");
        let journal = JsonActionJournal::new(&journal_path);
        let tick = AutonomousTick::new(Arc::new(GitStatusDecision));

        let result = tick
            .execute_and_verify_durable(&workspace, &executor, &verifier, &journal)
            .await
            .expect("real tick should execute");
        assert!(result.verified, "real kernel result: {result:?}");
        assert!(result.execution.output["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("untracked.txt"));
        assert_eq!(workspace.history(Some("tool.executed")).len(), 1);
        assert_eq!(workspace.history(Some("outcome.observed")).len(), 1);

        // A fresh tick instance sees the completed journal record and returns it
        // without dispatching Git a second time.
        let recovered_tick = AutonomousTick::new(Arc::new(GitStatusDecision));
        let recovered = recovered_tick
            .execute_and_verify_durable(&workspace, &executor, &verifier, &journal)
            .await
            .expect("completed action should be recovered");
        assert!(recovered.verified);
        assert_eq!(workspace.history(Some("tool.executed")).len(), 1);
        assert_eq!(journal.records().unwrap().len(), 1);
        let _ = fs::remove_dir_all(dir);
    }
}
