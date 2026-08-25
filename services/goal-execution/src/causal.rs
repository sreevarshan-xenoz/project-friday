//! # Causal World Model
//!
//! The knowledge graph answers *"what must exist before `git.commit` runs?"*
//! (schema layer: `git.commit Requires GitRepository`). The **causal** layer
//! answers the question the graph cannot: *"if action A runs, what does state
//! S become?"*
//!
//! ```text
//! Entity graph (grounded instances)          KnowledgeGraph (schema)
//!   repo:project-friday  ──kind──> GitRepository   git.commit Requires GitRepository
//!        │                                              │
//!        └─────────────── CausalEdge ───────────────────┘
//!                          action = git.commit
//!                          when   = EntityExists(GitRepository)
//!                          effects= VariableSet(commit_hash) ·
//!                                   EntityStateChanged(repo → committed)
//!                                   │
//!                                   ▼
//!                          CausalSimulation (S → S')
//! ```
//!
//! - **Entity graph** — the grounded instance layer: the actual repos,
//!   processes, files, sessions the model has observed.
//! - **CausalEdge** — a cause→effect rule: when `when` holds against the
//!   entity graph and `action` runs, `effects` happen.
//! - **CausalSimulation** — the counterfactual projection. `simulate` is
//!   **pure**: it never mutates the model or the world; it answers exactly
//!   "if action A runs, state S changes to S'".
//!
//! This is the simulation/counterfactual seam the constitution calls the
//! largest cognitive gap — NOT visualization (DrawDB stays deferred).

use crate::common::Capability;
use crate::world::{FileMeta, WorldState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Entity Graph ──────────────────────────────────────────────────

/// A grounded entity in the world — the instance layer the causal model
/// reasons over (`repo:project-friday`, `proc:builder`, `file:main.rs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    /// Stable identity within the model.
    pub id: String,
    /// The kind — `GitRepository`, `Process`, `File`, `Network`, ...
    pub kind: String,
    /// Typed properties (state, path, pid, ...). `state` is a conventional key.
    pub properties: HashMap<String, serde_json::Value>,
}

impl Entity {
    pub fn new(id: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            properties: HashMap::new(),
        }
    }

    pub fn with_property(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.properties.insert(key.into(), value);
        self
    }

    pub fn with_state(mut self, state: impl Into<String>) -> Self {
        self.properties
            .insert("state".into(), serde_json::json!(state.into()));
        self
    }

    /// The entity's `state` property, if set.
    pub fn state(&self) -> Option<&str> {
        self.properties.get("state").and_then(|v| v.as_str())
    }
}

/// A typed relation between two entities (instance-level edge).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityRelation {
    pub subject: String,
    pub predicate: String,
    pub object: String,
}

impl EntityRelation {
    pub fn new(
        subject: impl Into<String>,
        predicate: impl Into<String>,
        object: impl Into<String>,
    ) -> Self {
        Self {
            subject: subject.into(),
            predicate: predicate.into(),
            object: object.into(),
        }
    }
}

// ── Causal Conditions ─────────────────────────────────────────────

/// A guard evaluated against the entity graph: what must be true for a causal
/// edge to fire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CausalCondition {
    /// Any entity of this kind is present.
    EntityExists { kind: String },
    /// A specific entity is in a given state.
    EntityInState { entity_id: String, state: String },
    /// Every sub-condition must hold.
    All(Vec<CausalCondition>),
    /// At least one sub-condition must hold.
    Any(Vec<CausalCondition>),
    /// Always true.
    Always,
}

impl CausalCondition {
    /// `Ok(())` when the condition holds, `Err(reason)` with a human-readable
    /// explanation of the first unmet requirement.
    pub fn check(&self, entities: &[Entity]) -> Result<(), String> {
        match self {
            CausalCondition::EntityExists { kind } => {
                if entities.iter().any(|e| &e.kind == kind) {
                    Ok(())
                } else {
                    Err(format!("no entity of kind '{kind}' exists"))
                }
            }
            CausalCondition::EntityInState { entity_id, state } => {
                match entities.iter().find(|e| &e.id == entity_id) {
                    Some(e) if e.state() == Some(state.as_str()) => Ok(()),
                    Some(e) => Err(format!(
                        "entity '{entity_id}' is {:?}, not '{state}'",
                        e.state()
                    )),
                    None => Err(format!("entity '{entity_id}' does not exist")),
                }
            }
            CausalCondition::All(conds) => {
                for c in conds {
                    c.check(entities)?;
                }
                Ok(())
            }
            CausalCondition::Any(conds) => {
                let mut reasons = Vec::new();
                for c in conds {
                    match c.check(entities) {
                        Ok(()) => return Ok(()),
                        Err(reason) => reasons.push(reason),
                    }
                }
                Err(format!(
                    "none of {} alternative(s) held: {}",
                    reasons.len(),
                    reasons.join("; ")
                ))
            }
            CausalCondition::Always => Ok(()),
        }
    }
}

