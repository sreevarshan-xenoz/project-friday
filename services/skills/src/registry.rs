//! `SkillRegistry` — the in-memory registry of loaded skills and the planner's
//! entry point to them (spec `specs/skill-engine.md` §5).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};

use crate::error::SkillError;
use crate::events::{LoggingSink, SkillEvent, SkillEventSink};
use crate::loader::CapabilityResolver;
use crate::manifest::SkillManifest;

/// Where a skill was discovered from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SkillSource {
    /// Shipped with FRIDAY (`skills/bundled/`).
    Builtin,
    /// User-installed (`~/.friday/skills/`).
    User,
    /// Attached to a plugin.
    Plugin,
    /// Compounded at runtime from a verified trajectory (skill compounding —
    /// the closed learning loop). Not loaded from disk; registered by
    /// [`crate::SkillCompiler`].
    Learned,
}

/// Whether a declared capability id is currently bound in the Capability
/// Registry, or soft-declared for a future capability (spec §4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityStatus {
    Resolved(String),
    SoftDeclared(String),
}

/// A loaded skill: manifest plus loader-assigned runtime state. Pure data.
#[derive(Debug, Clone)]
pub struct Skill {
    pub manifest: SkillManifest,
    pub source: SkillSource,
    pub loaded_at: DateTime<Utc>,
    pub enabled: bool,
    /// Each declared capability tagged with its resolution status.
    pub capabilities: Vec<CapabilityStatus>,
}

impl Skill {
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    pub fn version(&self) -> &str {
        &self.manifest.version
    }

    /// The skill can currently be used: enabled and with at least one
    /// resolvable capability.
    pub fn is_usable(&self) -> bool {
        self.enabled
            && self
                .capabilities
                .iter()
                .any(|c| matches!(c, CapabilityStatus::Resolved(_)))
    }

    pub fn resolved_capabilities(&self) -> Vec<&str> {
        self.capabilities
            .iter()
            .filter_map(|c| match c {
                CapabilityStatus::Resolved(id) => Some(id.as_str()),
                CapabilityStatus::SoftDeclared(_) => None,
            })
            .collect()
    }

    pub fn soft_declared_capabilities(&self) -> Vec<&str> {
        self.capabilities
            .iter()
            .filter_map(|c| match c {
                CapabilityStatus::SoftDeclared(id) => Some(id.as_str()),
                CapabilityStatus::Resolved(_) => None,
            })
            .collect()
    }
}

/// A skill plus its relevance score for a goal.
#[derive(Debug, Clone)]
pub struct RankedSkill {
    pub skill: Arc<Skill>,
    pub score: f64,
}

/// The result of a deterministic skill selection for a goal.
#[derive(Debug, Clone)]
pub struct SelectedSkill {
    /// The chosen skill.
    pub skill: Arc<Skill>,
    /// Its relevance score against the goal.
    pub score: f64,
    /// Every ranked candidate (with scores) the selection considered, in rank
    /// order — the full `Relevant → Chosen` chain for the trace.
    pub candidates: Vec<RankedSkill>,
}

