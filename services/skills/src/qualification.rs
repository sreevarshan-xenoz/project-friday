//! Skill qualification (M5.8) — the predictable gate every skill passes before
//! it is considered usable:
//!
//! ```text
//! Load → Validate → Dependency Check → Capability Check → Permission Check → Enable
//! ```
//!
//! This is the same pipeline Skill Forge (M7) will run on generated skills, so
//! "creating a skill" is just another producer of artifacts this gate consumes.

use crate::loader::CapabilityResolver;
use crate::manifest::SkillManifest;
use crate::registry::SkillRegistry;

/// Outcome of a single qualification check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Fail,
    Warning,
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub check: &'static str,
    pub status: CheckStatus,
    pub detail: String,
}

impl CheckResult {
    fn pass(check: &'static str, detail: impl Into<String>) -> Self {
        Self {
            check,
            status: CheckStatus::Pass,
            detail: detail.into(),
        }
    }

    fn fail(check: &'static str, detail: impl Into<String>) -> Self {
        Self {
            check,
            status: CheckStatus::Fail,
            detail: detail.into(),
        }
    }

    fn warn(check: &'static str, detail: impl Into<String>) -> Self {
        Self {
            check,
            status: CheckStatus::Warning,
            detail: detail.into(),
        }
    }
}

/// The full qualification result for one skill.
#[derive(Debug, Clone)]
pub struct QualificationReport {
    pub skill_id: String,
    /// `true` when every check passed (warnings are non-blocking).
    pub passed: bool,
    pub checks: Vec<CheckResult>,
}

impl QualificationReport {
    pub fn fails(&self) -> Vec<&CheckResult> {
        self.checks
            .iter()
            .filter(|c| c.status == CheckStatus::Fail)
            .collect()
    }
}

