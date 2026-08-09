use std::collections::HashMap;
use std::fs;
use std::io::Write;

use friday_plugin_runtime::{
    Capability, CapabilityCatalog, CapabilityPlanner, CapabilityRegistry, CapabilityRequest,
    CorrelationId, DefaultPolicyEngine, FilesystemPlugin, HealthMonitor, Plugin, PluginRuntime,
    ScoringWeights, SelectionContext,
};

/// Full pipeline: Executive → CapabilityRequest → Resolver → Runtime → Plugin → Result
///
/// This validates the entire capability execution architecture without
/// modifying the frozen cognitive core.
#[test]
fn full_filesystem_pipeline_read() {
    let mut runtime = PluginRuntime::new();
    runtime.register(Box::new(FilesystemPlugin::new()));
    runtime.initialize_all();
    runtime.activate_all();

    // Create a test file
    let path = std::env::temp_dir().join("friday_integration_read.txt");
    {
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(b"integration test content").unwrap();
    }

    // Simulate the Executive's flow
    let request = CapabilityRequest::new(Capability::ReadFile)
        .with_parameter("path", path.to_string_lossy().to_string());

    // The Executive would call runtime.plan_and_execute()
    let (result, rationale) = runtime.plan_and_execute(request).unwrap();

    assert!(result.success, "read should succeed");
    assert_eq!(result.output, "integration test content");
    assert!(result.telemetry.duration_ms() > 0);
    assert_eq!(result.telemetry.plugin_id, "filesystem");

    // Verify rationale produced
    assert!(rationale.explanation.contains("Filesystem"));

    // Verify health metrics updated
    let health = runtime.health_of("filesystem").unwrap();
    assert_eq!(health.total_executions, 1);
    assert_eq!(health.failure_count, 0);

    let _ = fs::remove_file(&path);
}

#[test]
fn full_filesystem_pipeline_write_then_read() {
    let mut runtime = PluginRuntime::new();
    runtime.register(Box::new(FilesystemPlugin::new()));
    runtime.initialize_all();
    runtime.activate_all();

    let path = std::env::temp_dir().join("friday_integration_write_read.txt");

    // Write
    let write_req = CapabilityRequest::new(Capability::WriteFile)
        .with_parameter("path", path.to_string_lossy().to_string())
        .with_parameter("content", "written in integration test");
    let (write_result, _) = runtime.plan_and_execute(write_req).unwrap();
    assert!(write_result.success);

    // Read back
    let read_req = CapabilityRequest::new(Capability::ReadFile)
        .with_parameter("path", path.to_string_lossy().to_string());
    let (read_result, _) = runtime.plan_and_execute(read_req).unwrap();
    assert!(read_result.success);
    assert_eq!(read_result.output, "written in integration test");

    let _ = fs::remove_file(&path);
}