/// In-memory registry of loaded skills.
pub struct SkillRegistry {
    skills: RwLock<HashMap<String, Skill>>,
    sink: Arc<dyn SkillEventSink>,
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: RwLock::new(HashMap::new()),
            sink: Arc::new(LoggingSink),
        }
    }

    pub fn with_sink(sink: Arc<dyn SkillEventSink>) -> Self {
        Self {
            skills: RwLock::new(HashMap::new()),
            sink,
        }
    }

    /// The shared event sink, also used by the loader to emit scan/error events.
    pub fn event_sink(&self) -> Arc<dyn SkillEventSink> {
        Arc::clone(&self.sink)
    }

    /// Register a skill. Fails on duplicate id. Emits `skill.loaded` (and
    /// `skill.enabled` if the skill starts enabled).
    pub fn register(&self, skill: Skill) -> Result<(), SkillError> {
        let id = skill.id().to_string();
        let version = skill.version().to_string();
        let starts_enabled = skill.enabled;
        let mut guard = self.skills.write().expect("skill registry poisoned");
        if guard.contains_key(&id) {
            return Err(SkillError::Duplicate(id));
        }
        guard.insert(id.clone(), skill);
        drop(guard);
        self.sink.emit(SkillEvent::Loaded {
            skill_id: id.clone(),
            version: version.clone(),
        });
        if starts_enabled {
            self.sink.emit(SkillEvent::Enabled { skill_id: id });
        }
        Ok(())
    }

    /// Remove a skill. Emits `skill.removed`.
    pub fn deregister(&self, skill_id: &str) -> Result<(), SkillError> {
        let mut guard = self.skills.write().expect("skill registry poisoned");
        match guard.remove(skill_id) {
            Some(_) => {
                drop(guard);
                self.sink.emit(SkillEvent::Removed {
                    skill_id: skill_id.to_string(),
                });
                Ok(())
            }
            None => Err(SkillError::NotFound(skill_id.to_string())),
        }
    }

    /// Activate a skill. Emits `skill.enabled`.
    pub fn enable(&self, skill_id: &str) -> Result<(), SkillError> {
        let mut guard = self.skills.write().expect("skill registry poisoned");
        let skill = guard
            .get_mut(skill_id)
            .ok_or_else(|| SkillError::NotFound(skill_id.to_string()))?;
        if !skill.enabled {
            skill.enabled = true;
            drop(guard);
            self.sink.emit(SkillEvent::Enabled {
                skill_id: skill_id.to_string(),
            });
        }
        Ok(())
    }

    /// Explicitly admit a runtime-learned skill (skill compounding §3.3): the
    /// deliberate admission step that flips a compiled **candidate** into an
    /// enabled, selectable skill. Only [`SkillSource::Learned`] skills can be
    /// admitted here — a bundled/user/plugin skill goes through the normal
    /// [`SkillRegistry::enable`], which is why this is a distinct,
    /// deliberately-named gate. Emits `skill.enabled`. Unknown id → `NotFound`;
    /// a non-Learned id → `NotLearned` (the candidate gate is enforced, not
    /// documented).
    pub fn enable_learned(&self, skill_id: &str) -> Result<(), SkillError> {
        let mut guard = self.skills.write().expect("skill registry poisoned");
        let skill = guard
            .get_mut(skill_id)
            .ok_or_else(|| SkillError::NotFound(skill_id.to_string()))?;
        if skill.source != SkillSource::Learned {
            return Err(SkillError::NotLearned(skill_id.to_string()));
        }
        if !skill.enabled {
            skill.enabled = true;
            drop(guard);
            self.sink.emit(SkillEvent::Enabled {
                skill_id: skill_id.to_string(),
            });
        }
        Ok(())
    }

    /// Deactivate a skill. Emits `skill.disabled`.
    pub fn disable(&self, skill_id: &str) -> Result<(), SkillError> {
        let mut guard = self.skills.write().expect("skill registry poisoned");
        let skill = guard
            .get_mut(skill_id)
            .ok_or_else(|| SkillError::NotFound(skill_id.to_string()))?;
        if skill.enabled {
            skill.enabled = false;
            drop(guard);
            self.sink.emit(SkillEvent::Disabled {
                skill_id: skill_id.to_string(),
            });
        }
        Ok(())
    }

    pub fn get(&self, skill_id: &str) -> Option<Arc<Skill>> {
        self.skills
            .read()
            .expect("skill registry poisoned")
            .get(skill_id)
            .cloned()
            .map(Arc::new)
    }

    pub fn all(&self) -> Vec<Arc<Skill>> {
        self.skills
            .read()
            .expect("skill registry poisoned")
            .values()
            .cloned()
            .map(Arc::new)
            .collect()
    }

    pub fn enabled(&self) -> Vec<Arc<Skill>> {
        self.all().into_iter().filter(|s| s.enabled).collect()
    }

    /// Unique categories across loaded skills, sorted.
    pub fn categories(&self) -> Vec<String> {
        let mut cats: Vec<String> = self
            .all()
            .into_iter()
            .map(|s| s.manifest.category.clone())
            .filter(|c| !c.is_empty())
            .collect();
        cats.sort();
        cats.dedup();
        cats
    }

    pub fn by_category(&self, category: &str) -> Vec<Arc<Skill>> {
        self.all()
            .into_iter()
            .filter(|s| s.manifest.category == category)
            .collect()
    }

    /// Skills whose natural-language actions match `action`.
    pub fn for_action(&self, action: &str) -> Vec<Arc<Skill>> {
        let action_lower = action.to_lowercase();
        self.all()
            .into_iter()
            .filter(|s| {
                s.manifest
                    .actions
                    .iter()
                    .any(|a| a.to_lowercase().contains(&action_lower))
            })
            .collect()
    }

    /// Skills that declare `capability` (in their capabilities or permissions).
    pub fn for_capability(&self, capability: &str) -> Vec<Arc<Skill>> {
        self.all()
            .into_iter()
            .filter(|s| {
                s.manifest.capabilities.iter().any(|c| c == capability)
                    || s.manifest.permissions.iter().any(|c| c == capability)
            })
            .collect()
    }

    /// Rank the *enabled* skills against a free-text goal. Higher score =
    /// more relevant. Combines lexical action/title relevance with a small
    /// resolved-capability bonus (mirrors `score_capability` in the planner).
    pub fn search(&self, goal: &str, limit: usize) -> Vec<RankedSkill> {
        self.search_filtered(goal, limit, true)
    }

    /// Rank the *disabled* skills against a goal — the explainability view
    /// for "preferred but off": what *would* have ranked had it been enabled.
    pub fn search_disabled(&self, goal: &str, limit: usize) -> Vec<RankedSkill> {
        self.search_filtered(goal, limit, false)
    }

    fn search_filtered(&self, goal: &str, limit: usize, only_enabled: bool) -> Vec<RankedSkill> {
        let goal_lower = goal.to_lowercase();
        let goal_tokens: Vec<&str> = goal_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() >= 3)
            .collect();
        let mut scored: Vec<RankedSkill> = self
            .all()
            .into_iter()
            .filter(|skill| skill.enabled == only_enabled)
            .map(|skill| {
                let score = score_skill(&goal_lower, &goal_tokens, &skill);
                RankedSkill { skill, score }
            })
            .collect();
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                // Determinism: equal scores break lexicographically by id, so
                // two hosts loading the same skills always rank identically.
                .then_with(|| a.skill.id().cmp(b.skill.id()))
        });
        scored.truncate(limit);
        scored
    }

    /// A skill may be *chosen* by [`SkillRegistry::select_for`]: enabled, at
    /// least one capability actually resolved against the registry, and every
    /// declared dependency present and enabled. Stricter than
    /// [`Skill::is_usable`] — it is the planner-facing "usable right now".
    pub fn is_selectable(&self, skill: &Skill) -> bool {
        if !skill.enabled || skill.resolved_capabilities().is_empty() {
            return false;
        }
        let guard = self.skills.read().expect("skill registry poisoned");
        skill
            .manifest
            .dependencies
            .iter()
            .all(|d| guard.get(d).is_some_and(|s| s.enabled))
    }

    /// Deterministic selection of the best *currently usable* skill for a
    /// goal: the top-ranked candidate that passes
    /// [`SkillRegistry::is_selectable`]. A disabled preferred skill, a skill
    /// whose dependency chain is broken, or a soft-declared-only skill is
    /// skipped in favor of the next usable candidate. Returns `None` when no
    /// candidate is usable.
    pub fn select_for(&self, goal: &str, limit: usize) -> Option<SelectedSkill> {
        let candidates = self.search(goal, limit);
        for ranked in &candidates {
            if self.is_selectable(&ranked.skill) {
                return Some(SelectedSkill {
                    skill: Arc::clone(&ranked.skill),
                    score: ranked.score,
                    candidates: candidates.clone(),
                });
            }
        }
        None
    }

    /// Build a scoped registry containing only `allowed` skill ids plus their
    /// dependency closure — the per-agent "allowed skills" view (Agent
    /// Runtime). An empty `allowed` returns every loaded skill. Enabled/disabled
    /// state is preserved, so a skill disabled in the source stays disabled in
    /// the subset, and a skill whose dependency is outside the scope is dropped
    /// (it would be unselectable anyway). The subset shares nothing mutable with
    /// this registry — it is a point-in-time snapshot for one worker.
    pub fn subset(&self, allowed: &[&str]) -> SkillRegistry {
        let all = self.all();
        let mut keep: Vec<String> = Vec::new();
        if allowed.is_empty() {
            keep.extend(all.iter().map(|s| s.id().to_string()));
        } else {
            let mut queue: Vec<Arc<Skill>> = all
                .iter()
                .filter(|s| allowed.contains(&s.id()))
                .cloned()
                .collect();
            let mut seen: HashSet<String> = HashSet::new();
            while let Some(skill) = queue.pop() {
                let id = skill.id().to_string();
                if !seen.insert(id.clone()) {
                    continue;
                }
                keep.push(id);
                for dep in &skill.manifest.dependencies {
                    if let Some(dep_skill) = all.iter().find(|s| s.id() == dep) {
                        if !seen.contains(dep) {
                            queue.push(Arc::clone(dep_skill));
                        }
                    }
                }
            }
        }
        let subset = SkillRegistry::new();
        for id in keep {
            if let Some(skill) = self.get(&id) {
                let _ = subset.register((*skill).clone());
            }
        }
        subset
    }

    /// Re-resolve every skill's declared capabilities against the current
    /// capability registry view (e.g. after a capability was registered or
    /// removed at runtime). Returns how many skills changed resolution
    /// status. Skills never disappear here — their [`CapabilityStatus`]es are
    /// refreshed so [`Skill::is_usable`] reflects reality instead of the
    /// load-time snapshot.
    pub fn reconcile(&self, resolver: &dyn CapabilityResolver) -> usize {
        let mut guard = self.skills.write().expect("skill registry poisoned");
        let mut changed = 0usize;
        for skill in guard.values_mut() {
            let refreshed: Vec<CapabilityStatus> = skill
                .manifest
                .capabilities
                .iter()
                .map(|cap| {
                    if resolver.exists(cap) {
                        CapabilityStatus::Resolved(cap.clone())
                    } else {
                        CapabilityStatus::SoftDeclared(cap.clone())
                    }
                })
                .collect();
            if refreshed != skill.capabilities {
                changed += 1;
            }
            skill.capabilities = refreshed;
        }
        changed
    }
}