/// Run the qualification pipeline for one skill. `resolver` is the capability
/// registry view used at load time; `risk` optionally enables the readonly
/// safety check (skipped with a warning when absent).
pub fn qualify(
    registry: &SkillRegistry,
    resolver: &dyn CapabilityResolver,
    skill_id: &str,
) -> QualificationReport {
    let mut checks = Vec::new();
    let Some(skill) = registry.get(skill_id) else {
        return QualificationReport {
            skill_id: skill_id.to_string(),
            passed: false,
            checks: vec![CheckResult::fail("loaded", "skill is not registered")],
        };
    };

    // 1. Manifest validity (re-validate; a loaded skill already passed).
    match skill.manifest.validate() {
        Ok(()) => checks.push(CheckResult::pass(
            "manifest",
            format!(
                "id={} version={}",
                skill.manifest.id, skill.manifest.version
            ),
        )),
        Err(e) => checks.push(CheckResult::fail("manifest", e.to_string())),
    }

    // 2. Capability resolution — every declared capability must be resolvable
    //    or explicitly soft-declared for a future capability.
    let resolved = skill.resolved_capabilities().len();
    let soft = skill.soft_declared_capabilities().len();
    let total = skill.manifest.capabilities.len();
    if total == 0 {
        checks.push(CheckResult::warn(
            "capabilities",
            "skill declares no capabilities",
        ));
    } else if resolved > 0 {
        checks.push(CheckResult::pass(
            "capabilities",
            format!("{resolved}/{total} resolved, {soft} soft-declared"),
        ));
    } else {
        checks.push(CheckResult::fail(
            "capabilities",
            format!(
                "0/{total} resolved (all soft-declared: {:?})",
                skill.soft_declared_capabilities()
            ),
        ));
    }

    // 3. Permission declarations — every permission should be declared as a
    //    capability too (or be resolvable on its own).
    let undeclared: Vec<&String> = skill
        .manifest
        .permissions
        .iter()
        .filter(|p| !skill.manifest.capabilities.contains(p) && !resolver.exists(p))
        .collect();
    if undeclared.is_empty() {
        checks.push(CheckResult::pass(
            "permissions",
            format!(
                "{} permission(s) declared",
                skill.manifest.permissions.len()
            ),
        ));
    } else {
        checks.push(CheckResult::warn(
            "permissions",
            format!("undeclared permissions: {undeclared:?}"),
        ));
    }

    // 4. Dependencies — present and enabled.
    if skill.manifest.dependencies.is_empty() {
        checks.push(CheckResult::pass("dependencies", "no dependencies"));
    } else {
        let mut ok = true;
        let mut detail = String::new();
        for dep in &skill.manifest.dependencies {
            match registry.get(dep) {
                Some(d) if d.enabled => detail.push_str(&format!("{dep}:ok ")),
                Some(_) => {
                    ok = false;
                    detail.push_str(&format!("{dep}:disabled "));
                }
                None => {
                    ok = false;
                    detail.push_str(&format!("{dep}:missing "));
                }
            }
        }
        if ok {
            checks.push(CheckResult::pass("dependencies", detail.trim()));
        } else {
            checks.push(CheckResult::fail("dependencies", detail.trim()));
        }
    }

    // 5. Platform + core version.
    if skill.manifest.platform_compatible() {
        checks.push(CheckResult::pass(
            "platform",
            format!("compatible with {}", SkillManifest::host_platform()),
        ));
    } else {
        checks.push(CheckResult::fail(
            "platform",
            format!("requires {}", skill.manifest.platform),
        ));
    }
    if skill.manifest.core_version_ok() {
        checks.push(CheckResult::pass(
            "core_version",
            format!("min {} satisfied", skill.manifest.min_core_version),
        ));
    } else {
        checks.push(CheckResult::fail(
            "core_version",
            format!("min {} not satisfied", skill.manifest.min_core_version),
        ));
    }

    // 6. Readonly safety (requires a risk source; deferred to SkillPolicy when
    //    none is wired).
    if skill.manifest.readonly {
        checks.push(CheckResult::warn(
            "readonly",
            "readonly skills auto-approve only Risk::Safe capabilities at permission time",
        ));
    }

    let passed = !checks.iter().any(|c| c.status == CheckStatus::Fail);
    QualificationReport {
        skill_id: skill_id.to_string(),
        passed,
        checks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::StaticCapabilityResolver;
    use crate::manifest::SkillManifest;
    use crate::registry::{CapabilityStatus, Skill, SkillRegistry, SkillSource};
    use chrono::Utc;

    fn skill(registry: &SkillRegistry, id: &str, deps: &[&str], caps: &[&str]) {
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
            readonly: false,
            dependencies: deps.iter().map(|s| s.to_string()).collect(),
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
    fn qualification_passes_for_healthy_skill() {
        let registry = SkillRegistry::new();
        skill(&registry, "git", &[], &["git.status", "git.commit"]);
        let resolver =
            StaticCapabilityResolver::new(vec!["git.status".to_string(), "git.commit".to_string()]);
        let report = qualify(&registry, &resolver, "git");
        assert!(report.passed, "{report:?}");
        assert_eq!(report.fails().len(), 0);
    }

    #[test]
    fn qualification_fails_for_missing_dependency() {
        let registry = SkillRegistry::new();
        skill(&registry, "docker", &["developer"], &["terminal.execute"]);
        let resolver = StaticCapabilityResolver::new(vec!["terminal.execute".to_string()]);
        let report = qualify(&registry, &resolver, "docker");
        assert!(!report.passed);
        assert!(report
            .checks
            .iter()
            .any(|c| c.check == "dependencies" && c.status == CheckStatus::Fail));
    }

    #[test]
    fn qualification_flags_unresolvable_capabilities() {
        let registry = SkillRegistry::new();
        // A skill whose only capability is soft-declared (unresolvable at load).
        let manifest = SkillManifest {
            id: "ghost".to_string(),
            name: "Ghost".to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            author: "test".to_string(),
            category: "developer".to_string(),
            icon: String::new(),
            capabilities: vec!["nothing.exists".to_string()],
            actions: vec!["do it".to_string()],
            permissions: vec![],
            readonly: false,
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
                capabilities: vec![CapabilityStatus::SoftDeclared("nothing.exists".to_string())],
            })
            .unwrap();
        let resolver = StaticCapabilityResolver::new(vec![]);
        let report = qualify(&registry, &resolver, "ghost");
        assert!(!report.passed);
        assert!(report
            .checks
            .iter()
            .any(|c| c.check == "capabilities" && c.status == CheckStatus::Fail));
    }

    #[test]
    fn qualification_warns_for_unknown_skill() {
        let registry = SkillRegistry::new();
        let resolver = StaticCapabilityResolver::new(vec![]);
        let report = qualify(&registry, &resolver, "nope");
        assert!(!report.passed);
    }
}
