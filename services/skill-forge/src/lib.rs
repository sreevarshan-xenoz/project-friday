//! Skill Forge (M7) — the creator/validator/publisher that turns FRIDAY into
//! a platform. Third parties build skills without touching the core:
//!
//! ```text
//! friday create-skill minecraft     # scaffold a skill directory
//! friday validate                   # qualify it against the real registry
//! friday publish                    # install into ~/.friday/skills
//! ```
//!
//! The command logic lives here (testable without spawning the binary); the
//! `friday` binary in `main.rs` is a thin argv shim over it.

pub mod templates;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use friday_planner::execution::capability_registry::CapabilityRegistry;
use friday_planner::execution::native_caps;
use friday_skills::{CapabilityResolver, SkillLoader, SkillManifest, SkillRegistry, SkillSource};

use templates::TemplateKind;

/// The built-in native capability registry (filesystem / terminal / git) —
/// the same registry the runtime supervisor gates on. `validate` resolves
/// declared capabilities against this, so a skill that will fail at runtime
/// is caught at the door.
pub fn native_registry() -> CapabilityRegistry {
    let reg = CapabilityRegistry::new();
    native_caps::register_native_builtins(&reg);
    reg
}

/// Resolver over the native registry, mapping `capability → resolved` exactly
/// like the runtime's `PlannerRegistryResolver`.
pub struct NativeResolver(pub Arc<CapabilityRegistry>);

impl CapabilityResolver for NativeResolver {
    fn exists(&self, capability: &str) -> bool {
        self.0.resolve(capability).is_some()
    }
}

/// Result of [`create_skill`].
#[derive(Debug)]
pub struct ScaffoldReport {
    pub skill_id: String,
    pub dir: PathBuf,
    pub files: Vec<PathBuf>,
}

/// Scaffold a new skill: writes `<dir>/<id>/skill.json` + `README.md` from a
/// template. Fails on an invalid id (slug rules) or an unwritable dir.
pub fn create_skill(
    id: &str,
    template: TemplateKind,
    dir: &Path,
) -> Result<ScaffoldReport, String> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "'{id}' is not a valid skill id (use only a-z, 0-9, '-', '_')"
        ));
    }
    let name = titleize(id);
    let manifest = templates::manifest(template, id, &name);
    let skill_dir = dir.join(id);
    std::fs::create_dir_all(&skill_dir)
        .map_err(|e| format!("cannot create {}: {e}", skill_dir.display()))?;

    let manifest_path = skill_dir.join("skill.json");
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(&manifest_path, format!("{json}\n"))
        .map_err(|e| format!("cannot write {}: {e}", manifest_path.display()))?;

    let readme = templates::readme(id, &name, &manifest);
    let readme_path = skill_dir.join("README.md");
    std::fs::write(&readme_path, readme)
        .map_err(|e| format!("cannot write {}: {e}", readme_path.display()))?;

    Ok(ScaffoldReport {
        skill_id: id.to_string(),
        dir: skill_dir,
        files: vec![manifest_path, readme_path],
    })
}

/// One qualification check rendered for the CLI.
#[derive(Debug)]
pub struct CheckLine {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

/// The qualification verdict for one skill.
#[derive(Debug)]
pub struct SkillValidation {
    pub skill_id: String,
    pub version: String,
    pub passed: bool,
    pub checks: Vec<CheckLine>,
}

/// The full `friday validate` outcome for a path.
#[derive(Debug)]
pub struct ValidationReport {
    pub source: PathBuf,
    /// Every skill that loaded and was qualified.
    pub skills: Vec<SkillValidation>,
    /// Manifests that failed to even load (parse/validation errors), as
    /// `path: reason` strings.
    pub load_failures: Vec<String>,
}

impl ValidationReport {
    /// Passed = every manifest loaded AND every loaded skill qualified.
    pub fn passed(&self) -> bool {
        self.load_failures.is_empty() && self.skills.iter().all(|s| s.passed)
    }
}

/// Validate every skill under `path` against the native registry. `path` may
/// be a skill directory (`<id>/` or `<id>/skill.json`), or a parent
/// directory containing many skills (like `skills/bundled/`).
pub fn validate_path(path: &Path) -> Result<ValidationReport, String> {
    let dir = if path.is_file() && path.file_name().is_some_and(|n| n == "skill.json") {
        path.parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| format!("cannot resolve the directory of {}", path.display()))?
    } else {
        path.to_path_buf()
    };
    if !dir.is_dir() {
        return Err(format!("'{}' is not a directory", dir.display()));
    }

