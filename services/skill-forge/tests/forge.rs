//! Skill Forge (M7) integration tests — the platform turn proven behaviorally:
//!
//! * every template scaffolds a manifest that validates AND resolves against
//!   the real native registry (canonical capability ids),
//! * `create-skill` produces a directory `validate` accepts,
//! * `validate` accepts the real bundled skills and rejects corrupted
//!   manifests,
//! * `publish` installs into a user skill dir and the real loader loads it.

use std::path::Path;
use std::sync::Arc;

use friday_skill_forge::templates::TemplateKind;
use friday_skill_forge::{create_skill, native_registry, publish, validate_path, NativeResolver};
use friday_skills::{CapabilityResolver, SkillLoader, SkillRegistry, SkillSource};

fn temp_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("forge-{name}-{}", uuid::Uuid::new_v4()))
}

#[test]
fn every_template_manifests_is_valid_and_canonical() {
    let registry = Arc::new(native_registry());
    for kind in TemplateKind::all() {
        let value = friday_skill_forge::templates::manifest(kind, "testskill", "Test Skill");
        let manifest: friday_skills::SkillManifest =
            serde_json::from_value(value).expect("template manifest must parse");
        assert!(
            manifest.validate().is_ok(),
            "{:?} template failed validation: {manifest:?}",
            kind
        );
        // Canonical: every declared capability must resolve against the native
        // registry the runtime actually gates on.
        for cap in &manifest.capabilities {
            assert!(
                registry.resolve(cap).is_some(),
                "{:?} template declares non-canonical capability '{cap}'",
                kind
            );
        }
    }
}

#[test]
fn create_skill_scaffolds_a_valid_skill() {
    let dir = temp_dir("scaffold");
    let report = create_skill("minecraft", TemplateKind::Minecraft, &dir).unwrap();
    assert!(report.dir.join("skill.json").is_file());
    assert!(report.dir.join("README.md").is_file());
    assert!(report.files.iter().any(|f| f.ends_with("skill.json")));

    let manifest: friday_skills::SkillManifest =
        serde_json::from_str(&std::fs::read_to_string(report.dir.join("skill.json")).unwrap())
            .unwrap();
    assert_eq!(manifest.id, "minecraft");
    assert!(manifest.validate().is_ok());
    // The M8 demo template ships memory-aware.
    assert_eq!(manifest.memory.writes, vec!["minecraft.server_state"]);

    let validation = validate_path(&report.dir).unwrap();
    assert!(validation.passed(), "{validation:?}");
    assert_eq!(validation.skills.len(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn validate_accepts_real_bundled_skills() {
    let bundled = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/bundled");
    let report = validate_path(&bundled).unwrap();
    assert!(report.passed(), "{report:?}");
    assert!(report.skills.len() >= 8, "skills: {:?}", report.skills);
    let git = report.skills.iter().find(|s| s.skill_id == "git").unwrap();
    assert!(git.passed);
    assert!(git.checks.iter().any(|c| c.name == "capabilities" && c.ok));
}

#[test]
fn validate_rejects_corrupted_manifests() {
    let dir = temp_dir("corrupt");
    std::fs::create_dir_all(dir.join("broken")).unwrap();
    std::fs::write(dir.join("broken/skill.json"), "{not json").unwrap();

    let report = validate_path(&dir).unwrap();
    assert!(!report.passed());
    assert!(!report.load_failures.is_empty());

    // An unreadable path is an error, not a passing empty report.
    assert!(validate_path(&dir.join("nope")).is_err());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn publish_installs_into_skill_dir_and_the_loader_loads_it() {
    let src = temp_dir("pub-src");
    create_skill("docker", TemplateKind::Docker, &src).unwrap();
    let home = temp_dir("pub-home");

    let report = publish(&src.join("docker"), &home, false).unwrap();
    assert_eq!(report.skill_id, "docker");
    assert!(report.dest.join("skill.json").is_file());
    assert!(report.dest.join("README.md").is_file());

    // The installed skill loads through the REAL loader and resolves.
    let registry = Arc::new(native_registry());
    let resolver: Arc<dyn CapabilityResolver> = Arc::new(NativeResolver(Arc::clone(&registry)));
    let skills = Arc::new(SkillRegistry::new());
    let loader = SkillLoader::new(Arc::clone(&skills), Arc::clone(&resolver));
    let load = loader.load_dir(&home, SkillSource::User).unwrap();
    assert_eq!(load.loaded, 1, "load report: {load:?}");
    let docker = skills.get("docker").unwrap();
    assert!(
        docker.is_usable(),
        "installed skill must resolve capabilities"
    );

    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn publish_requires_force_to_overwrite() {
    let src = temp_dir("pub-force");
    create_skill("server", TemplateKind::Server, &src).unwrap();
    let home = temp_dir("pub-home");

    assert!(publish(&src.join("server"), &home, false).is_ok());
    assert!(
        publish(&src.join("server"), &home, false).is_err(),
        "second install without --force must be refused"
    );
    assert!(publish(&src.join("server"), &home, true).is_ok());

    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn publish_refuses_broken_skills() {
    let src = temp_dir("pub-broken");
    std::fs::create_dir_all(src.join("broken")).unwrap();
    std::fs::write(src.join("broken/skill.json"), "{not json").unwrap();
    let home = temp_dir("pub-home");

    let err = publish(&src.join("broken"), &home, false).unwrap_err();
    assert!(
        err.contains("refusing to publish"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_dir_all(&home);
}
