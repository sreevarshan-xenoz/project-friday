//! `SkillManifest` — the on-disk declaration of a skill. Pure data.
//!
//! Design rules (spec `specs/skill-engine.md` §1.1):
//! * A skill contains **no prompts, no instructions, no personality**.
//! * Unknown JSON fields (e.g. legacy `examples`) are ignored by serde, so the
//!   runtime model never carries prompt-like content.
//! * `readonly` is an optional manifest extension: a readonly skill promises to
//!   only touch the world through `Risk::Safe` capabilities, so the Permission
//!   Engine can auto-approve those without a prompt.

use serde::{Deserialize, Serialize};

use crate::error::SkillError;

/// The FRIDAY core version the Skill Engine currently validates manifests
/// against. Bump when the skill contract changes.
pub const FRIDAY_CORE_VERSION: &str = "0.1.0";

/// Minimal semver (`X.Y.Z`) used for `version` and `min_core_version`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CoreVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl CoreVersion {
    pub fn parse(s: &str) -> Result<Self, String> {
        let mut parts = s.trim().split('.');
        let (major, minor, patch) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(a), Some(b), Some(c), None) => (a, b, c),
            _ => return Err(format!("'{s}' is not a X.Y.Z semantic version")),
        };
        let parse = |part: &str| -> Result<u64, String> {
            part.parse::<u64>()
                .map_err(|_| format!("invalid version part '{part}' in '{s}'"))
        };
        Ok(Self {
            major: parse(major)?,
            minor: parse(minor)?,
            patch: parse(patch)?,
        })
    }

    pub fn current() -> Self {
        Self::parse(FRIDAY_CORE_VERSION).expect("FRIDAY_CORE_VERSION must be valid")
    }
}

fn default_author() -> String {
    "FRIDAY".to_string()
}

fn default_platform() -> String {
    "Any".to_string()
}

fn default_min_core() -> String {
    "0.0.0".to_string()
}

fn default_true() -> bool {
    true
}

/// The memory a skill declares it needs (M8 — memory-aware skills). Skills
/// stay pure data: they *declare* which memory keys they read and write, and
/// the runtime hands each skill a [`crate::memory::SkillMemory`] handle that
/// only allows exactly those keys — deterministic, model-independent memory
/// instead of prompt-remembered state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillMemorySpec {
    /// Keys the skill may read deterministically (`memory.read <key>`).
    #[serde(default)]
    pub reads: Vec<String>,
    /// Keys the skill may persist (`memory.store <key> <value>`).
    #[serde(default)]
    pub writes: Vec<String>,
}

/// The on-disk skill declaration. Parsed from `skill.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillManifest {
    /// Unique slug (`docker`, `git`, ...).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// SemVer of this skill.
    pub version: String,
    /// One-sentence description of what the skill does.
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_author")]
    pub author: String,
    /// Grouping: `developer`, `productivity`, `media`, `iot`, `gaming`, ...
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub icon: String,
    /// Capability IDs this skill needs from the registry.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Natural-language action descriptions used for goal matching.
    #[serde(default)]
    pub actions: Vec<String>,
    /// Capabilities that require user approval before use.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// `true` => skill promises to only use `Risk::Safe` capabilities.
    #[serde(default)]
    pub readonly: bool,
    /// Other skill IDs that must be loaded first.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// `Any`, `Windows`, or `Linux`.
    #[serde(default = "default_platform")]
    pub platform: String,
    /// Minimum FRIDAY core version required.
    #[serde(default = "default_min_core")]
    pub min_core_version: String,
    /// Whether the skill starts enabled after load.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Declared memory keys (M8). Absent = the skill uses no deterministic
    /// memory; every read/write then requires a declared key.
    #[serde(default)]
    pub memory: SkillMemorySpec,
}

