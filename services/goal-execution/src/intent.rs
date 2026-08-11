//! LifeOS intent layer — current → ideal → gap → objective → plan.
//!
//! ```text
//! WorldModel (current) ─┐
//!                       ├─→ Gap (unmet conditions) → Objective → Plan
//! DesiredState (ideal) ─┘
//! ```
//!
//! The constitution's missing capability: goals exist (`goal.rs`) but there is
//! no current→ideal / gap reasoning. This module is that layer:
//!
//! - [`DesiredState`] — a declarative description of the *ideal* world
//!   (entities, entity states/properties, files, tools), evaluated against the
//!   real world state **and** the causal entity graph.
//! - [`Gap`] — `desired_state − current_state`: the unmet conditions, each
//!   with a human reason.
//! - [`Objective`] — the gap turned into something actionable: a statement the
//!   planner can consume plus the *closing actions* — actions whose causal
//!   counterfactual projection satisfies the desired state (simulation, never
//!   execution).
//! - [`IntentLayer`] — `analyze` → `objective` → `plan`. `plan` feeds a
//!   [`crate::PlanningContext`] with the objective so a plan is generated
//!   *from* a desired-state gap, not only from a one-shot goal string.
//!
//! Honesty rule: a **closed** gap yields no objective and no plan — the layer
//! never invents busywork. An **open** gap with no modeled closing action
//! still yields an objective (the planner must explore), but its confidence is
//! 0.0 and the statement says so.

use crate::causal::Entity;
use crate::planning::{
    Plan, PlanCandidate, PlanEvaluator, PlanSource, Planning, PlanningContext, RankedPlan,
};
use crate::world::{WorldModel, WorldState};
use crate::{Action, Capability, Goal};
use serde::{Deserialize, Serialize};

// ── Desired State ─────────────────────────────────────────────────

/// A declarative description of the ideal (target) world state — the "what
/// should be true" half of current→ideal. Mirrors the causal condition style:
/// a small recursive algebra (`All` / `Any`) over observable conditions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DesiredState {
    /// An entity of this kind must exist in the causal entity graph.
    EntityExists { kind: String },
    /// A specific entity must be in a given `state`.
    EntityInState { entity_id: String, state: String },
    /// A specific entity must have a property set to a value.
    EntityProperty {
        entity_id: String,
        key: String,
        value: serde_json::Value,
    },
    /// A file must exist in the world state.
    FileExists { path: String },
    /// A tool / capability must be available in the world state.
    ToolAvailable { capability: Capability },
    /// Every sub-condition must hold.
    All(Vec<DesiredState>),
    /// At least one sub-condition must hold.
    Any(Vec<DesiredState>),
}

/// A condition that is not met in the current world — one row of the gap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnmetCondition {
    /// Human description of what was desired.
    pub desired: String,
    /// Why the current state does not satisfy it.
    pub reason: String,
}