// ── Causal Effects ────────────────────────────────────────────────

/// A typed effect an action produces when its causal edge fires. Effects touch
/// both the entity graph and the world state (variables/files).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CausalEffect {
    /// A new entity appears (upsert by id).
    EntityCreated(Entity),
    /// An entity is removed.
    EntityRemoved { entity_id: String },
    /// An entity's `state` property changes.
    EntityStateChanged { entity_id: String, to: String },
    /// A property on an entity is set.
    EntityPropertySet {
        entity_id: String,
        key: String,
        value: serde_json::Value,
    },
    /// A shared variable is written.
    VariableSet {
        key: String,
        value: serde_json::Value,
    },
    /// A file appears in the world state.
    FileCreated { path: String },
    /// A free-form effect (audit/descriptions only).
    Custom(String),
}

impl CausalEffect {
    pub fn describe(&self) -> String {
        match self {
            CausalEffect::EntityCreated(e) => format!("entity created: {} ({})", e.id, e.kind),
            CausalEffect::EntityRemoved { entity_id } => format!("entity removed: {entity_id}"),
            CausalEffect::EntityStateChanged { entity_id, to } => {
                format!("entity {entity_id} → state '{to}'")
            }
            CausalEffect::EntityPropertySet { entity_id, key, .. } => {
                format!("entity {entity_id} property '{key}' set")
            }
            CausalEffect::VariableSet { key, .. } => format!("variable '{key}' set"),
            CausalEffect::FileCreated { path } => format!("file created: {path}"),
            CausalEffect::Custom(desc) => desc.clone(),
        }
    }
}

// ── Causal Edge ───────────────────────────────────────────────────

/// A cause→effect rule: when `when` holds against the entity graph and
/// `action` runs, `effects` happen. Edges are deterministic — `simulate`
/// fires the **first** edge for the action whose condition holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalEdge {
    pub id: String,
    pub action: Capability,
    pub when: CausalCondition,
    pub effects: Vec<CausalEffect>,
    /// Confidence in this cause→effect mapping, [0.0, 1.0].
    pub confidence: f32,
}

impl CausalEdge {
    pub fn new(
        id: impl Into<String>,
        action: Capability,
        when: CausalCondition,
        effects: Vec<CausalEffect>,
    ) -> Self {
        Self {
            id: id.into(),
            action,
            when,
            effects,
            confidence: 1.0,
        }
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }
}

// ── Causal Simulation ─────────────────────────────────────────────

/// The answer to *"if action A runs, state S changes to S'"*.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalSimulation {
    pub action: Capability,
    /// Whether an edge fired (its condition held) — `false` means the
    /// projection is NOT a prediction: it is the unchanged input state.
    pub applicable: bool,
    /// The id of the edge that fired, if any.
    pub fired_edge: Option<String>,
    /// Human descriptions of the effects that would apply.
    pub applied_effects: Vec<String>,
    /// Projected world state (S') — the counterfactual.
    pub projected_world: WorldState,
    /// Projected entity graph after the action.
    pub projected_entities: Vec<Entity>,
    /// The world state this simulation was computed against — the freshness
    /// anchor for [`crate::WorldModel::apply_simulation`], which refuses to
    /// commit a projection derived from stale state.
    pub source_world: WorldState,
    /// Why the simulation was inapplicable, if it was.
    pub unmet: Vec<String>,
    /// Edge confidence; 0.0 when nothing fired.
    pub confidence: f32,
}

impl CausalSimulation {
    /// An inapplicable simulation is **not** a prediction: the projection is
    /// exactly the unchanged input state (world + entities), so callers can
    /// inspect `projected_world` safely regardless of applicability.
    fn inapplicable(
        action: Capability,
        world: &WorldState,
        entities: &[Entity],
        unmet: Vec<String>,
    ) -> Self {
        Self {
            action,
            applicable: false,
            fired_edge: None,
            applied_effects: Vec::new(),
            projected_world: world.clone(),
            projected_entities: entities.to_vec(),
            source_world: world.clone(),
            unmet,
            confidence: 0.0,
        }
    }
}

// ── Causal Model ──────────────────────────────────────────────────