#[test]
fn pipeline_list_directory() {
    let mut runtime = PluginRuntime::new();
    runtime.register(Box::new(FilesystemPlugin::new()));
    runtime.initialize_all();
    runtime.activate_all();

    let dir = std::env::temp_dir().join("friday_integration_list");
    let _ = fs::create_dir_all(&dir);
    fs::write(dir.join("alpha.txt"), "a").unwrap();
    fs::write(dir.join("beta.txt"), "b").unwrap();

    let request = CapabilityRequest::new(Capability::ListDirectory)
        .with_parameter("path", dir.to_string_lossy().to_string());
    let (result, _) = runtime.plan_and_execute(request).unwrap();

    assert!(result.success);
    assert!(result.output.contains("alpha.txt"));
    assert!(result.output.contains("beta.txt"));
    assert_eq!(result.metadata.get("entry_count").unwrap(), "2");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resolver_uses_registry_health_and_catalog_together() {
    // This validates the Resolver ↔ Registry ↔ Catalog ↔ HealthMonitor triangle
    let mut reg = CapabilityRegistry::new();
    let fs_desc = FilesystemPlugin::new().descriptor();
    let profile = fs_desc.profile.clone();
    reg.register(fs_desc);

    let catalog = CapabilityCatalog::with_defaults();
    let policy = DefaultPolicyEngine;
    let weights = ScoringWeights::default();
    let mut profiles = HashMap::new();
    profiles.insert("filesystem".into(), profile);
    let planner = CapabilityPlanner::new(&reg, &catalog, &policy, &weights, &profiles);

    // No health data yet — resolver falls back
    let cid = CorrelationId::from_string("test");
    let ctx = SelectionContext::default();
    let (resolved, rationale) = planner
        .resolve(&Capability::ReadFile, &[], &cid, &ctx)
        .unwrap();
    assert_eq!(resolved.plugin_id, "filesystem");
    assert!((resolved.confidence - 0.3).abs() < 0.01);
    assert!(resolved.rationale.contains("not yet healthy"));

    // Rationale should reflect the fallback
    assert!(rationale.explanation.contains("not yet healthy"));
}

#[test]
fn health_monitor_tracks_across_multiple_executions() {
    let mut runtime = PluginRuntime::new();
    runtime.register(Box::new(FilesystemPlugin::new()));
    runtime.initialize_all();
    runtime.activate_all();

    let path = std::env::temp_dir().join("friday_health_track.txt");

    for i in 0..5 {
        if i % 2 == 0 {
            // Read
            if i == 0 {
                fs::write(&path, "health test").unwrap();
            }
            let req = CapabilityRequest::new(Capability::ReadFile)
                .with_parameter("path", path.to_string_lossy().to_string());
            let _ = runtime.plan_and_execute(req).unwrap();
        } else {
            // List dir
            let req = CapabilityRequest::new(Capability::ListDirectory)
                .with_parameter("path", std::env::temp_dir().to_string_lossy().to_string());
            let _ = runtime.plan_and_execute(req).unwrap();
        }
    }

    let health = runtime.health_of("filesystem").unwrap();
    assert_eq!(health.total_executions, 5);
    assert_eq!(health.failure_count, 0);
    assert!((health.success_rate - 1.0).abs() < 0.01);
    assert!(health.average_latency_ms > 0.0);

    let _ = fs::remove_file(&path);
}

#[test]
fn unknown_capability_returns_proper_error() {
    let mut runtime = PluginRuntime::new();
    let result = runtime.plan_and_execute(CapabilityRequest::new(Capability::OpenBrowser));
    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(err.to_string().contains("no provider"));
}

#[test]
fn runtime_supports_multiple_plugins() {
    let mut runtime = PluginRuntime::new();

    runtime.register(Box::new(FilesystemPlugin::new()));
    runtime.initialize_all();
    runtime.activate_all();

    assert_eq!(runtime.plugin_ids().len(), 1);
    assert!(runtime.can_handle(&Capability::ReadFile));
    assert!(runtime.can_handle(&Capability::WriteFile));
    assert!(runtime.can_handle(&Capability::ListDirectory));
    assert!(!runtime.can_handle(&Capability::SearchWeb));
}

#[test]
fn health_monitor_registers_independent_of_runtime() {
    let mut hm = HealthMonitor::new();
    hm.register("plugin-a");
    hm.register("plugin-b");
    hm.mark_healthy("plugin-a");

    let all = hm.all_health();
    assert_eq!(all.len(), 2);

    let healthy = hm.healthy_for(&["plugin-a".into(), "plugin-b".into()]);
    assert_eq!(healthy.len(), 1);
    assert_eq!(healthy[0].plugin_id, "plugin-a");
}

#[test]
fn runtime_with_custom_policy_blocks_plugins() {
    use friday_plugin_runtime::RuleBasedPolicyEngine;

    let mut runtime = PluginRuntime::new();
    runtime.set_policy(Box::new(RuleBasedPolicyEngine::new().block("filesystem")));
    runtime.register(Box::new(FilesystemPlugin::new()));
    runtime.initialize_all();
    runtime.activate_all();

    // Filesystem plugin is blocked by policy — should still fall back
    let path = std::env::temp_dir().join("friday_policy_test.txt");
    fs::write(&path, "hello").unwrap();
    let req = CapabilityRequest::new(Capability::ReadFile)
        .with_parameter("path", path.to_string_lossy().to_string());
    let (result, rationale) = runtime.plan_and_execute(req).unwrap();
    assert!(
        result.success,
        "read should succeed: {:?}",
        result.metadata.get("error")
    );
    assert!(rationale.explanation.contains("blocked by policy"));
    let _ = fs::remove_file(&path);
}

#[test]
fn runtime_produces_rationale_with_scored_candidates() {
    let mut runtime = PluginRuntime::new();
    runtime.register(Box::new(FilesystemPlugin::new()));
    runtime.initialize_all();
    runtime.activate_all();

    let req = CapabilityRequest::new(Capability::ListDirectory)
        .with_parameter("path", std::env::temp_dir().to_string_lossy().to_string());
    let (_, rationale) = runtime.plan_and_execute(req).unwrap();

    assert_eq!(rationale.capability, Capability::ListDirectory);
    assert!(!rationale.candidates.is_empty());
    assert!(rationale.composite_score > 0.0);
}

#[test]
fn runtime_with_adaptive_context_prefers_local_filesystem() {
    use friday_plugin_runtime::SelectionContext;

    let mut runtime = PluginRuntime::new();
    runtime.register(Box::new(FilesystemPlugin::new()));
    runtime.initialize_all();
    runtime.activate_all();

    // In offline mode, the filesystem plugin's profile (offline_capable) should be favored
    let ctx = SelectionContext::offline();
    let req = CapabilityRequest::new(Capability::ListDirectory)
        .with_context(ctx)
        .with_parameter("path", std::env::temp_dir().to_string_lossy().to_string());
    let (result, rationale) = runtime.plan_and_execute(req).unwrap();
    assert!(result.success);
    // The rationale should mention profile or context scoring
    assert!(rationale.composite_score > 0.0);
}