impl DesiredState {
    /// Evaluate this condition against the current world (state + entity
    /// graph). Returns the unmet conditions; empty = satisfied. Pure.
    pub fn evaluate(&self, world: &WorldState, entities: &[Entity]) -> Vec<UnmetCondition> {
        match self {
            DesiredState::EntityExists { kind } => {
                if entities.iter().any(|e| &e.kind == kind) {
                    Vec::new()
                } else {
                    vec![UnmetCondition {
                        desired: describe(self),
                        reason: format!("no entity of kind '{kind}' has been observed"),
                    }]
                }
            }
            DesiredState::EntityInState { entity_id, state } => {
                match entities.iter().find(|e| &e.id == entity_id) {
                    Some(e) if e.state() == Some(state.as_str()) => Vec::new(),
                    Some(e) => vec![UnmetCondition {
                        desired: describe(self),
                        reason: format!("entity '{entity_id}' is {:?}, not '{state}'", e.state()),
                    }],
                    None => vec![UnmetCondition {
                        desired: describe(self),
                        reason: format!("entity '{entity_id}' does not exist"),
                    }],
                }
            }
            DesiredState::EntityProperty {
                entity_id,
                key,
                value,
            } => match entities.iter().find(|e| &e.id == entity_id) {
                Some(e) if e.properties.get(key) == Some(value) => Vec::new(),
                Some(e) => vec![UnmetCondition {
                    desired: describe(self),
                    reason: format!(
                        "entity '{entity_id}' property '{key}' is {:?}, not {value}",
                        e.properties.get(key)
                    ),
                }],
                None => vec![UnmetCondition {
                    desired: describe(self),
                    reason: format!("entity '{entity_id}' does not exist"),
                }],
            },
            DesiredState::FileExists { path } => {
                if world.file_exists(path) {
                    Vec::new()
                } else {
                    vec![UnmetCondition {
                        desired: describe(self),
                        reason: format!("file '{path}' does not exist"),
                    }]
                }
            }
            DesiredState::ToolAvailable { capability } => {
                if world.has_tool(capability) {
                    Vec::new()
                } else {
                    vec![UnmetCondition {
                        desired: describe(self),
                        reason: format!("tool '{capability}' is unavailable"),
                    }]
                }
            }
            DesiredState::All(conds) => conds
                .iter()
                .flat_map(|c| c.evaluate(world, entities))
                .collect(),
            DesiredState::Any(conds) => {
                // The disjunction of no alternatives is false — an empty `Any`
                // is never satisfied, unlike the vacuously-true empty `All`.
                if conds.is_empty() {
                    return vec![UnmetCondition {
                        desired: describe(self),
                        reason: "no alternative conditions specified".to_string(),
                    }];
                }
                // Satisfied as soon as one alternative holds; otherwise report
                // every alternative's unmet reasons (the most instructive).
                let mut unmet = Vec::new();
                for c in conds {
                    let u = c.evaluate(world, entities);
                    if u.is_empty() {
                        return Vec::new();
                    }
                    unmet.extend(u);
                }
                unmet
            }
        }
    }

    /// Whether the desired state is fully satisfied by the current world.
    pub fn satisfied(&self, world: &WorldState, entities: &[Entity]) -> bool {
        self.evaluate(world, entities).is_empty()
    }

    /// Number of independently observable conditions represented by this ideal.
    /// Used only for a transparent progress estimate; satisfaction remains the
    /// authoritative completion test, especially for `Any` conditions.
    pub fn condition_count(&self) -> usize {
        match self {
            DesiredState::All(conditions) | DesiredState::Any(conditions) => conditions
                .iter()
                .map(Self::condition_count)
                .sum::<usize>()
                .max(1),
            _ => 1,
        }
    }
}

// ── Gap ───────────────────────────────────────────────────────────

/// The answer to `desired_state − current_state`: what is missing, and why.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gap {
    pub desired: DesiredState,
    /// The unmet conditions (empty = the gap is closed).
    pub unmet: Vec<UnmetCondition>,
}

impl Gap {
    pub fn is_closed(&self) -> bool {
        self.unmet.is_empty()
    }

    /// Human-readable description of the gap, e.g.
    /// `entity of kind 'GitRepository' exists — unmet: no entity of kind
    /// 'GitRepository' has been observed`.
    pub fn describe(&self) -> String {
        if self.unmet.is_empty() {
            describe(&self.desired)
        } else {
            let reasons = self
                .unmet
                .iter()
                .map(|u| u.reason.clone())
                .collect::<Vec<_>>()
                .join("; ");
            format!("{} — unmet: {reasons}", describe(&self.desired))
        }
    }

    /// Estimated completion from observable conditions. A closed gap is always
    /// exactly 1.0; an open gap reports the proportion of unmet conditions.
    pub fn progress(&self) -> f32 {
        if self.is_closed() {
            return 1.0;
        }
        let total = self.desired.condition_count() as f32;
        (1.0 - self.unmet.len() as f32 / total).clamp(0.0, 1.0)
    }
}

// ── Objective ─────────────────────────────────────────────────────

/// The gap turned into an actionable objective: a statement the planner can
/// consume, plus the actions whose causal counterfactual **closes** the gap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Objective {
    pub id: String,
    /// The goal statement — feeds `Goal` → `PlanningContext` → planner.
    pub statement: String,
    /// The ideal state this objective pursues.
    pub desired: DesiredState,
    /// Actions (causal edges) whose simulated projection satisfies `desired`.
    /// Empty when no modeled action is known to close the gap — honest: the
    /// planner must explore rather than pretend.
    pub closing_actions: Vec<Capability>,
    /// Mean confidence of the closing edges; 0.0 when none are known.
    pub confidence: f32,
}

