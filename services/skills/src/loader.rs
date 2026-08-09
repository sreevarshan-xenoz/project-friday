//! `SkillLoader` — discovers, validates, and loads skills from disk (spec
//! `specs/skill-engine.md` §4).
//!
//! Pipeline per source directory:
//!
//! ```text
//! scan skill.json files
//!   → parse manifest
//!   → validate (id/version/actions/platform/core)
//!   → order by dependencies (topological sort, cycle detection)
//!   → resolve capabilities (resolved vs. soft-declared)
//!   → register into the SkillRegistry
//! ```
//!
//! The loader never fails the whole batch on one bad manifest: failures are
//! collected into a [`LoadReport`] and surfaced as `skill.error` events.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;

use crate::error::SkillError;
use crate::events::SkillEvent;
use crate::manifest::SkillManifest;
use crate::registry::{CapabilityStatus, Skill, SkillRegistry, SkillSource};

/// Abstract check for whether a capability id is bound in the Capability
/// Registry. Keeps this crate decoupled from `friday-capabilities` /
/// `friday-planner`; integration adapters implement it at wiring points.
pub trait CapabilityResolver: Send + Sync {
    fn exists(&self, capability: &str) -> bool;
}

/// Resolver over a fixed set of known capability ids — for tests and
/// soft-declaration demos.
pub struct StaticCapabilityResolver {
    known: Vec<String>,
}

impl StaticCapabilityResolver {
    pub fn new(known: Vec<String>) -> Self {
        Self { known }
    }
}

impl CapabilityResolver for StaticCapabilityResolver {
    fn exists(&self, capability: &str) -> bool {
        self.known.iter().any(|k| k == capability)
    }
}

/// A single manifest that failed to load, with the reason.
#[derive(Debug, Clone)]
pub struct LoadFailure {
    pub path: String,
    pub error: String,
}

/// Result of loading a source directory.
#[derive(Debug, Clone)]
pub struct LoadReport {
    pub source: SkillSource,
    pub scanned: usize,
    pub loaded: usize,
    pub failed: Vec<LoadFailure>,
}

impl LoadReport {
    pub fn ok(&self) -> bool {
        self.failed.is_empty()
    }
}

struct Candidate {
    path: PathBuf,
    manifest: SkillManifest,
    source: SkillSource,
}

/// Discovers and loads skills from built-in / user / plugin directories.
pub struct SkillLoader {
    registry: Arc<SkillRegistry>,
    resolver: Arc<dyn CapabilityResolver>,
}

impl SkillLoader {
    pub fn new(registry: Arc<SkillRegistry>, resolver: Arc<dyn CapabilityResolver>) -> Self {
        Self { registry, resolver }
    }

    /// The capability resolver, exposed for qualification checks.
    pub fn capability_resolver(&self) -> Arc<dyn CapabilityResolver> {
        Arc::clone(&self.resolver)
    }

    /// Load every `skill.json` under `dir` (one level deep:
    /// `<dir>/<id>/skill.json` or a flat `<dir>/skill.json`).
    pub fn load_dir(&self, dir: &Path, source: SkillSource) -> Result<LoadReport, SkillError> {
        if !dir.is_dir() {
            return Err(SkillError::Io {
                path: dir.display().to_string(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "directory not found"),
            });
        }

        let mut paths = Vec::new();
        walk_for_manifests(dir, &mut paths);
        paths.sort();

        let sink = self.registry.event_sink();
        sink.emit(SkillEvent::Scanned {
            source,
            count: paths.len(),
        });

        // Phase 1: parse + validate.
        let mut candidates = Vec::new();
        let mut failed: Vec<LoadFailure> = Vec::new();
        for path in &paths {
            match parse_candidate(path, source) {
                Ok(c) => candidates.push(c),
                Err(f) => failed.push(f),
            }
        }

        // Phase 2: topological dependency order (unresolved deps / cycles fail
        // the affected skills only).
        let (ordered, order_failures) = order_by_dependencies(candidates);
        failed.extend(order_failures);

        // Phase 3: resolve capabilities + register.
        let mut loaded = 0usize;
        for cand in ordered {
            let deps_ok = cand
                .manifest
                .dependencies
                .iter()
                .all(|d| self.registry.get(d).is_some_and(|s| s.enabled));
            if !deps_ok {
                failed.push(LoadFailure {
                    path: cand.path.display().to_string(),
                    error: format!(
                        "dependency of skill '{}' is missing or disabled",
                        cand.manifest.id
                    ),
                });
                continue;
            }
            let enabled = cand.manifest.enabled && deps_ok;
            let skill_id = cand.manifest.id.clone();
            let skill = Skill {
                capabilities: cand
                    .manifest
                    .capabilities
                    .iter()
                    .map(|cap| {
                        if self.resolver.exists(cap) {
                            CapabilityStatus::Resolved(cap.clone())
                        } else {
                            CapabilityStatus::SoftDeclared(cap.clone())
                        }
                    })
                    .collect(),
                manifest: cand.manifest,
                source: cand.source,
                loaded_at: Utc::now(),
                enabled,
            };
            match self.registry.register(skill) {
                Ok(()) => loaded += 1,
                Err(e) => {
                    sink.emit(SkillEvent::Error {
                        skill_id: skill_id.clone(),
                        error: e.to_string(),
                    });
                    failed.push(LoadFailure {
                        path: cand.path.display().to_string(),
                        error: e.to_string(),
                    });
                }
            }
        }

        Ok(LoadReport {
            source,
            scanned: paths.len(),
            loaded,
            failed,
        })
    }

