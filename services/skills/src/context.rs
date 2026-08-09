//! `SkillContext` — the skill-aware view handed to the Context Builder (spec
//! `specs/skill-engine.md` §6). Pure data: the planner reasons in terms of
//! skills; the Context Builder decides how to present them to the LLM.

use serde::{Deserialize, Serialize};

use crate::registry::{RankedSkill, Skill, SkillRegistry};

/// Compact, LLM-safe summary of a skill. Contains **no prompts** — only the
/// metadata the Context Builder needs to render a "relevant skills" section.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub category: String,
    pub actions: Vec<String>,
    pub capabilities: Vec<String>,
    pub permission_hints: Vec<String>,
    pub readonly: bool,
}

impl From<&Skill> for SkillSummary {
    fn from(skill: &Skill) -> Self {
        Self {
            id: skill.manifest.id.clone(),
            name: skill.manifest.name.clone(),
            version: skill.manifest.version.clone(),
            description: skill.manifest.description.clone(),
            category: skill.manifest.category.clone(),
            actions: skill.manifest.actions.clone(),
            capabilities: skill.manifest.capabilities.clone(),
            permission_hints: skill.manifest.permissions.clone(),
            readonly: skill.manifest.readonly,
        }
    }
}

/// The skill-aware context handed to the Context Builder.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SkillContext {
    /// Skills ranked against the current goal (top-`limit`).
    pub relevant_skills: Vec<SkillSummary>,
    /// Currently active skill id, if any.
    pub active_skill: Option<String>,
}

/// Abstract source of [`SkillContext`] for a goal. Implemented by
/// [`SkillRegistry`]; consumed by the Context Builder without importing the
/// registry concretely.
pub trait SkillContextSource: Send + Sync {
    fn context_for(&self, goal: &str, limit: usize) -> SkillContext;
}

impl SkillContextSource for SkillRegistry {
    fn context_for(&self, goal: &str, limit: usize) -> SkillContext {
        // One ranking pass: select_for already scored the candidates; only
        // when nothing is selectable do we rank separately for the summaries.
        // (Single-pass avoids double-scoring and a torn read between passes.)
        let selected = self.select_for(goal, limit);
        let ranked: Vec<RankedSkill> = match &selected {
            Some(s) => s.candidates.clone(),
            None => self.search(goal, limit),
        };
        // Explainability trace: log every candidate skill with its relevance
        // score so a goal's "Minecraft 0.96 / Server 0.61" decision can be
        // audited. Scores live in the trace, not in the LLM-visible summaries.
        tracing::info!(
            goal = %goal,
            ranked = ?ranked
                .iter()
                .map(|r| (r.skill.id().to_string(), r.score))
                .collect::<Vec<_>>(),
            "skill ranking: candidate skills scored against goal"
        );
        // Deterministic "Chosen" skill: the top-ranked *usable* candidate
        // (skips disabled skills and broken dependency chains).
        SkillContext {
            relevant_skills: ranked
                .into_iter()
                .map(|ranked| SkillSummary::from(&*ranked.skill))
                .collect(),
            active_skill: selected.map(|s| s.skill.id().to_string()),
        }
    }
}

/// Convenience wrapper so callers can hold a boxed source over `&SkillRegistry`.
pub struct RegistryContextSource {
    pub registry: SkillRegistry,
}

impl SkillContextSource for RegistryContextSource {
    fn context_for(&self, goal: &str, limit: usize) -> SkillContext {
        self.registry.context_for(goal, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::SkillManifest;
    use crate::registry::{CapabilityStatus, Skill, SkillSource};
    use chrono::Utc;

    fn manifest(id: &str, readonly: bool) -> SkillManifest {
        SkillManifest {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            description: format!("{id} skill"),
            author: "test".to_string(),
            category: "developer".to_string(),
            icon: String::new(),
            capabilities: vec!["filesystem.read".to_string()],
            actions: vec!["read files".to_string()],
            permissions: vec!["filesystem.read".to_string()],
            readonly,
            dependencies: vec![],
            platform: "Any".to_string(),
            min_core_version: "0.0.0".to_string(),
            enabled: true,
            memory: Default::default(),
        }
    }

    fn skill(id: &str, readonly: bool) -> Skill {
        let m = manifest(id, readonly);
        Skill {
            manifest: m,
            source: SkillSource::Builtin,
            loaded_at: Utc::now(),
            enabled: true,
            capabilities: vec![CapabilityStatus::Resolved("filesystem.read".to_string())],
        }
    }

    #[test]
    fn context_for_returns_relevant_summaries() {
        let registry = SkillRegistry::new();
        registry.register(skill("reader", true)).unwrap();
        registry.register(skill("writer", false)).unwrap();

        let ctx = registry.context_for("read files safely", 5);
        assert_eq!(ctx.relevant_skills.len(), 2);
        let reader = ctx
            .relevant_skills
            .iter()
            .find(|s| s.id == "reader")
            .unwrap();
        assert!(reader.readonly);
        assert_eq!(reader.permission_hints, vec!["filesystem.read"]);
        // The top-ranked usable skill is chosen deterministically.
        assert_eq!(ctx.active_skill.as_deref(), Some("reader"));
    }

    #[test]
    fn summary_carries_no_prompt_text() {
        let s = skill("reader", true);
        let summary = SkillSummary::from(&s);
        let json = serde_json::to_value(&summary).unwrap();
        assert!(json.get("system_prompt").is_none());
        assert!(json.get("instructions").is_none());
        assert!(json.get("examples").is_none());
    }
}
