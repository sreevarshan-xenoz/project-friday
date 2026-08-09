//! Permission integration (spec `specs/skill-engine.md` §7).
//!
//! The Skill Engine becomes a **policy source** over the existing
//! `PermissionEngine` / `PolicyAuthority` layers instead of reinventing them.
//!
//! [`SkillPolicy`] answers one question: *should capability `X` run without a
//! user prompt?* It says yes when:
//! 1. the capability is covered by an enabled **readonly** skill and is
//!    classified safe by the [`RiskResolver`], or
//! 2. the capability is covered by a skill the user has **approved** this
//!    session ([`SkillApprovals`]).
//!
//! The planner's `PolicyAuthority` adapter (which maps `tool.action` →
//! capability id) lives at the integration point that already imports
//! `friday-planner` (e.g. `friday-context`), not here — this crate stays
//! decoupled.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::registry::SkillRegistry;

/// Abstract safe/unsafe classification of a capability id. Mirrors the `Risk`
/// classification in `friday-capabilities::spec` without importing it.
pub trait RiskResolver: Send + Sync {
    fn is_safe(&self, capability: &str) -> bool;
}

/// A configurable risk resolver over an explicit safe-capability list.
pub struct StaticRiskResolver {
    safe: HashSet<String>,
}

impl StaticRiskResolver {
    pub fn new(safe: Vec<String>) -> Self {
        Self {
            safe: safe.into_iter().collect(),
        }
    }
}

impl RiskResolver for StaticRiskResolver {
    fn is_safe(&self, capability: &str) -> bool {
        self.safe.contains(capability)
    }
}

/// Classifies every capability as safe — useful when no risk source is wired.
pub struct AllSafeRiskResolver;

impl RiskResolver for AllSafeRiskResolver {
    fn is_safe(&self, _capability: &str) -> bool {
        true
    }
}

/// Session-scoped set of skills the user has approved (spec §7: "once,
/// per-session, or per-action").
#[derive(Default)]
pub struct SkillApprovals {
    approved: Mutex<HashSet<String>>,
}

impl SkillApprovals {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn approve(&self, skill_id: &str) {
        self.approved
            .lock()
            .expect("approvals poisoned")
            .insert(skill_id.to_string());
    }

    pub fn revoke(&self, skill_id: &str) {
        self.approved
            .lock()
            .expect("approvals poisoned")
            .remove(skill_id);
    }

    pub fn is_approved(&self, skill_id: &str) -> bool {
        self.approved
            .lock()
            .expect("approvals poisoned")
            .contains(skill_id)
    }
}

/// Why a capability is (or isn't) allowed to run without a user prompt.
/// Richer than [`SkillPolicy::allows_capability`] so the decision trace can
/// show *why* a capability runs or prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    /// Covered by an enabled readonly skill and classified safe.
    AutoApproved,
    /// Covered by a skill the user approved this session.
    Approved,
    /// No loaded skill declares the capability — policy chains defer it to
    /// the previous policy (the Skill Engine is not authoritative here).
    Unmanaged,
    /// A covering skill exists but no readonly/approval path applies.
    PromptRequired,
}

/// The skill-aware policy source. Delegates nothing by itself — callers that
/// hold another policy chain it *after* [`SkillPolicy`] rejects.
pub struct SkillPolicy {
    registry: Arc<SkillRegistry>,
    approvals: Arc<SkillApprovals>,
    risk: Arc<dyn RiskResolver>,
}

impl SkillPolicy {
    pub fn new(
        registry: Arc<SkillRegistry>,
        approvals: Arc<SkillApprovals>,
        risk: Arc<dyn RiskResolver>,
    ) -> Self {
        Self {
            registry,
            approvals,
            risk,
        }
    }

    /// Why `capability` is allowed, prompts, or defers — the observability
    /// surface behind [`SkillPolicy::allows_capability`].
    ///
    /// Only **enabled** skills gate or grant: disabling a skill makes it
    /// permission-inert, so it neither auto-approves its safe capabilities
    /// nor keeps a stale session approval alive.
    pub fn permission_for(&self, capability: &str) -> PermissionState {
        let covering: Vec<_> = self
            .registry
            .for_capability(capability)
            .into_iter()
            .filter(|s| s.enabled)
            .collect();
        if covering.is_empty() {
            return PermissionState::Unmanaged;
        }
        // 1. readonly skill covering a safe capability → auto-approve.
        if covering
            .iter()
            .any(|s| s.manifest.readonly && self.risk.is_safe(capability))
        {
            return PermissionState::AutoApproved;
        }
        // 2. user-approved skill covering the capability → allow.
        if covering
            .iter()
            .any(|s| self.approvals.is_approved(&s.manifest.id))
        {
            return PermissionState::Approved;
        }
        PermissionState::PromptRequired
    }

    /// `true` if `capability` may run without a user prompt under skill policy.
    pub fn allows_capability(&self, capability: &str) -> bool {
        matches!(
            self.permission_for(capability),
            PermissionState::AutoApproved | PermissionState::Approved
        )
    }

    /// Inverse of [`SkillPolicy::allows_capability`].
    pub fn requires_prompt(&self, capability: &str) -> bool {
        !self.allows_capability(capability)
    }