// ── Intent Layer ──────────────────────────────────────────────────

/// The current→ideal/gap seam: `analyze` → `objective` → `plan`.
///
/// ```text
/// DesiredState + WorldModel ─→ Gap ─→ Objective ─→ RankedPlan
/// ```
///
/// `plan` feeds the objective into the existing planner, producing a
/// `PlanSource::GapDriven` candidate that competes with the planner's own
/// candidates under `PlanEvaluator` — so a plan is generated *from* a
/// desired-state gap, not only from a one-shot goal string.
pub struct IntentLayer;

impl IntentLayer {
    pub fn new() -> Self {
        Self
    }

    /// `desired_state − current_state` — the unmet conditions against the
    /// world state and the causal entity graph.
    pub fn analyze(&self, model: &WorldModel, desired: &DesiredState) -> Gap {
        let unmet = desired.evaluate(model.state(), &model.causal().entities);
        Gap {
            desired: desired.clone(),
            unmet,
        }
    }

    /// The current→ideal bridge. `None` when the gap is already closed — the
    /// layer never invents work for a satisfied ideal. Otherwise an objective
    /// whose closing actions come from the causal model: each modeled action
    /// is simulated; an action closes the gap when its **projection** (never
    /// execution) satisfies the desired state.
    /// Current progress toward an ideal, recalculated from the authoritative
    /// WorldModel snapshot.
    pub fn progress(&self, model: &WorldModel, desired: &DesiredState) -> f32 {
        self.analyze(model, desired).progress()
    }

    /// Whether the current world has reached the desired state.
    pub fn is_complete(&self, model: &WorldModel, desired: &DesiredState) -> bool {
        self.analyze(model, desired).is_closed()
    }