    /// Convenience for the bundled skills directory.
    pub fn load_bundled(&self, dir: &Path) -> Result<LoadReport, SkillError> {
        self.load_dir(dir, SkillSource::Builtin)
    }
}

fn walk_for_manifests(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let nested = path.join("skill.json");
            if nested.is_file() {
                out.push(nested);
            } else {
                walk_for_manifests(&path, out);
            }
        } else if path.file_name().is_some_and(|n| n == "skill.json") {
            out.push(path);
        }
    }
}

fn parse_candidate(path: &Path, source: SkillSource) -> Result<Candidate, LoadFailure> {
    let fail = |error: String| LoadFailure {
        path: path.display().to_string(),
        error,
    };
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) => return Err(fail(format!("cannot read manifest: {e}"))),
    };
    let manifest: SkillManifest = match serde_json::from_str(&raw) {
        Ok(m) => m,
        Err(e) => return Err(fail(format!("invalid skill.json: {e}"))),
    };
    if let Err(e) = manifest.validate() {
        return Err(fail(e.to_string()));
    }
    Ok(Candidate {
        path: path.to_path_buf(),
        manifest,
        source,
    })
}

/// Kahn's algorithm over the dependency graph. Returns the manifests in
/// dependency-first order, plus a failure for each manifest with a missing
/// dependency or participating in a cycle.
fn order_by_dependencies(candidates: Vec<Candidate>) -> (Vec<Candidate>, Vec<LoadFailure>) {
    let fail = |c: &Candidate, error: String| LoadFailure {
        path: c.path.display().to_string(),
        error,
    };

    let ids: Vec<String> = candidates.iter().map(|c| c.manifest.id.clone()).collect();
    let index: std::collections::HashMap<&str, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();

    let mut deps_remaining: Vec<usize> = candidates
        .iter()
        .map(|c| {
            c.manifest
                .dependencies
                .iter()
                .filter(|d| index.contains_key(d.as_str()))
                .count()
        })
        .collect();
    // Dependencies not present in the batch are unresolved (fail that skill).
    let mut failed: Vec<LoadFailure> = Vec::new();
    for c in &candidates {
        for dep in &c.manifest.dependencies {
            if !index.contains_key(dep.as_str()) {
                failed.push(fail(
                    c,
                    SkillError::UnresolvedDependency {
                        skill: dep.clone(),
                        by: c.manifest.id.clone(),
                    }
                    .to_string(),
                ));
                // Mark it as "processed" so it is not queued or cycle-flagged.
                let idx = index[&c.manifest.id.as_str()];
                deps_remaining[idx] = usize::MAX;
            }
        }
    }

    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); candidates.len()];
    for (i, c) in candidates.iter().enumerate() {
        for dep in &c.manifest.dependencies {
            if let Some(&j) = index.get(dep.as_str()) {
                dependents[j].push(i);
            }
        }
    }

    // Kahn's algorithm.
    let mut queue: Vec<usize> = deps_remaining
        .iter()
        .enumerate()
        .filter(|(_, &n)| n == 0)
        .map(|(i, _)| i)
        .collect();
    let mut order: Vec<usize> = Vec::with_capacity(candidates.len());
    while let Some(i) = queue.pop() {
        order.push(i);
        for &j in &dependents[i] {
            if deps_remaining[j] == usize::MAX {
                continue;
            }
            deps_remaining[j] -= 1;
            if deps_remaining[j] == 0 {
                queue.push(j);
            }
        }
    }

    // Anything left with deps_remaining in normal range is part of a cycle.
    for (i, c) in candidates.iter().enumerate() {
        if deps_remaining[i] != 0 && deps_remaining[i] != usize::MAX {
            let cycle_ids: Vec<String> = (0..candidates.len())
                .filter(|&k| deps_remaining[k] != 0 && deps_remaining[k] != usize::MAX)
                .map(|k| candidates[k].manifest.id.clone())
                .collect();
            failed.push(fail(c, SkillError::DependencyCycle(cycle_ids).to_string()));
        }
    }

    let mut by_index: Vec<Option<Candidate>> = candidates.into_iter().map(Some).collect();
    let ordered: Vec<Candidate> = order
        .into_iter()
        .filter_map(|i| by_index[i].take())
        .collect();
    (ordered, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(dir: &Path, id: &str, json: &str) {
        let sub = dir.join(id);
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("skill.json"), json).unwrap();
    }

    fn basic(id: &str, actions: &[&str], deps: &[&str]) -> String {
        let deps_json: Vec<String> = deps.iter().map(|d| format!("\"{d}\"")).collect();
        let actions_json: Vec<String> = actions.iter().map(|a| format!("\"{a}\"")).collect();
        format!(
            r#"{{"id":"{id}","name":"{id}","version":"1.0.0","actions":[{}],"capabilities":["terminal.execute"],"dependencies":[{}]}}"#,
            actions_json.join(","),
            deps_json.join(",")
        )
    }

    fn loader(known_caps: &[&str]) -> (Arc<SkillRegistry>, SkillLoader) {
        let registry = Arc::new(SkillRegistry::new());
        let resolver: Arc<dyn CapabilityResolver> = Arc::new(StaticCapabilityResolver::new(
            known_caps.iter().map(|s| s.to_string()).collect(),
        ));
        let loader = SkillLoader::new(Arc::clone(&registry), resolver);
        (registry, loader)
    }

    #[test]
    fn loads_flat_and_nested_skills() {
        let dir = std::env::temp_dir().join(format!("skills-loader-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        write_skill(&dir, "git", &basic("git", &["commit changes"], &[]));
        std::fs::write(dir.join("skill.json"), basic("flat", &["flat action"], &[])).unwrap();

        let (registry, loader) = loader(&["terminal.execute"]);
        let report = loader.load_bundled(&dir).unwrap();

        assert_eq!(report.scanned, 2);
        assert_eq!(report.loaded, 2);
        assert!(report.ok());
        assert_eq!(registry.all().len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn orders_dependencies_and_fails_missing() {
        let dir = std::env::temp_dir().join(format!("skills-deps-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        write_skill(&dir, "developer", &basic("developer", &["code"], &[]));
        write_skill(
            &dir,
            "docker",
            &basic("docker", &["compose up"], &["developer"]),
        );
        write_skill(&dir, "orphan", &basic("orphan", &["x"], &["missing-dep"]));

        let (registry, loader) = loader(&["terminal.execute"]);
        let report = loader.load_bundled(&dir).unwrap();

        assert_eq!(report.loaded, 2);
        assert_eq!(report.failed.len(), 1);
        assert!(report.failed[0].error.contains("missing-dep"));
        assert!(registry.get("docker").is_some());
        assert!(registry.get("orphan").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_dependency_cycles() {
        let dir = std::env::temp_dir().join(format!("skills-cycle-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        write_skill(&dir, "a", &basic("a", &["a action"], &["b"]));
        write_skill(&dir, "b", &basic("b", &["b action"], &["a"]));

        let (_, loader) = loader(&["terminal.execute"]);
        let report = loader.load_bundled(&dir).unwrap();

        assert_eq!(report.loaded, 0);
        assert_eq!(report.failed.len(), 2);
        assert!(report.failed[0].error.contains("cycle"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn soft_declares_unknown_capabilities() {
        let dir = std::env::temp_dir().join(format!("skills-soft-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        write_skill(&dir, "future", &basic("future", &["future thing"], &[]));

        // `terminal.execute` is NOT in the known set → soft-declared.
        let (registry, loader) = loader(&[]);
        let report = loader.load_bundled(&dir).unwrap();
        assert!(report.ok());
        assert_eq!(report.loaded, 1);

        let skill = registry.get("future").unwrap();
        assert!(!skill.is_usable()); // no resolved capabilities yet
        assert_eq!(skill.soft_declared_capabilities(), vec!["terminal.execute"]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