    /// `true` if at least one **enabled** skill declares `capability`. Policy
    /// chains use this to decide whether the Skill Engine is authoritative for
    /// the capability at all: skills gate **what they declare** (disabled
    /// skills are permission-inert) and defer the rest to the next policy
    /// (e.g. an MCP tool no skill covers keeps running under the previous
    /// allow-all policy).
    pub fn manages_capability(&self, capability: &str) -> bool {
        self.registry
            .for_capability(capability)
            .iter()
            .any(|s| s.enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::SkillManifest;
    use crate::registry::{CapabilityStatus, Skill, SkillSource};
    use chrono::Utc;

    fn skill(registry: &SkillRegistry, id: &str, readonly: bool, caps: &[&str]) {
        let manifest = SkillManifest {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            author: "test".to_string(),
            category: "developer".to_string(),
            icon: String::new(),
            capabilities: caps.iter().map(|s| s.to_string()).collect(),
            actions: vec!["do it".to_string()],
            permissions: caps.iter().map(|s| s.to_string()).collect(),
            readonly,
            dependencies: vec![],
            platform: "Any".to_string(),
            min_core_version: "0.0.0".to_string(),
            enabled: true,
            memory: Default::default(),
        };
        registry
            .register(Skill {
                manifest,
                source: SkillSource::Builtin,
                loaded_at: Utc::now(),
                enabled: true,
                capabilities: caps
                    .iter()
                    .map(|c| CapabilityStatus::Resolved(c.to_string()))
                    .collect(),
            })
            .unwrap();
    }

    #[test]
    fn readonly_skill_auto_approves_safe_capabilities() {
        let registry = Arc::new(SkillRegistry::new());
        skill(&registry, "reader", true, &["filesystem.read"]);

        let policy = SkillPolicy::new(
            Arc::clone(&registry),
            Arc::new(SkillApprovals::new()),
            Arc::new(StaticRiskResolver::new(vec!["filesystem.read".to_string()])),
        );
        assert!(policy.allows_capability("filesystem.read"));
        assert!(!policy.allows_capability("terminal.execute"));
    }

    #[test]
    fn non_readonly_skill_requires_approval() {
        let registry = Arc::new(SkillRegistry::new());
        skill(&registry, "docker", false, &["terminal.execute"]);

        let approvals = Arc::new(SkillApprovals::new());
        let policy = SkillPolicy::new(
            Arc::clone(&registry),
            Arc::clone(&approvals),
            Arc::new(AllSafeRiskResolver),
        );
        assert!(!policy.allows_capability("terminal.execute"));

        approvals.approve("docker");
        assert!(policy.allows_capability("terminal.execute"));

        approvals.revoke("docker");
        assert!(!policy.allows_capability("terminal.execute"));
    }

    #[test]
    fn unknown_capability_is_not_allowed() {
        let registry = Arc::new(SkillRegistry::new());
        skill(&registry, "git", false, &["git.commit"]);
        let policy = SkillPolicy::new(
            Arc::clone(&registry),
            Arc::new(SkillApprovals::new()),
            Arc::new(AllSafeRiskResolver),
        );
        assert!(policy.requires_prompt("filesystem.delete"));
    }

    #[test]
    fn disabled_skill_grants_and_gates_nothing() {
        let registry = Arc::new(SkillRegistry::new());
        skill(&registry, "reader", true, &["filesystem.read"]);

        let approvals = Arc::new(SkillApprovals::new());
        let policy = SkillPolicy::new(
            Arc::clone(&registry),
            Arc::clone(&approvals),
            Arc::new(StaticRiskResolver::new(vec!["filesystem.read".to_string()])),
        );
        assert_eq!(
            policy.permission_for("filesystem.read"),
            PermissionState::AutoApproved
        );
        assert!(policy.manages_capability("filesystem.read"));

        // A disabled skill is permission-inert: no auto-approve, no
        // management — the capability defers to the previous policy.
        registry.disable("reader").unwrap();
        assert_eq!(
            policy.permission_for("filesystem.read"),
            PermissionState::Unmanaged
        );
        assert!(!policy.allows_capability("filesystem.read"));
        assert!(!policy.manages_capability("filesystem.read"));

        // Disabling an approved skill revokes the approval at policy level.
        skill(&registry, "docker", false, &["terminal.exec"]);
        approvals.approve("docker");
        assert_eq!(
            policy.permission_for("terminal.exec"),
            PermissionState::Approved
        );
        registry.disable("docker").unwrap();
        assert_eq!(
            policy.permission_for("terminal.exec"),
            PermissionState::Unmanaged
        );
    }

    #[test]
    fn permission_for_reports_all_states() {
        let registry = Arc::new(SkillRegistry::new());
        skill(&registry, "reader", true, &["filesystem.read"]);
        skill(&registry, "docker", false, &["terminal.exec"]);

        let approvals = Arc::new(SkillApprovals::new());
        let policy = SkillPolicy::new(
            Arc::clone(&registry),
            Arc::clone(&approvals),
            Arc::new(StaticRiskResolver::new(vec!["filesystem.read".to_string()])),
        );
        assert_eq!(
            policy.permission_for("filesystem.read"),
            PermissionState::AutoApproved
        );
        assert_eq!(
            policy.permission_for("terminal.exec"),
            PermissionState::PromptRequired
        );
        assert_eq!(
            policy.permission_for("mcp:filesystem.write_file"),
            PermissionState::Unmanaged
        );

        approvals.approve("docker");
        assert_eq!(
            policy.permission_for("terminal.exec"),
            PermissionState::Approved
        );
    }
}
