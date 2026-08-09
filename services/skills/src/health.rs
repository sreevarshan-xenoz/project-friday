//! Skill health — the runtime wellness view of a loaded skill.
//!
//! [`SkillHealth`] answers one question per skill: *can this skill do its job
//! right now?* It combines the states the Skill Engine already owns — enabled,
//! capability resolution (fresh after [`SkillRegistry::reconcile`]) — with
//! *live* capability health from the registry that actually serves it. The
//! result is a small, debuggable status surface (`Healthy` / `Warning` /
//! `Broken` / `Disabled`) that hosts can render as a dashboard row or feed
//! into observability.
//!
//! Capability health is abstracted behind [`CapabilityHealthSource`] so this
//! crate stays decoupled from `friday-capabilities`; integration adapters
//! (e.g. `friday-context`'s `RegistryHealthSource`) wire the live registry in.

use crate::registry::{Skill, SkillRegistry};

/// Live availability of one capability, abstracted from
/// `friday_capabilities::spec::CapabilityHealth` so this crate stays
/// decoupled from the capabilities crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapHealth {
    /// Reachable and usable.
    Healthy,
    /// Reachable but degraded (slow / partial). Still usable, worth a warning.
    Degraded,
    /// Unreachable. The capability cannot be served right now.
    Offline,
}

/// Abstract source of live capability health. Implemented over the planner's
/// capability registry at integration points.
pub trait CapabilityHealthSource: Send + Sync {
    fn health_of(&self, capability: &str) -> CapHealth;
}

/// A health source where every capability is healthy — the default for tests
/// and for hosts without live health wiring.
pub struct AllHealthyHealthSource;

impl CapabilityHealthSource for AllHealthyHealthSource {
    fn health_of(&self, _capability: &str) -> CapHealth {
        CapHealth::Healthy
    }
}

/// The wellness state of one skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillHealth {
    /// Enabled, usable, and every resolved capability is healthy.
    Healthy,
    /// Enabled and usable but something external is off: a resolved
    /// capability is offline/degraded, or the skill declares no capabilities
    /// of its own (aggregator/meta-skill).
    Warning(String),
    /// Structurally unusable — the skill declares capabilities but none
    /// resolve against the registry (soft-declared only, or removed after
    /// load). The skill cannot do anything until its capabilities exist.
    Broken(String),
    /// Disabled by the user (or by the loader because a dependency failed).
    Disabled,
}

impl SkillRegistry {
    /// The health of one loaded skill, or `None` if it is not registered.
    pub fn health_of(
        &self,
        skill_id: &str,
        health: &dyn CapabilityHealthSource,
    ) -> Option<SkillHealth> {
        self.get(skill_id)
            .map(|skill| assess_health(&skill, health))
    }