    pub fn objective(&self, model: &WorldModel, desired: &DesiredState) -> Option<Objective> {
        let gap = self.analyze(model, desired);
        if gap.is_closed() {
            return None;
        }
        let (closing_actions, confidence) = closing_actions(model, desired);
        let suggested = if closing_actions.is_empty() {
            "no modeled action closes this gap — the planner must explore".to_string()
        } else {
            format!(
                "suggested: {}",
                closing_actions
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let statement = format!("Close the gap: {} ({suggested})", gap.describe());
        Some(Objective {
            id: format!("obj_{}", uuid::Uuid::new_v4()),
            statement,
            desired: desired.clone(),
            closing_actions,
            confidence,
        })
    }

    /// Gap → plan: generate a real [`RankedPlan`] *from* a desired-state gap.
    /// `None` when the gap is closed (nothing to plan). The objective becomes
    /// a goal, the closing actions become `PlanSource::GapDriven` candidates,
    /// and the existing [`PlanEvaluator`] picks the winner against the
    /// planner's own candidates.
    ///
    /// When **no modeled action closes the gap**, no `GapDriven` candidate is
    /// injected: the planner's own routing plans the objective statement, and
    /// the result's `source` reflects that honestly (the planner has no
    /// causal backing to claim a closing action).
    pub fn plan(
        &self,
        planner: &Planning,
        model: &WorldModel,
        desired: &DesiredState,
    ) -> Option<RankedPlan> {
        let objective = self.objective(model, desired)?;
        let goal =
            Goal::new(&objective.statement).with_context(format!("Desired: {}", describe(desired)));
        let ctx = PlanningContext::new(goal.clone()).with_world_state(model.state().clone());

        let mut candidates = planner.think(&ctx);
        for capability in &objective.closing_actions {
            let action = Action::single(&objective.statement, capability.clone(), &goal.id);
            let mut plan = Plan::from_action(
                action,
                format!("Gap-driven via {capability}: {}", objective.statement),
            )
            .with_confidence(objective.confidence);
            plan.source = PlanSource::GapDriven;
            candidates.push(PlanCandidate {
                preliminary_score: objective.confidence,
                plan,
            });
        }

        let evaluator = PlanEvaluator::new();
        Some(evaluator.evaluate(&candidates, model.state()))
    }
}

impl Default for IntentLayer {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ───────────────────────────────────────────────────────

/// The actions whose causal simulation projects a world that satisfies the
/// desired state, plus their mean edge confidence. Pure — only simulates.
fn closing_actions(model: &WorldModel, desired: &DesiredState) -> (Vec<Capability>, f32) {
    let mut actions = Vec::new();
    let mut confidences = Vec::new();
    let mut seen = Vec::new();

    for edge in &model.causal().edges {
        if seen.contains(&edge.action) {
            continue;
        }
        seen.push(edge.action.clone());
        let sim = model.simulate(&edge.action);
        if sim.applicable && desired.satisfied(&sim.projected_world, &sim.projected_entities) {
            actions.push(edge.action.clone());
            confidences.push(sim.confidence);
        }
    }

    let confidence = if confidences.is_empty() {
        0.0
    } else {
        confidences.iter().sum::<f32>() / confidences.len() as f32
    };
    (actions, confidence)
}

/// Human-readable description of a desired condition.
fn describe(desired: &DesiredState) -> String {
    match desired {
        DesiredState::EntityExists { kind } => format!("entity of kind '{kind}' exists"),
        DesiredState::EntityInState { entity_id, state } => {
            format!("entity '{entity_id}' is in state '{state}'")
        }
        DesiredState::EntityProperty {
            entity_id,
            key,
            value,
        } => format!("entity '{entity_id}' property '{key}' = {value}"),
        DesiredState::FileExists { path } => format!("file exists: {path}"),
        DesiredState::ToolAvailable { capability } => format!("tool '{capability}' available"),
        DesiredState::All(conds) => format!(
            "all of [{}]",
            conds.iter().map(describe).collect::<Vec<_>>().join(", ")
        ),
        DesiredState::Any(conds) => format!(
            "any of [{}]",
            conds.iter().map(describe).collect::<Vec<_>>().join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::causal::{CausalCondition, CausalEdge, CausalEffect, Entity};
    use crate::world::FileMeta;
    use crate::{Capability, Goal, WorldModel};

    fn world() -> WorldModel {
        WorldModel::new(WorldState::new())
    }

    fn git_init_edge() -> CausalEdge {
        CausalEdge::new(
            "e:git.init",
            Capability::Custom("git.init".into()),
            CausalCondition::Always,
            vec![CausalEffect::EntityCreated(Entity::new(
                "repo:work",
                "GitRepository",
            ))],
        )
    }

    // ── DesiredState evaluation ──────────────────────────────────

    #[test]
    fn entity_exists_reports_unmet_with_reason() {
        let model = world();
        let desired = DesiredState::EntityExists {
            kind: "GitRepository".into(),
        };
        let unmet = desired.evaluate(model.state(), &model.causal().entities);
        assert_eq!(unmet.len(), 1);
        assert!(unmet[0].reason.contains("GitRepository"));
        assert!(!desired.satisfied(model.state(), &model.causal().entities));
    }

    #[test]
    fn entity_exists_satisfied_when_observed() {
        let mut model = world();
        model
            .causal_mut()
            .add_entity(Entity::new("repo:work", "GitRepository"));
        let desired = DesiredState::EntityExists {
            kind: "GitRepository".into(),
        };
        assert!(desired.satisfied(model.state(), &model.causal().entities));
    }

    #[test]
    fn entity_in_state_and_property_evaluate() {
        let mut model = world();
        model.causal_mut().add_entity(
            Entity::new("proc:build", "Process")
                .with_state("running")
                .with_property("pid", serde_json::json!(42)),
        );
        assert!(DesiredState::EntityInState {
            entity_id: "proc:build".into(),
            state: "running".into(),
        }
        .satisfied(model.state(), &model.causal().entities));
        assert!(!DesiredState::EntityInState {
            entity_id: "proc:build".into(),
            state: "stopped".into(),
        }
        .satisfied(model.state(), &model.causal().entities));
        assert!(DesiredState::EntityProperty {
            entity_id: "proc:build".into(),
            key: "pid".into(),
            value: serde_json::json!(42),
        }
        .satisfied(model.state(), &model.causal().entities));
        // A missing entity is unmet regardless of state.
        assert!(!DesiredState::EntityInState {
            entity_id: "absent".into(),
            state: "running".into(),
        }
        .satisfied(model.state(), &model.causal().entities));
    }

    #[test]
    fn all_and_any_aggregate_conditions() {
        let model = world();
        let both = DesiredState::All(vec![
            DesiredState::EntityExists {
                kind: "GitRepository".into(),
            },
            DesiredState::ToolAvailable {
                capability: Capability::ReadFile,
            },
        ]);
        let unmet = both.evaluate(model.state(), &model.causal().entities);
        assert_eq!(unmet.len(), 2, "All reports every unmet branch");

        let either = DesiredState::Any(vec![
            DesiredState::EntityExists {
                kind: "GitRepository".into(),
            },
            DesiredState::ToolAvailable {
                capability: Capability::ReadFile,
            },
        ]);
        assert!(
            !either.satisfied(model.state(), &model.causal().entities),
            "Any is unmet when every alternative is unmet"
        );

        let mut model = world();
        model
            .causal_mut()
            .add_entity(Entity::new("repo:x", "GitRepository"));
        assert!(either.satisfied(model.state(), &model.causal().entities));
    }

    #[test]
    fn empty_any_is_never_satisfied_but_empty_all_is() {
        // Disjunction of no alternatives is false; conjunction of none is
        // vacuously true. Locking in the degenerate-case semantics.
        let model = world();
        assert!(!DesiredState::Any(vec![]).satisfied(model.state(), &model.causal().entities));
        assert!(DesiredState::All(vec![]).satisfied(model.state(), &model.causal().entities));
    }

    #[test]
    fn file_and_tool_conditions_against_world_state() {
        let model = world();
        assert!(!DesiredState::FileExists {
            path: "/work/Cargo.toml".into(),
        }
        .satisfied(model.state(), &model.causal().entities));
        assert!(DesiredState::ToolAvailable {
            capability: Capability::ReadFile,
        }
        .satisfied(&WorldState::new().with_tool(Capability::ReadFile), &[],));
    }

    // ── Gap / Objective ──────────────────────────────────────────

    #[test]
    fn progress_recalculates_from_current_world_state() {
        let layer = IntentLayer::new();
        let desired = DesiredState::All(vec![
            DesiredState::FileExists {
                path: "a.txt".into(),
            },
            DesiredState::FileExists {
                path: "b.txt".into(),
            },
        ]);
        let mut model = world();
        assert_eq!(layer.progress(&model, &desired), 0.0);
        model.record_transition(
            "observe-a",
            "progress",
            WorldState::new().with_file(
                "a.txt",
                FileMeta {
                    path: "a.txt".into(),
                    exists: true,
                    size_bytes: None,
                    is_directory: false,
                    modified_at_ms: None,
                },
            ),
            &[],
        );
        assert_eq!(layer.progress(&model, &desired), 0.5);
        assert!(!layer.is_complete(&model, &desired));
        model.record_transition(
            "observe-b",
            "progress",
            WorldState::new()
                .with_file(
                    "a.txt",
                    FileMeta {
                        path: "a.txt".into(),
                        exists: true,
                        size_bytes: None,
                        is_directory: false,
                        modified_at_ms: None,
                    },
                )
                .with_file(
                    "b.txt",
                    FileMeta {
                        path: "b.txt".into(),
                        exists: true,
                        size_bytes: None,
                        is_directory: false,
                        modified_at_ms: None,
                    },
                ),
            &[],
        );
        assert_eq!(layer.progress(&model, &desired), 1.0);
        assert!(layer.is_complete(&model, &desired));
    }

    #[test]
    fn analyze_reports_open_gap_and_describe() {
        let layer = IntentLayer::new();
        let model = world();
        let gap = layer.analyze(
            &model,
            &DesiredState::EntityExists {
                kind: "GitRepository".into(),
            },
        );
        assert!(!gap.is_closed());
        assert_eq!(gap.unmet.len(), 1);
        assert!(gap.describe().contains("GitRepository"));
    }

    #[test]
    fn closed_gap_yields_no_objective() {
        let layer = IntentLayer::new();
        let mut model = world();
        model
            .causal_mut()
            .add_entity(Entity::new("repo:work", "GitRepository"));
        let desired = DesiredState::EntityExists {
            kind: "GitRepository".into(),
        };
        assert!(layer.analyze(&model, &desired).is_closed());
        assert!(
            layer.objective(&model, &desired).is_none(),
            "honest refusal"
        );
    }

    #[test]
    fn objective_proposes_closing_action_from_causal_simulation() {
        let layer = IntentLayer::new();
        let mut model = world();
        model.causal_mut().add_edge(git_init_edge());
        let desired = DesiredState::EntityExists {
            kind: "GitRepository".into(),
        };

        let objective = layer
            .objective(&model, &desired)
            .expect("open gap yields an objective");
        assert_eq!(
            objective.closing_actions,
            vec![Capability::Custom("git.init".into())],
            "the edge whose projection closes the gap is proposed"
        );
        assert!((objective.confidence - 1.0).abs() < 1e-6);
        assert!(objective.statement.contains("suggested: git.init"));
    }

    #[test]
    fn objective_refuses_actions_that_do_not_close_the_gap() {
        let layer = IntentLayer::new();
        let mut model = world();
        // An edge that creates a Process — irrelevant to a GitRepository gap.
        model.causal_mut().add_edge(CausalEdge::new(
            "e:spawn",
            Capability::ExecuteCommand,
            CausalCondition::Always,
            vec![CausalEffect::EntityCreated(Entity::new(
                "proc:build",
                "Process",
            ))],
        ));
        let objective = layer
            .objective(
                &model,
                &DesiredState::EntityExists {
                    kind: "GitRepository".into(),
                },
            )
            .expect("gap still open, objective exists");
        assert!(objective.closing_actions.is_empty(), "no false positives");
        assert_eq!(objective.confidence, 0.0);
        assert!(
            objective.statement.contains("planner must explore"),
            "honest about the missing action: {}",
            objective.statement
        );
    }

    // ── Plan from a gap ──────────────────────────────────────────

    #[test]
    fn plan_is_generated_from_a_desired_state_gap() {
        let layer = IntentLayer::new();
        let planner = Planning::new();
        let mut model = world();
        model.causal_mut().add_edge(git_init_edge());
        let desired = DesiredState::EntityExists {
            kind: "GitRepository".into(),
        };

        let ranked = layer
            .plan(&planner, &model, &desired)
            .expect("open gap yields a plan");
        assert_eq!(ranked.plan.source, PlanSource::GapDriven);
        assert_eq!(ranked.plan.action_count(), 1);
        assert_eq!(
            ranked.plan.actions[0].capability,
            Capability::Custom("git.init".into())
        );
    }

    #[test]
    fn no_plan_when_gap_already_closed() {
        let layer = IntentLayer::new();
        let planner = Planning::new();
        let mut model = world();
        model
            .causal_mut()
            .add_entity(Entity::new("repo:work", "GitRepository"));
        let desired = DesiredState::EntityExists {
            kind: "GitRepository".into(),
        };
        assert!(layer.plan(&planner, &model, &desired).is_none());
    }

    /// The full LifeOS loop: desired → gap → objective → apply the closing
    /// action's verified world transition → recalculate → complete.
    #[test]
    fn autonomous_intent_loop_recalculates_after_verified_world_update() {
        let layer = IntentLayer::new();
        let mut model = world();
        model.causal_mut().add_edge(git_init_edge());
        let desired = DesiredState::EntityExists {
            kind: "GitRepository".into(),
        };

        let objective = layer
            .objective(&model, &desired)
            .expect("objective for the open gap");
        let sim = model.simulate(&objective.closing_actions[0]);
        let transition = model.apply_simulation("git.init", &Goal::new("x").id, &sim);
        assert!(transition.is_some());

        let gap = layer.analyze(&model, &desired);
        assert!(gap.is_closed(), "ideal reached after the closing action");
        assert!(model.causal().entity("repo:work").is_some());
    }
}