/// Transparent relevance score in 0..1 over a single skill.
fn score_skill(goal_lower: &str, goal_tokens: &[&str], skill: &Skill) -> f64 {
    let text = format!(
        "{} {} {} {}",
        skill.manifest.id,
        skill.manifest.name,
        skill.manifest.description,
        skill.manifest.actions.join(" ")
    )
    .to_lowercase();
    let mut hits = 0u32;
    for t in goal_tokens {
        if text.contains(t) {
            hits += 1;
        }
    }
    let relevance = if goal_tokens.is_empty() {
        0.5
    } else {
        (hits as f64 / goal_tokens.len() as f64).clamp(0.0, 1.0)
    };
    let id_boost = if goal_lower.contains(&skill.manifest.id) {
        0.2
    } else {
        0.0
    };
    let resolved = skill.resolved_capabilities().len();
    let total = skill.manifest.capabilities.len().max(1);
    let capability_bonus = (resolved as f64 / total as f64) * 0.1;
    (relevance * 0.7 + id_boost + capability_bonus).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(id: &str, actions: &[&str], capabilities: &[&str]) -> Skill {
        let manifest = SkillManifest {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            description: format!("does {id} things"),
            author: "test".to_string(),
            category: "developer".to_string(),
            icon: String::new(),
            capabilities: capabilities.iter().map(|s| s.to_string()).collect(),
            actions: actions.iter().map(|s| s.to_string()).collect(),
            permissions: vec![],
            readonly: false,
            dependencies: vec![],
            platform: "Any".to_string(),
            min_core_version: "0.0.0".to_string(),
            enabled: true,
            memory: Default::default(),
        };
        Skill {
            manifest,
            source: SkillSource::Builtin,
            loaded_at: Utc::now(),
            enabled: true,
            capabilities: capabilities
                .iter()
                .map(|c| CapabilityStatus::Resolved(c.to_string()))
                .collect(),
        }
    }

    #[test]
    fn register_enable_disable_deregister() {
        let registry = SkillRegistry::new();
        let s = skill("docker", &["build docker image"], &["terminal.execute"]);
        registry.register(s).unwrap();
        assert!(registry.register(skill("docker", &["x"], &[])).is_err()); // duplicate

        let s = skill("git", &["commit"], &["git.commit"]);
        registry.register(s).unwrap();
        assert_eq!(registry.all().len(), 2);
        assert_eq!(registry.categories(), vec!["developer".to_string()]);

        registry.disable("git").unwrap();
        assert!(!registry.get("git").unwrap().enabled);
        assert_eq!(registry.enabled().len(), 1);

        registry.enable("git").unwrap();
        assert!(registry.get("git").unwrap().enabled);

        registry.deregister("git").unwrap();
        assert!(registry.get("git").is_none());
        assert!(registry.deregister("git").is_err()); // NotFound
    }

    #[test]
    fn search_ranks_relevant_skill_first() {
        let registry = SkillRegistry::new();
        registry
            .register(skill(
                "docker",
                &["build docker image", "docker compose up"],
                &["terminal.execute"],
            ))
            .unwrap();
        registry
            .register(skill(
                "minecraft",
                &["start minecraft server"],
                &["terminal.execute"],
            ))
            .unwrap();

        let ranked = registry.search("build the docker image", 5);
        assert_eq!(ranked[0].skill.id(), "docker");
        assert!(ranked[0].score > ranked[1].score);
    }

    #[test]
    fn for_action_and_for_capability() {
        let registry = SkillRegistry::new();
        registry
            .register(skill(
                "docker",
                &["docker compose up", "prune docker images"],
                &["terminal.execute", "filesystem.read"],
            ))
            .unwrap();

        assert_eq!(registry.for_action("docker compose up").len(), 1);
        assert_eq!(registry.for_action("nonexistent action").len(), 0);
        assert_eq!(registry.for_capability("filesystem.read").len(), 1);
        assert_eq!(registry.for_capability("filesystem.write").len(), 0);
    }

    fn skill_with_deps(id: &str, actions: &[&str], caps: &[&str], deps: &[&str]) -> Skill {
        let manifest = SkillManifest {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            description: format!("does {id} things"),
            author: "test".to_string(),
            category: "developer".to_string(),
            icon: String::new(),
            capabilities: caps.iter().map(|s| s.to_string()).collect(),
            actions: actions.iter().map(|s| s.to_string()).collect(),
            permissions: vec![],
            readonly: false,
            dependencies: deps.iter().map(|s| s.to_string()).collect(),
            platform: "Any".to_string(),
            min_core_version: "0.0.0".to_string(),
            enabled: true,
            memory: Default::default(),
        };
        Skill {
            manifest,
            source: SkillSource::Builtin,
            loaded_at: Utc::now(),
            enabled: true,
            capabilities: caps
                .iter()
                .map(|c| CapabilityStatus::Resolved(c.to_string()))
                .collect(),
        }
    }

    #[test]
    fn select_for_chooses_top_usable_skill() {
        let registry = SkillRegistry::new();
        registry
            .register(skill("server", &["start server"], &["terminal.execute"]))
            .unwrap();
        registry
            .register(skill(
                "minecraft",
                &["start minecraft server"],
                &["terminal.execute"],
            ))
            .unwrap();

        let selected = registry.select_for("start my minecraft server", 5).unwrap();
        assert_eq!(selected.skill.id(), "minecraft");
        assert!(selected.score > 0.0);
        assert_eq!(selected.candidates.len(), 2);
    }

    #[test]
    fn select_for_skips_disabled_preferred_skill() {
        let registry = SkillRegistry::new();
        registry
            .register(skill(
                "minecraft",
                &["start minecraft server"],
                &["terminal.execute"],
            ))
            .unwrap();
        registry
            .register(skill("server", &["start server"], &["terminal.execute"]))
            .unwrap();
        registry.disable("minecraft").unwrap();

        let selected = registry.select_for("start my minecraft server", 5).unwrap();
        assert_eq!(selected.skill.id(), "server");
    }

    #[test]
    fn select_for_skips_skill_with_broken_dependency_chain() {
        let registry = SkillRegistry::new();
        registry
            .register(skill_with_deps(
                "minecraft",
                &["start minecraft server"],
                &["terminal.execute"],
                &["missing-dep"],
            ))
            .unwrap();
        registry
            .register(skill("server", &["start server"], &["terminal.execute"]))
            .unwrap();

        // minecraft ranks first but its dependency is missing → server chosen.
        let selected = registry.select_for("start my minecraft server", 5).unwrap();
        assert_eq!(selected.skill.id(), "server");
    }

    #[test]
    fn select_for_returns_none_when_nothing_usable() {
        let registry = SkillRegistry::new();
        registry
            .register(skill_with_deps(
                "minecraft",
                &["start minecraft server"],
                &["terminal.execute"],
                &["missing-dep"],
            ))
            .unwrap();
        assert!(registry
            .select_for("start my minecraft server", 5)
            .is_none());
    }

    #[test]
    fn select_for_ties_break_deterministically_by_id() {
        // Identical action text, ids absent from the goal → identical scores;
        // the tie must break lexicographically so selection is reproducible.
        let registry = SkillRegistry::new();
        registry
            .register(skill("beta", &["do the thing"], &["terminal.execute"]))
            .unwrap();
        registry
            .register(skill("alpha", &["do the thing"], &["terminal.execute"]))
            .unwrap();

        let selected = registry.select_for("do the thing please", 5).unwrap();
        assert_eq!(selected.skill.id(), "alpha");
    }

    #[test]
    fn subset_with_empty_allowed_returns_everything() {
        let registry = SkillRegistry::new();
        registry
            .register(skill("git", &["commit"], &["git.commit"]))
            .unwrap();
        registry
            .register(skill("docker", &["build image"], &["terminal.execute"]))
            .unwrap();

        let subset = registry.subset(&[]);
        let all = subset.all();
        let ids: Vec<&str> = all.iter().map(|s| s.id()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"git") && ids.contains(&"docker"));
    }

    #[test]
    fn subset_filters_to_allowed_skills() {
        let registry = SkillRegistry::new();
        registry
            .register(skill("git", &["commit"], &["git.commit"]))
            .unwrap();
        registry
            .register(skill("docker", &["build image"], &["terminal.execute"]))
            .unwrap();
        registry
            .register(skill("web", &["search the web"], &["web.search"]))
            .unwrap();

        let subset = registry.subset(&["git", "web"]);
        let all = subset.all();
        let ids: Vec<&str> = all.iter().map(|s| s.id()).collect();
        assert!(ids.contains(&"git") && ids.contains(&"web"));
        assert!(!ids.contains(&"docker"), "ids: {ids:?}");
        // Unknown ids are silently ignored.
        assert_eq!(registry.subset(&["nope"]).all().len(), 0);
    }

    #[test]
    fn subset_includes_dependency_closure() {
        let registry = SkillRegistry::new();
        registry
            .register(skill_with_deps(
                "minecraft",
                &["start minecraft server"],
                &["terminal.execute"],
                &["server"],
            ))
            .unwrap();
        registry
            .register(skill("server", &["start server"], &["terminal.execute"]))
            .unwrap();
        registry
            .register(skill("git", &["commit"], &["git.commit"]))
            .unwrap();

        let subset = registry.subset(&["minecraft"]);
        let all = subset.all();
        let ids: Vec<&str> = all.iter().map(|s| s.id()).collect();
        assert!(ids.contains(&"minecraft"), "ids: {ids:?}");
        assert!(
            ids.contains(&"server"),
            "dependency must ride along: {ids:?}"
        );
        assert!(!ids.contains(&"git"), "ids: {ids:?}");
        // minecraft is selectable inside the subset because its dep is present.
        assert!(subset.select_for("start my minecraft server", 5).is_some());
    }

    #[test]
    fn subset_preserves_enabled_state() {
        let registry = SkillRegistry::new();
        registry
            .register(skill("git", &["commit"], &["git.commit"]))
            .unwrap();
        registry
            .register(skill("docker", &["build image"], &["terminal.execute"]))
            .unwrap();
        registry.disable("docker").unwrap();

        let subset = registry.subset(&["git", "docker"]);
        let docker = subset.get("docker").unwrap();
        assert!(!docker.enabled, "disabled state must survive the subset");
        assert!(subset.get("git").unwrap().enabled);
    }

    #[test]
    fn reconcile_flips_resolution_when_capability_appears_or_disappears() {
        let registry = SkillRegistry::new();
        let mut s = skill("git", &["status"], &["git.status", "future.cap"]);
        // git.status resolved at load; future.cap soft-declared.
        s.capabilities = vec![
            CapabilityStatus::Resolved("git.status".to_string()),
            CapabilityStatus::SoftDeclared("future.cap".to_string()),
        ];
        registry.register(s).unwrap();

        // Stable registry view → no changes.
        let resolver = crate::loader::StaticCapabilityResolver::new(vec!["git.status".to_string()]);
        assert_eq!(registry.reconcile(&resolver), 0);

        // future.cap gets bound at runtime → flips to Resolved.
        let resolver = crate::loader::StaticCapabilityResolver::new(vec![
            "git.status".to_string(),
            "future.cap".to_string(),
        ]);
        assert_eq!(registry.reconcile(&resolver), 1);
        assert!(registry.get("git").unwrap().is_usable());

        // git.status disappears after load → flips back to SoftDeclared. The
        // skill stays usable via future.cap (still bound) — re-resolution
        // reflects *which* capabilities exist, not a skill-wide freeze.
        let resolver = crate::loader::StaticCapabilityResolver::new(vec!["future.cap".to_string()]);
        assert_eq!(registry.reconcile(&resolver), 1);
        let git = registry.get("git").unwrap();
        assert!(git.is_usable());
        assert_eq!(git.soft_declared_capabilities(), vec!["git.status"]);

        // Remove the last bound capability → the skill becomes unusable live.
        let resolver = crate::loader::StaticCapabilityResolver::new(vec![]);
        assert_eq!(registry.reconcile(&resolver), 1);
        let git = registry.get("git").unwrap();
        assert!(!git.is_usable());
        assert_eq!(
            git.soft_declared_capabilities(),
            vec!["git.status", "future.cap"]
        );
    }

    fn learned_candidate(id: &str) -> Skill {
        let mut s = skill(id, &["do the thing"], &["terminal.execute"]);
        s.source = SkillSource::Learned;
        s.enabled = false; // a compiled candidate starts disabled
        s
    }

    #[test]
    fn enable_learned_admits_only_learned_candidates() {
        let registry = SkillRegistry::new();
        let s = learned_candidate("learned");
        registry.register(s).unwrap();
        assert!(!registry.get("learned").unwrap().enabled);
        assert!(registry.select_for("do the thing please", 5).is_none());

        registry.enable_learned("learned").unwrap();
        let admitted = registry.get("learned").unwrap();
        assert!(admitted.enabled);
        assert!(admitted.is_usable());
        // A later goal's selection now finds it — the admission gate closes
        // the learning loop.
        let selected = registry.select_for("do the thing please", 5).unwrap();
        assert_eq!(selected.skill.id(), "learned");
    }

    #[test]
    fn enable_learned_refuses_non_learned_skills() {
        let registry = SkillRegistry::new();
        registry
            .register(skill("git", &["commit"], &["git.commit"]))
            .unwrap();
        assert!(matches!(
            registry.enable_learned("git"),
            Err(SkillError::NotLearned(_))
        ));
        // The regular enable gate is unaffected by the learned-only rule.
        assert!(registry.enable("git").is_ok());
    }

    #[test]
    fn enable_learned_refuses_unknown_skill() {
        let registry = SkillRegistry::new();
        assert!(matches!(
            registry.enable_learned("nope"),
            Err(SkillError::NotFound(_))
        ));
    }

    #[test]
    fn enable_learned_emits_the_enabled_event() {
        let sink = Arc::new(crate::events::VecSink::new());
        let registry = SkillRegistry::with_sink(sink.clone());
        registry.register(learned_candidate("learned")).unwrap();
        // Registration of a disabled candidate emits Loaded, not Enabled.
        assert!(!sink
            .events
            .lock()
            .expect("sink")
            .iter()
            .any(|e| matches!(e, SkillEvent::Enabled { .. })));

        registry.enable_learned("learned").unwrap();
        let events = sink.events.lock().expect("sink");
        let enabled: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, SkillEvent::Enabled { skill_id } if skill_id == "learned"))
            .collect();
        assert_eq!(enabled.len(), 1, "admission emits exactly one enabled");
    }
}
