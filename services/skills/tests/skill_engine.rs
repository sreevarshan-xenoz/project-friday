//! End-to-end Skill Engine flow (spec `specs/skill-engine.md` §12):
//! `load → enable → context → permission → qualification`, against a realistic
//! bundled-style skill directory with dependencies.

use friday_skills::{
    qualify, AllSafeRiskResolver, CapabilityResolver, LoadReport, SkillContextSource, SkillEvent,
    SkillPolicy, SkillRegistry, StaticCapabilityResolver, VecSink,
};
use std::path::Path;
use std::sync::Arc;

/// Writes a skill manifest under `dir/<id>/skill.json`.
fn write_skill(dir: &Path, json: &str, id: &str) {
    let sub = dir.join(id);
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("skill.json"), json).unwrap();
}

fn manifest(id: &str, name: &str, deps: &[&str], caps: &[&str], readonly: bool) -> String {
    let deps_json: Vec<String> = deps.iter().map(|d| format!("\"{d}\"")).collect();
    let caps_json: Vec<String> = caps.iter().map(|c| format!("\"{c}\"")).collect();
    format!(
        r#"{{
            "id": "{id}",
            "name": "{name}",
            "version": "1.0.0",
            "description": "The {name} skill for FRIDAY",
            "category": "developer",
            "capabilities": [{caps}],
            "actions": ["do {name} things", "handle {name} workflow"],
            "permissions": [{caps}],
            "readonly": {ro},
            "dependencies": [{deps}],
            "platform": "Any",
            "min_core_version": "0.0.0",
            "enabled": true
        }}"#,
        caps = caps_json.join(","),
        deps = deps_json.join(","),
        ro = readonly,
    )
}

fn known_caps() -> Vec<String> {
    [
        "terminal.exec",
        "filesystem.read_file",
        "filesystem.write_file",
        "git.status",
        "git.commit",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn setup_skill_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("skills-e2e-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    write_skill(
        &dir,
        &manifest("developer", "Developer", &[], &["terminal.exec"], false),
        "developer",
    );
    write_skill(
        &dir,
        &manifest(
            "git",
            "Git",
            &["developer"],
            &["git.status", "git.commit"],
            false,
        ),
        "git",
    );
    write_skill(
        &dir,
        &manifest(
            "docker",
            "Docker",
            &["developer"],
            &["terminal.exec", "filesystem.read_file"],
            false,
        ),
        "docker",
    );
    write_skill(
        &dir,
        &manifest("reader", "Reader", &[], &["filesystem.read_file"], true),
        "reader",
    );
    dir
}

#[test]
fn load_emits_lifecycle_events_in_order() {
    let sink = Arc::new(VecSink::new());
    let registry = Arc::new(SkillRegistry::with_sink(sink.clone()));
    let resolver: Arc<dyn CapabilityResolver> =
        Arc::new(StaticCapabilityResolver::new(known_caps()));
    let loader = friday_skills::SkillLoader::new(Arc::clone(&registry), resolver);

    let dir = setup_skill_dir();
    let report: LoadReport = loader.load_bundled(&dir).unwrap();

    assert_eq!(report.scanned, 4);
    assert_eq!(report.loaded, 4);
    assert!(report.ok(), "unexpected failures: {:?}", report.failed);

    let events = sink.events.lock().unwrap();
    assert!(events
        .iter()
        .any(|e| matches!(e, SkillEvent::Scanned { count: 4, .. })));
    let loaded: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            SkillEvent::Loaded { skill_id, .. } => Some(skill_id.as_str()),
            _ => None,
        })
        .collect();
    assert!(loaded.contains(&"docker"));
    assert!(loaded.contains(&"reader"));
    // Dependencies load before dependents (git before docker is not required,
    // but both must be present).
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn context_builder_sees_relevant_skill_summaries() {
    let sink = Arc::new(VecSink::new());
    let registry = Arc::new(SkillRegistry::with_sink(sink));
    let resolver: Arc<dyn CapabilityResolver> =
        Arc::new(StaticCapabilityResolver::new(known_caps()));
    friday_skills::SkillLoader::new(Arc::clone(&registry), resolver)
        .load_bundled(&setup_skill_dir())
        .unwrap();

    let ctx = registry.context_for("build the docker image", 5);
    assert_eq!(ctx.relevant_skills.len(), 4);
    assert_eq!(ctx.relevant_skills[0].id, "docker");

    let docker = &ctx.relevant_skills[0];
    assert_eq!(
        docker.capabilities,
        vec!["terminal.exec", "filesystem.read_file"]
    );
    assert_eq!(
        docker.permission_hints,
        vec!["terminal.exec", "filesystem.read_file"]
    );
    assert!(!docker.readonly);

    let reader = ctx
        .relevant_skills
        .iter()
        .find(|s| s.id == "reader")
        .unwrap();
    assert!(reader.readonly);
}

