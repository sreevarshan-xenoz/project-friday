//! Unit tests for the Recovery Graph.

use std::collections::HashSet;

use friday_recovery::taxonomy::{FailureCategory, RecoveryAction};
use friday_recovery::{default_graph, RecoveryGraph};

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
fn filesystem_write_recovers_by_retry_when_provider_down() {
    let g = default_graph();
    let avail = caps(&[]); // nothing available; retry still resolves locally
    let r = g.recover("filesystem.create_file", FailureCategory::Provider, &avail);
    assert!(r.recovered);
    assert_eq!(r.category, FailureCategory::Provider);
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
fn inference_recovers_by_retry_under_provider_failure() {
    let g = default_graph();
    let r = g.recover("inference.complete", FailureCategory::Provider, &caps(&[]));
    assert!(r.recovered);
    assert_eq!(r.confidence, 0.70);
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