    let registry = Arc::new(native_registry());
    let resolver: Arc<dyn CapabilityResolver> = Arc::new(NativeResolver(Arc::clone(&registry)));
    let skills = Arc::new(SkillRegistry::new());
    let loader = SkillLoader::new(Arc::clone(&skills), Arc::clone(&resolver));

    let mut report = ValidationReport {
        source: dir.clone(),
        skills: Vec::new(),
        load_failures: Vec::new(),
    };

    match loader.load_dir(&dir, SkillSource::User) {
        Ok(load) => {
            for failure in &load.failed {
                report
                    .load_failures
                    .push(format!("{}: {}", failure.path, failure.error));
            }
            for skill in skills.all() {
                let q = friday_skills::qualify(&skills, &*resolver, skill.id());
                report.skills.push(SkillValidation {
                    skill_id: skill.id().to_string(),
                    version: skill.version().to_string(),
                    passed: q.passed,
                    checks: q
                        .checks
                        .iter()
                        .map(|c| CheckLine {
                            name: c.check.to_string(),
                            ok: c.status != friday_skills::CheckStatus::Fail,
                            detail: c.detail.clone(),
                        })
                        .collect(),
                });
            }
        }
        Err(e) => report.load_failures.push(format!("{}: {e}", dir.display())),
    }
    Ok(report)
}

/// Result of [`publish`].
#[derive(Debug)]
pub struct PublishReport {
    pub skill_id: String,
    pub source: PathBuf,
    pub dest: PathBuf,
    pub files_copied: usize,
}

/// The default user skill install root: `FRIDAY_SKILLS_DIR` if set, else
/// `~/.friday/skills` (HOME, or USERPROFILE on Windows).
pub fn default_skill_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("FRIDAY_SKILLS_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    home.join(".friday").join("skills")
}

/// Publish (install) a skill into `dest_root/<id>/`. The skill is validated
/// first — a broken skill is never installed. `source` may be a skill
/// directory or a `skill.json` file; overwrites require `force`.
pub fn publish(source: &Path, dest_root: &Path, force: bool) -> Result<PublishReport, String> {
    let skill_dir = resolve_skill_dir(source)?;

    // Never publish a broken skill.
    let report = validate_path(&skill_dir)?;
    if !report.passed() {
        return Err(format!(
            "refusing to publish — validation failed: {} load error(s), {} skill(s) with failures",
            report.load_failures.len(),
            report.skills.iter().filter(|s| !s.passed).count()
        ));
    }

    let manifest: SkillManifest = serde_json::from_str(
        &std::fs::read_to_string(skill_dir.join("skill.json")).map_err(|e| {
            format!(
                "cannot read {}: {e}",
                skill_dir.join("skill.json").display()
            )
        })?,
    )
    .map_err(|e| format!("invalid skill.json: {e}"))?;
    let id = manifest.id;

    let dest = dest_root.join(&id);
    if dest.join("skill.json").exists() && !force {
        return Err(format!(
            "'{id}' is already installed at {} (use --force to overwrite)",
            dest.display()
        ));
    }

    let mut copied = 0usize;
    copy_dir(&skill_dir, &dest, &mut copied)
        .map_err(|e| format!("failed to install into {}: {e}", dest.display()))?;

    Ok(PublishReport {
        skill_id: id,
        source: skill_dir,
        dest,
        files_copied: copied,
    })
}

/// Resolve a CLI argument (a skill id, a directory, or a `skill.json`) to the
/// skill's directory: `<cwd>/<id>`, the path itself, or the file's parent.
pub fn resolve_skill_dir(source: &Path) -> Result<PathBuf, String> {
    if source.is_file() {
        return source
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| format!("cannot resolve the directory of {}", source.display()));
    }
    if source.join("skill.json").is_file() {
        return Ok(source.to_path_buf());
    }
    // An id: `<cwd>/<id>`.
    let by_id = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join(source);
    if by_id.join("skill.json").is_file() {
        return Ok(by_id);
    }
    Err(format!(
        "'{}' is not a skill directory (no skill.json found)",
        source.display()
    ))
}

fn copy_dir(src: &Path, dst: &Path, copied: &mut usize) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target, copied)?;
        } else {
            std::fs::copy(&path, &target)?;
            *copied += 1;
        }
    }
    Ok(())
}

fn titleize(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    let mut capitalize = true;
    for c in id.chars() {
        if c == '-' || c == '_' {
            out.push(' ');
            capitalize = true;
        } else if capitalize {
            out.extend(c.to_uppercase());
            capitalize = false;
        } else {
            out.push(c);
        }
    }
    out
}