impl SkillManifest {
    /// Host platform name, derived at compile time.
    pub fn host_platform() -> &'static str {
        if cfg!(target_os = "windows") {
            "Windows"
        } else if cfg!(target_os = "linux") {
            "Linux"
        } else {
            "Any"
        }
    }

    /// `true` if the manifest can run on the current host.
    pub fn platform_compatible(&self) -> bool {
        matches!(self.platform.as_str(), "Any" | "*" | "all")
            || self.platform == Self::host_platform()
    }

    /// `true` if `min_core_version` is satisfied by the running core.
    pub fn core_version_ok(&self) -> bool {
        CoreVersion::parse(&self.min_core_version)
            .map(|v| v <= CoreVersion::current())
            .unwrap_or(false)
    }

    /// Validate the manifest against the Skill Engine rules. Returns a
    /// [`SkillError`] with a specific reason on the first violation.
    pub fn validate(&self) -> Result<(), SkillError> {
        let invalid = |reason: String| SkillError::Invalid {
            id: self.id.clone(),
            reason,
        };

        if self.id.is_empty() {
            return Err(invalid("id must not be empty".into()));
        }
        if !self
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(invalid(
                "id must be a slug using only a-z, 0-9, '-', '_'".into(),
            ));
        }
        if self.name.is_empty() {
            return Err(invalid("name must not be empty".into()));
        }
        if CoreVersion::parse(&self.version).is_err() {
            return Err(invalid(format!(
                "version '{0}' is not X.Y.Z semver",
                self.version
            )));
        }
        if self.actions.is_empty() {
            return Err(invalid("actions must not be empty".into()));
        }
        if self.dependencies.iter().any(|dep| dep == &self.id) {
            return Err(invalid("a skill cannot depend on itself".into()));
        }
        if !self.platform_compatible() {
            return Err(SkillError::Platform(self.platform.clone()));
        }
        if !self.core_version_ok() {
            return Err(SkillError::CoreVersionTooNew {
                manifest: self.min_core_version.clone(),
                core: FRIDAY_CORE_VERSION.to_string(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_validates() {
        let manifest: SkillManifest = serde_json::from_str(
            r#"{
                "id": "docker",
                "name": "Docker",
                "version": "1.0.0",
                "actions": ["build docker image", "docker compose up"],
                "capabilities": ["terminal.execute", "filesystem.read"],
                "permissions": ["terminal.execute"],
                "readonly": false,
                "platform": "Any"
            }"#,
        )
        .unwrap();
        assert!(manifest.validate().is_ok());
        assert!(!manifest.readonly);
        assert_eq!(manifest.author, "FRIDAY");
        assert!(manifest.platform_compatible());
        assert!(manifest.core_version_ok());
    }

    #[test]
    fn parses_memory_declaration() {
        let manifest: SkillManifest = serde_json::from_str(
            r#"{
                "id": "minecraft",
                "name": "Minecraft Server",
                "version": "1.0.0",
                "actions": ["start minecraft server"],
                "capabilities": ["terminal.exec"],
                "memory": {
                    "reads": ["minecraft.server_state"],
                    "writes": ["minecraft.server_state"]
                }
            }"#,
        )
        .unwrap();
        assert_eq!(manifest.memory.reads, vec!["minecraft.server_state"]);
        assert_eq!(manifest.memory.writes, vec!["minecraft.server_state"]);

        // No memory section → defaults to empty.
        let plain: SkillManifest = serde_json::from_str(
            r#"{"id":"git","name":"Git","version":"1.0.0","actions":["commit"]}"#,
        )
        .unwrap();
        assert!(plain.memory.reads.is_empty());
        assert!(plain.memory.writes.is_empty());
    }

    #[test]
    fn ignores_unknown_fields() {
        // Existing bundled manifests carry `examples` and `icon`; they must load
        // without polluting the runtime model.
        let manifest: SkillManifest = serde_json::from_str(
            r#"{
                "id": "git",
                "name": "Git",
                "version": "1.0.0",
                "actions": ["commit changes"],
                "capabilities": ["git.status"],
                "icon": "⎇",
                "examples": [{"goal": "commit", "plan": ["git commit"]}]
            }"#,
        )
        .unwrap();
        assert_eq!(manifest.id, "git");
    }

    #[test]
    fn rejects_invalid_manifests() {
        let cases: Vec<(&str, &str)> = vec![
            (
                r#"{"id":"","name":"x","version":"1.0.0","actions":["a"]}"#,
                "empty id",
            ),
            (
                r#"{"id":"bad id!","name":"x","version":"1.0.0","actions":["a"]}"#,
                "non-slug id",
            ),
            (
                r#"{"id":"x","name":"","version":"1.0.0","actions":["a"]}"#,
                "empty name",
            ),
            (
                r#"{"id":"x","name":"x","version":"not-semver","actions":["a"]}"#,
                "bad version",
            ),
            (
                r#"{"id":"x","name":"x","version":"1.0.0","actions":[]}"#,
                "empty actions",
            ),
            (
                r#"{"id":"x","name":"x","version":"1.0.0","actions":["a"],"dependencies":["x"]}"#,
                "self dependency",
            ),
            (
                r#"{"id":"x","name":"x","version":"1.0.0","actions":["a"],"platform":"Linux"}"#,
                "platform mismatch",
            ),
            (
                r#"{"id":"x","name":"x","version":"1.0.0","actions":["a"],"min_core_version":"99.0.0"}"#,
                "core too new",
            ),
        ];
        for (json, label) in cases {
            let manifest: SkillManifest = serde_json::from_str(json).unwrap();
            assert!(manifest.validate().is_err(), "expected failure: {label}");
        }
    }

    #[test]
    fn core_version_ordering() {
        assert!(CoreVersion::parse("0.1.0").unwrap() < CoreVersion::parse("0.2.0").unwrap());
        assert!(CoreVersion::parse("1.2.3").unwrap() > CoreVersion::parse("1.2.2").unwrap());
        assert!(CoreVersion::parse("0.0.0").unwrap() < CoreVersion::current());
        assert!(CoreVersion::parse("1.2.3.4").is_err());
    }
}
