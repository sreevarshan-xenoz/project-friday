//! Capability Intelligence tests: ranking, registry queries, health/monitor,
//! manifest discovery, graph, and integration with the real services.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use friday_capabilities::graph::CapabilityGraph;
use friday_capabilities::health::{HealthProbe, Monitor};
use friday_capabilities::manifest::ServiceManifest;
use friday_capabilities::model::{Capability, Platform};
use friday_capabilities::{score, CapabilityRegistry, Health};

#[test]
fn ranked_providers_picks_best_by_score() {
    let reg = CapabilityRegistry::new();
    reg.register(
        "browser",
        Capability::new("open_url", "Browser Open URL", "Browser")
            .with_latency(20)
            .with_reliability(0.99)
            .with_confidence(1.0),
    );
    reg.register(
        "windows",
        Capability::new("open_url", "Windows Open URL", "Windows")
            .with_latency(50)
            .with_reliability(0.835)
            .with_confidence(1.0),
    );

    let ranked = reg.ranked_providers_for("open_url");
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].0, "browser", "planner should pick Browser first");
    assert!(
        (score(&ranked[0].1) - 98.0).abs() < 0.01,
        "browser score ~98"
    );
    assert!(
        (score(&ranked[1].1) - 81.0).abs() < 0.01,
        "windows score ~81"
    );
}

#[test]
fn unavailable_providers_are_excluded_from_ranking() {
    let reg = CapabilityRegistry::new();
    reg.register(
        "browser",
        Capability::new("open_url", "Browser", "Browser")
            .with_latency(20)
            .with_reliability(0.99)
            .mark_unavailable(),
    );
    reg.register(
        "windows",
        Capability::new("open_url", "Windows", "Windows")
            .with_latency(50)
            .with_reliability(0.835),
    );
    let ranked = reg.ranked_providers_for("open_url");
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].0, "windows");
    assert!(reg.available("open_url"));
}

#[test]
fn graph_groups_capabilities_by_group() {
    let caps = vec![
        Capability::new("filesystem", "fs", "Files"),
        Capability::new("git", "g", "Git"),
        Capability::new("windows", "w", "Windows"),
    ];
    let g = CapabilityGraph::from_capabilities(&caps);
    assert_eq!(g.roots.len(), 3);
    assert!(g.roots.iter().any(|n| n.name == "Windows"));
    let json = g.to_json();
    assert!(json.contains("\"Files\""));
}

struct AlwaysOfflineProbe;

#[async_trait]
impl HealthProbe for AlwaysOfflineProbe {
    async fn probe(&self, _provider: &str, _cap: &Capability) -> Health {
        Health::Offline
    }
}

#[tokio::test]
async fn monitor_marks_unavailable_and_fires_recovery() {
    let reg = Arc::new(CapabilityRegistry::new());
    reg.register(
        "git",
        Capability::new("git", "Git Service", "Git")
            .with_latency(30)
            .with_reliability(0.98),
    );
    assert!(reg.available("git"));

    let recovered = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = recovered.clone();
    let monitor = Monitor::new(
        reg.clone(),
        Arc::new(AlwaysOfflineProbe),
        Duration::from_secs(60),
    )
    .with_recovery(Arc::new(move |_p, _c| {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }));

    monitor.tick().await;

    assert!(
        !reg.available("git"),
        "offline capability must be unavailable"
    );
    let (_, cap) = reg.providers_for("git")[0].clone();
    assert_eq!(cap.health, Health::Offline);
    assert!(
        recovered.load(std::sync::atomic::Ordering::SeqCst),
        "recovery hook must fire"
    );
}

#[test]
fn manifest_scan_validate_and_register() {
    let dir = std::env::temp_dir().join(format!("friday_cap_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("demo")).unwrap();
    let manifest = ServiceManifest {
        service: "demo".into(),
        version: "1.0".into(),
        platform: "Any".into(),
        capabilities: vec![
            Capability::new("demo.alpha", "Alpha", "Demo").with_actions(&["run"]),
            Capability::new("demo.beta", "Beta", "Demo").with_actions(&["run"]),
        ],
    };
    let text = serde_json::to_string_pretty(&manifest).unwrap();
    std::fs::write(dir.join("demo").join("manifest.json"), text).unwrap();

    let manifests = ServiceManifest::load_dir(&dir).unwrap();
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0].service, "demo");
    assert_eq!(manifests[0].platform().unwrap(), Platform::Any);

    let reg = CapabilityRegistry::new();
    manifests[0].register_into(&reg);
    assert!(reg.available("demo.alpha"));
    assert!(reg.available("demo.beta"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn manifest_rejects_missing_actions() {
    let bad = ServiceManifest {
        service: "bad".into(),
        version: "1.0".into(),
        platform: "Any".into(),
        capabilities: vec![Capability::new("bad.x", "X", "Bad")], // no actions
    };
    assert!(bad.validate().is_err());
}

#[test]
fn real_services_publish_capabilities() {
    let reg = CapabilityRegistry::new();
    for c in friday_actions::adapters::all_capabilities() {
        reg.register("friday-actions", c);
    }
    for c in friday_windows::capabilities() {
        reg.register("windows-host", c);
    }
    assert!(reg.available("filesystem"));
    assert!(reg.available("git"));
    assert!(reg.available("windows.open"));
    let g = reg.graph();
    assert!(g.roots.iter().any(|n| n.name == "Windows"));
    assert!(g.roots.iter().any(|n| n.name == "Files"));
}
