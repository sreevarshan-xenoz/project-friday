//! Unit tests for the Recovery Graph.

use std::collections::HashSet;

use friday_recovery::taxonomy::{FailureCategory, RecoveryAction};
use friday_recovery::{default_graph, ConfidenceModel, RecoveryGraph};

fn caps(set: &[&str]) -> HashSet<String> {
    set.iter().map(|s| s.to_string()).collect()
}

#[test]
fn browser_navigate_recovers_via_windows_when_browser_down() {
    let g = default_graph();
    // Windows is alive (chaos keeps it up); browser is killed.
    let avail = caps(&["windows.navigate", "filesystem.create_file"]);
    let r = g.recover("browser.navigate", FailureCategory::Provider, &avail);
    assert!(
        r.recovered,
        "browser.navigate should recover via windows.navigate"
    );
    assert!(matches!(
        r.chosen,
        RecoveryAction::UseAlternateCapability { .. }
    ));
    assert!(r.confidence >= 0.5);
}

#[test]
fn filesystem_write_does_not_recover_by_retry_when_provider_down() {
    let g = default_graph();
    // A dead provider is NOT healed by retrying it.
    let avail = caps(&[]);
    let r = g.recover("filesystem.create_file", FailureCategory::Provider, &avail);
    assert!(
        !r.recovered,
        "retrying a dead provider must not count as recovery"
    );
}

#[test]
fn retry_resolves_for_transient_timeout_but_not_permission() {
    let g = default_graph();
    let avail = caps(&[]);
    let timeout = g.recover("filesystem.create_file", FailureCategory::Timeout, &avail);
    assert!(
        timeout.recovered,
        "Retry should resolve a transient Timeout"
    );

    let perm = g.recover(
        "filesystem.create_file",
        FailureCategory::Permission,
        &avail,
    );
    assert!(
        !perm.recovered,
        "Retry must NOT resolve a Permission failure"
    );
}

#[test]
fn restart_does_not_resolve_provider_offline() {
    let g = default_graph();
    let r = g.recover("terminal.run", FailureCategory::Provider, &caps(&[]));
    assert!(
        !r.recovered,
        "RestartService must not heal a Provider-offline failure"
    );
}

#[test]
fn inference_recovers_under_capability_via_alternate_provider() {
    let g = default_graph();
    // Under a Capability failure (e.g. malformed output) the plan reroutes to
    // another provider exposing the same capability.
    let r = g.recover(
        "inference.complete",
        FailureCategory::Capability,
        &caps(&["inference.complete"]),
    );
    assert!(r.recovered);
    assert!(matches!(r.chosen, RecoveryAction::UseProvider { .. }));
}

#[test]
fn unrecoverable_when_no_policy_and_no_alternate() {
    let g = RecoveryGraph::new();
    let avail = caps(&[]);
    let r = g.recover("git.commit", FailureCategory::Provider, &avail);
    assert!(!r.recovered);
    assert!(r.needs_approval);
}

#[test]
fn graph_has_policies_for_all_core_actions() {
    let g = default_graph();
    for a in [
        "browser.navigate",
        "filesystem.create_file",
        "terminal.run",
        "git.commit",
        "inference.complete",
        "windows.open",
    ] {
        assert!(g.policy_for(a).is_some(), "missing policy for {a}");
    }
}

#[test]
fn confidence_model_learns_from_outcomes() {
    let mut m = ConfidenceModel::new();
    let base = 0.70;
    // 40 observed successes out of 40 trials for a Retry step.
    for _ in 0..40 {
        m.record(
            "filesystem.create_file",
            FailureCategory::Timeout,
            &RecoveryAction::Retry { attempts: 2 },
            true,
        );
    }
    let learned = m.effective(
        base,
        "filesystem.create_file",
        FailureCategory::Timeout,
        &RecoveryAction::Retry { attempts: 2 },
    );
    // Converges from the 0.70 prior toward ~1.0 observed.
    assert!(
        learned > 0.95,
        "learned confidence should approach observed ~1.0, got {learned}"
    );
}