/// The causal layer: entity graph + relations + cause→effect edges, with pure
/// simulation. Owned by [`crate::WorldModel`] as its causal seam.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CausalModel {
    pub entities: Vec<Entity>,
    pub relations: Vec<EntityRelation>,
    pub edges: Vec<CausalEdge>,
}

impl CausalModel {
    pub fn new() -> Self {
        Self::default()
    }

    // ── Entity graph ─────────────────────────────────────────────

    pub fn add_entity(&mut self, entity: Entity) {
        if let Some(existing) = self.entities.iter_mut().find(|e| e.id == entity.id) {
            *existing = entity;
        } else {
            self.entities.push(entity);
        }
    }

    pub fn with_entity(mut self, entity: Entity) -> Self {
        self.add_entity(entity);
        self
    }

    pub fn add_relation(&mut self, relation: EntityRelation) {
        if !self.relations.contains(&relation) {
            self.relations.push(relation);
        }
    }

    pub fn entity(&self, id: &str) -> Option<&Entity> {
        self.entities.iter().find(|e| e.id == id)
    }

    pub fn entities_of_kind<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Entity> + 'a {
        self.entities.iter().filter(move |e| e.kind == kind)
    }

    // ── Causal edges ─────────────────────────────────────────────

    pub fn add_edge(&mut self, edge: CausalEdge) {
        if !self.edges.iter().any(|e| e.id == edge.id) {
            self.edges.push(edge);
        }
    }

    pub fn with_edge(mut self, edge: CausalEdge) -> Self {
        self.add_edge(edge);
        self
    }

    /// All edges declared for an action (ordered; first-match wins on
    /// simulation).
    pub fn edges_for<'a>(
        &'a self,
        action: &'a Capability,
    ) -> impl Iterator<Item = &'a CausalEdge> + 'a {
        self.edges.iter().filter(move |e| &e.action == action)
    }

    // ── Simulation (pure — never mutates) ────────────────────────

    /// Project *"if `action` runs against `world` now, what changes?"* —
    /// the counterfactual. The first edge for `action` whose condition holds
    /// fires; its effects are applied to clones of the world state and entity
    /// graph. The model and `world` are untouched.
    pub fn simulate(&self, action: &Capability, world: &WorldState) -> CausalSimulation {
        let edges: Vec<&CausalEdge> = self.edges_for(action).collect();
        if edges.is_empty() {
            return CausalSimulation::inapplicable(
                action.clone(),
                world,
                &self.entities,
                vec![format!("no causal edge modeled for {action}")],
            );
        }

        for edge in &edges {
            if edge.when.check(&self.entities).is_err() {
                // Keep looking: an alternative edge may be applicable.
                continue;
            }
            let mut projected_world = world.clone();
            let mut projected_entities = self.entities.clone();
            let mut applied = Vec::new();
            for effect in &edge.effects {
                Self::apply_effect(effect, &mut projected_world, &mut projected_entities);
                applied.push(effect.describe());
            }
            return CausalSimulation {
                action: action.clone(),
                applicable: true,
                fired_edge: Some(edge.id.clone()),
                applied_effects: applied,
                projected_world,
                projected_entities,
                source_world: world.clone(),
                unmet: Vec::new(),
                confidence: edge.confidence,
            };
        }

        // No edge for this action was applicable — explain why for EVERY
        // candidate edge (the most instructive reason surfaces).
        let unmet: Vec<String> = edges
            .iter()
            .filter_map(|e| e.when.check(&self.entities).err())
            .collect();
        CausalSimulation::inapplicable(action.clone(), world, &self.entities, unmet)
    }

    fn apply_effect(effect: &CausalEffect, world: &mut WorldState, entities: &mut Vec<Entity>) {
        match effect {
            CausalEffect::EntityCreated(entity) => {
                if let Some(existing) = entities.iter_mut().find(|e| e.id == entity.id) {
                    *existing = entity.clone();
                } else {
                    entities.push(entity.clone());
                }
            }
            CausalEffect::EntityRemoved { entity_id } => {
                entities.retain(|e| &e.id != entity_id);
            }
            CausalEffect::EntityStateChanged { entity_id, to } => {
                if let Some(e) = entities.iter_mut().find(|e| &e.id == entity_id) {
                    e.properties.insert("state".into(), serde_json::json!(to));
                }
            }
            CausalEffect::EntityPropertySet {
                entity_id,
                key,
                value,
            } => {
                if let Some(e) = entities.iter_mut().find(|e| &e.id == entity_id) {
                    e.properties.insert(key.clone(), value.clone());
                }
            }
            CausalEffect::VariableSet { key, value } => {
                world.variables.insert(key.clone(), value.clone());
            }
            CausalEffect::FileCreated { path } => {
                world.files.insert(
                    path.clone(),
                    FileMeta {
                        path: path.clone(),
                        exists: true,
                        size_bytes: None,
                        is_directory: false,
                        modified_at_ms: Some(chrono::Utc::now().timestamp_millis() as u64),
                    },
                );
            }
            CausalEffect::Custom(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> Entity {
        Entity::new("repo:project-friday", "GitRepository").with_state("clean")
    }

    fn commit_edge() -> CausalEdge {
        CausalEdge::new(
            "git.commit->commit",
            Capability::Custom("git.commit".into()),
            CausalCondition::EntityExists {
                kind: "GitRepository".into(),
            },
            vec![
                CausalEffect::VariableSet {
                    key: "commit_hash".into(),
                    value: serde_json::json!("abc123"),
                },
                CausalEffect::EntityStateChanged {
                    entity_id: "repo:project-friday".into(),
                    to: "committed".into(),
                },
            ],
        )
    }

    #[test]
    fn entity_graph_lookup_by_kind_and_id() {
        let model = CausalModel::new().with_entity(repo());
        assert_eq!(model.entities_of_kind("GitRepository").count(), 1);
        assert_eq!(
            model.entity("repo:project-friday").unwrap().kind,
            "GitRepository"
        );
        assert!(model.entity("missing").is_none());
        assert_eq!(model.entities_of_kind("Process").count(), 0);
    }

    #[test]
    fn add_entity_is_an_upsert() {
        let mut model = CausalModel::new();
        model.add_entity(repo());
        model.add_entity(Entity::new("repo:project-friday", "GitRepository").with_state("dirty"));
        assert_eq!(
            model.entities.len(),
            1,
            "same id replaces, never duplicates"
        );
        assert_eq!(
            model.entity("repo:project-friday").unwrap().state(),
            Some("dirty")
        );
    }

    #[test]
    fn condition_holds_and_reports_unmet() {
        let entities = vec![repo()];
        assert_eq!(
            CausalCondition::EntityExists {
                kind: "GitRepository".into()
            }
            .check(&entities),
            Ok(())
        );
        let unmet = CausalCondition::EntityExists {
            kind: "BrowserSession".into(),
        }
        .check(&entities)
        .unwrap_err();
        assert!(unmet.contains("BrowserSession"), "got: {unmet}");

        assert_eq!(
            CausalCondition::EntityInState {
                entity_id: "repo:project-friday".into(),
                state: "clean".into(),
            }
            .check(&entities),
            Ok(())
        );
        assert!(CausalCondition::EntityInState {
            entity_id: "repo:project-friday".into(),
            state: "committed".into(),
        }
        .check(&entities)
        .is_err());
    }

    #[test]
    fn all_and_any_compose() {
        let entities = vec![repo()];
        let both = CausalCondition::All(vec![
            CausalCondition::EntityExists {
                kind: "GitRepository".into(),
            },
            CausalCondition::EntityInState {
                entity_id: "repo:project-friday".into(),
                state: "clean".into(),
            },
        ]);
        assert_eq!(both.check(&entities), Ok(()));

        let one_of = CausalCondition::Any(vec![
            CausalCondition::EntityExists {
                kind: "Process".into(),
            },
            CausalCondition::EntityExists {
                kind: "GitRepository".into(),
            },
        ]);
        assert_eq!(one_of.check(&entities), Ok(()));

        let neither = CausalCondition::Any(vec![
            CausalCondition::EntityExists {
                kind: "Process".into(),
            },
            CausalCondition::EntityExists {
                kind: "BrowserSession".into(),
            },
        ]);
        let reason = neither.check(&entities).unwrap_err();
        assert!(reason.contains("Process"), "got: {reason}");
        assert_eq!(CausalCondition::Always.check(&entities), Ok(()));
    }

    #[test]
    fn simulate_fires_edge_and_projects_state_purely() {
        let mut model = CausalModel::new().with_entity(repo());
        model.add_edge(commit_edge());

        let world = WorldState::new().with_variable("before", serde_json::json!(1));
        let sim = model.simulate(&Capability::Custom("git.commit".into()), &world);

        assert!(sim.applicable, "unmet: {:?}", sim.unmet);
        assert_eq!(sim.fired_edge.as_deref(), Some("git.commit->commit"));
        // S → S': the projected world carries the commit hash.
        assert_eq!(
            sim.projected_world.variables["commit_hash"],
            serde_json::json!("abc123")
        );
        assert_eq!(
            sim.projected_entities
                .iter()
                .find(|e| e.id == "repo:project-friday")
                .unwrap()
                .state(),
            Some("committed")
        );
        assert_eq!(sim.applied_effects.len(), 2);

        // Counterfactual purity: neither the model nor the input world moved.
        assert_eq!(model.entities.len(), 1);
        assert_eq!(
            model.entity("repo:project-friday").unwrap().state(),
            Some("clean")
        );
        assert!(
            !world.variables.contains_key("commit_hash"),
            "input world untouched"
        );
    }

    #[test]
    fn simulate_is_inapplicable_when_condition_fails() {
        let model = CausalModel::new().with_edge(commit_edge());
        let sim = model.simulate(&Capability::Custom("git.commit".into()), &WorldState::new());
        assert!(!sim.applicable);
        assert!(sim.fired_edge.is_none());
        assert!(sim.confidence == 0.0);
        assert!(
            sim.unmet.iter().any(|u| u.contains("GitRepository")),
            "unmet: {:?}",
            sim.unmet
        );
    }

    #[test]
    fn first_applicable_edge_wins_deterministically() {
        let mut model = CausalModel::new().with_entity(repo());
        model.add_edge(CausalEdge::new(
            "git.commit->warn",
            Capability::Custom("git.commit".into()),
            CausalCondition::Always,
            vec![CausalEffect::Custom("first edge (should not fire)".into())],
        ));
        model.add_edge(commit_edge());

        let sim = model.simulate(&Capability::Custom("git.commit".into()), &WorldState::new());
        // Both conditions hold, but the FIRST edge fires.
        assert_eq!(sim.fired_edge.as_deref(), Some("git.commit->warn"));
        assert_eq!(
            sim.applied_effects,
            vec!["first edge (should not fire)".to_string()]
        );
    }

    #[test]
    fn unmodeled_action_is_inapplicable_with_reason() {
        let model = CausalModel::new().with_entity(repo());
        let sim = model.simulate(&Capability::WriteFile, &WorldState::new());
        assert!(!sim.applicable);
        assert!(
            sim.unmet.iter().any(|u| u.contains("no causal edge")),
            "unmet: {:?}",
            sim.unmet
        );
    }

    #[test]
    fn effects_apply_across_world_and_entities() {
        let mut model = CausalModel::new().with_entity(repo());
        model.add_edge(CausalEdge::new(
            "deploy",
            Capability::Custom("deploy".into()),
            CausalCondition::Always,
            vec![
                CausalEffect::FileCreated {
                    path: "/srv/app/current".into(),
                },
                CausalEffect::EntityStateChanged {
                    entity_id: "repo:project-friday".into(),
                    to: "deployed".into(),
                },
                CausalEffect::EntityPropertySet {
                    entity_id: "repo:project-friday".into(),
                    key: "replicas".into(),
                    value: serde_json::json!(3),
                },
            ],
        ));
        let sim = model.simulate(&Capability::Custom("deploy".into()), &WorldState::new());
        assert!(sim.applicable);
        assert!(sim.projected_world.file_exists("/srv/app/current"));
        let repo = sim
            .projected_entities
            .iter()
            .find(|e| e.id == "repo:project-friday")
            .unwrap();
        assert_eq!(repo.state(), Some("deployed"));
        assert_eq!(repo.properties["replicas"], serde_json::json!(3));
    }

    #[test]
    fn entity_removal_and_relations() {
        let mut model = CausalModel::new()
            .with_entity(repo())
            .with_entity(Entity::new("sess:1", "BrowserSession"));
        model.add_relation(EntityRelation::new(
            "git.commit",
            "Requires",
            "GitRepository",
        ));
        model.add_edge(CausalEdge::new(
            "close",
            Capability::Custom("browser.close".into()),
            CausalCondition::EntityExists {
                kind: "BrowserSession".into(),
            },
            vec![CausalEffect::EntityRemoved {
                entity_id: "sess:1".into(),
            }],
        ));

        let sim = model.simulate(
            &Capability::Custom("browser.close".into()),
            &WorldState::new(),
        );
        assert!(sim.applicable);
        assert!(
            sim.projected_entities.iter().all(|e| e.id != "sess:1"),
            "session removed in the projection"
        );
        assert!(
            model.entities.iter().any(|e| e.id == "sess:1"),
            "model untouched"
        );
        assert_eq!(model.relations.len(), 1);
    }

    #[test]
    fn causal_model_serde_round_trips() {
        let model = CausalModel::new()
            .with_entity(repo())
            .with_edge(commit_edge());
        let json = serde_json::to_string(&model).unwrap();
        let back: CausalModel = serde_json::from_str(&json).unwrap();
        assert_eq!(back, model);
        assert_eq!(back.edges.len(), 1);
    }
}