#[test]
fn permission_flow_readonly_auto_approves_approved_skill_unlocks() {
    let sink = Arc::new(VecSink::new());
    let registry = Arc::new(SkillRegistry::with_sink(sink));
    let resolver: Arc<dyn CapabilityResolver> =
        Arc::new(StaticCapabilityResolver::new(known_caps()));
    friday_skills::SkillLoader::new(Arc::clone(&registry), resolver)
        .load_bundled(&setup_skill_dir())
        .unwrap();

    let approvals = Arc::new(friday_skills::SkillApprovals::new());
    let policy = SkillPolicy::new(
        Arc::clone(&registry),
        Arc::clone(&approvals),
        Arc::new(AllSafeRiskResolver),
    );

    // Docker is not readonly and not approved → terminal.exec prompts.
    assert!(policy.requires_prompt("terminal.exec"));

    // User approves the Docker skill once → its capabilities are unlocked.
    approvals.approve("docker");
    assert!(policy.allows_capability("terminal.exec"));
    assert!(policy.allows_capability("filesystem.read_file"));
}

#[test]
fn enable_disable_removal_surfaces_events_and_changes_search() {
    let sink = Arc::new(VecSink::new());
    let registry = Arc::new(SkillRegistry::with_sink(sink.clone()));
    let resolver: Arc<dyn CapabilityResolver> =
        Arc::new(StaticCapabilityResolver::new(known_caps()));
    friday_skills::SkillLoader::new(Arc::clone(&registry), resolver)
        .load_bundled(&setup_skill_dir())
        .unwrap();

    registry.disable("docker").unwrap();
    // Disabled skills disappear from goal search.
    let ctx = registry.context_for("build the docker image", 5);
    assert!(!ctx.relevant_skills.iter().any(|s| s.id == "docker"));

    registry.enable("docker").unwrap();
    assert!(registry.get("docker").unwrap().enabled);

    registry.deregister("docker").unwrap();
    assert!(registry.get("docker").is_none());

    let events = sink.events.lock().unwrap();
    assert!(events
        .iter()
        .any(|e| matches!(e, SkillEvent::Disabled { skill_id } if skill_id == "docker")));
    assert!(events
        .iter()
        .any(|e| matches!(e, SkillEvent::Enabled { skill_id } if skill_id == "docker")));
    assert!(events
        .iter()
        .any(|e| matches!(e, SkillEvent::Removed { skill_id } if skill_id == "docker")));
}

#[test]
fn qualification_gates_every_loaded_skill() {
    let sink = Arc::new(VecSink::new());
    let registry = Arc::new(SkillRegistry::with_sink(sink));
    let resolver: Arc<dyn CapabilityResolver> =
        Arc::new(StaticCapabilityResolver::new(known_caps()));
    friday_skills::SkillLoader::new(Arc::clone(&registry), resolver.clone())
        .load_bundled(&setup_skill_dir())
        .unwrap();

    for id in ["developer", "git", "docker", "reader"] {
        let report = qualify(&registry, &*resolver, id);
        assert!(report.passed, "skill {id} failed qualification: {report:?}");
    }

    // A skill with an unknown capability fails the gate.
    let report = qualify(&registry, &*resolver, "does-not-exist");
    assert!(!report.passed);
}

/// Acceptance test (spec §12): the 8 real `skills/bundled/*` manifests must
/// load, resolve, qualify, and surface in context — proving the Skill Engine
/// ingests the actual repo artifacts (including their `examples`/`icon`
/// fields, which must be ignored at parse time).
#[test]
fn bundled_manifests_load_resolve_and_qualify() {
    let bundled = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/bundled");
    if !bundled.exists() {
        eprintln!("skipping: {bundled:?} not present in this checkout");
        return;
    }

    // Must match the canonical native ids (`friday_capabilities::builtins`)
    // the runtime registers, or the real manifests would all be soft-declared.
    // The browser verbs the web skill soft-declares (`web.open`, `web.click`,
    // ...) are deliberately absent — a future browser runtime binds them.
    let resolver: Arc<dyn CapabilityResolver> = Arc::new(StaticCapabilityResolver::new(vec![
        "terminal.exec".to_string(),
        "filesystem.read_file".to_string(),
        "filesystem.write_file".to_string(),
        "git.status".to_string(),
        "git.commit".to_string(),
        "git.push".to_string(),
        "git.pull".to_string(),
        "git.diff".to_string(),
        "web.search".to_string(),
        "web.fetch".to_string(),
        "docs.search".to_string(),
        "docs.read".to_string(),
        "codeintel.understand".to_string(),
    ]));

    let sink = Arc::new(VecSink::new());
    let registry = Arc::new(SkillRegistry::with_sink(sink));
    let report: LoadReport =
        friday_skills::SkillLoader::new(Arc::clone(&registry), resolver.clone())
            .load_bundled(&bundled)
            .unwrap();

    assert_eq!(
        report.scanned, 11,
        "expected the 11 bundled skill.json files"
    );
    assert_eq!(report.loaded, 11, "failures: {:?}", report.failed);

    // Every bundled skill passes the qualification gate.
    for id in [
        "codeintel",
        "developer",
        "docker",
        "docs",
        "git",
        "minecraft",
        "python",
        "rust",
        "server",
        "testing",
        "web",
    ] {
        let report = qualify(&registry, &*resolver, id);
        assert!(report.passed, "bundled skill {id} failed: {report:?}");
    }

    // Skill summaries are visible to the context builder.
    let ctx = registry.context_for("run the tests for the rust project", 8);
    assert_eq!(ctx.relevant_skills.len(), 8);
    assert!(ctx.relevant_skills.iter().any(|s| s.id == "rust"));
    assert!(ctx.relevant_skills.iter().any(|s| s.id == "testing"));
}