    /// Health of every loaded skill, sorted by id.
    pub fn health_summary(
        &self,
        health: &dyn CapabilityHealthSource,
    ) -> Vec<(String, SkillHealth)> {
        let mut out: Vec<(String, SkillHealth)> = self
            .all()
            .into_iter()
            .map(|skill| {
                let h = assess_health(&skill, health);
                (skill.id().to_string(), h)
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

/// The health state machine. Order matters: Disabled wins over everything;
/// structural breakage wins over external warnings.
fn assess_health(skill: &Skill, health: &dyn CapabilityHealthSource) -> SkillHealth {
    if !skill.enabled {
        return SkillHealth::Disabled;
    }
    if skill.manifest.capabilities.is_empty() {
        return SkillHealth::Warning("declares no capabilities (aggregator skill)".into());
    }
    if skill.resolved_capabilities().is_empty() {
        return SkillHealth::Broken("no resolvable capabilities".into());
    }
    for capability in skill.resolved_capabilities() {
        match health.health_of(capability) {
            CapHealth::Offline => {
                return SkillHealth::Warning(format!("capability {capability} is offline"));
            }
            CapHealth::Degraded => {
                return SkillHealth::Warning(format!("capability {capability} is degraded"));
            }
            CapHealth::Healthy => {}
        }
    }
    SkillHealth::Healthy
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::SkillManifest;
    use crate::registry::{CapabilityStatus, SkillSource};
    use chrono::Utc;

    struct MockHealthSource(Vec<(&'static str, CapHealth)>);

    impl CapabilityHealthSource for MockHealthSource {
        fn health_of(&self, capability: &str) -> CapHealth {
            self.0
                .iter()
                .find(|(c, _)| *c == capability)
                .map(|(_, h)| *h)
                .unwrap_or(CapHealth::Healthy)
        }
    }

    fn skill_with(
        registry: &SkillRegistry,
        id: &str,
        caps: &[&str],
        statuses: Vec<CapabilityStatus>,
        enabled: bool,
    ) {
        let manifest = SkillManifest {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            author: "test".to_string(),
            category: "developer".to_string(),
            icon: String::new(),
            capabilities: caps.iter().map(|s| s.to_string()).collect(),
            actions: vec!["do things".to_string()],
            permissions: Vec::new(),
            readonly: false,
            dependencies: Vec::new(),
            platform: "Any".to_string(),
            min_core_version: "0.0.0".to_string(),
            enabled: true,
            memory: Default::default(),
        };
        registry
            .register(crate::registry::Skill {
                manifest,
                source: SkillSource::Builtin,
                loaded_at: Utc::now(),
                enabled,
                capabilities: statuses,
            })
            .unwrap();
    }

    #[test]
    fn health_is_healthy_when_enabled_usable_and_all_capabilities_up() {
        let registry = SkillRegistry::new();
        skill_with(
            &registry,
            "git",
            &["git.status"],
            vec![CapabilityStatus::Resolved("git.status".into())],
            true,
        );
        assert_eq!(
            registry.health_of("git", &AllHealthyHealthSource),
            Some(SkillHealth::Healthy)
        );
    }

    #[test]
    fn disabled_skill_reports_disabled() {
        let registry = SkillRegistry::new();
        skill_with(
            &registry,
            "git",
            &["git.status"],
            vec![CapabilityStatus::Resolved("git.status".into())],
            false,
        );
        assert_eq!(
            registry.health_of("git", &AllHealthyHealthSource),
            Some(SkillHealth::Disabled)
        );
    }

    #[test]
    fn soft_declared_only_skill_is_broken() {
        let registry = SkillRegistry::new();
        skill_with(
            &registry,
            "ghost",
            &["nothing.exists"],
            vec![CapabilityStatus::SoftDeclared("nothing.exists".into())],
            true,
        );
        assert!(matches!(
            registry.health_of("ghost", &AllHealthyHealthSource),
            Some(SkillHealth::Broken(_))
        ));
    }

    #[test]
    fn offline_capability_warns() {
        let registry = SkillRegistry::new();
        skill_with(
            &registry,
            "docker",
            &["terminal.exec"],
            vec![CapabilityStatus::Resolved("terminal.exec".into())],
            true,
        );
        let health = MockHealthSource(vec![("terminal.exec", CapHealth::Offline)]);
        assert!(matches!(
            registry.health_of("docker", &health),
            Some(SkillHealth::Warning(msg)) if msg.contains("offline")
        ));
    }

    #[test]
    fn aggregator_skill_without_capabilities_warns() {
        let registry = SkillRegistry::new();
        skill_with(&registry, "developer", &[], vec![], true);
        assert!(matches!(
            registry.health_of("developer", &AllHealthyHealthSource),
            Some(SkillHealth::Warning(_))
        ));
    }

    #[test]
    fn health_summary_is_sorted_by_id() {
        let registry = SkillRegistry::new();
        skill_with(
            &registry,
            "zebra",
            &["terminal.exec"],
            vec![CapabilityStatus::Resolved("terminal.exec".into())],
            true,
        );
        skill_with(
            &registry,
            "alpha",
            &["terminal.exec"],
            vec![CapabilityStatus::Resolved("terminal.exec".into())],
            true,
        );
        let summary = registry.health_summary(&AllHealthyHealthSource);
        assert_eq!(summary[0].0, "alpha");
        assert_eq!(summary[1].0, "zebra");
    }
}
