//! Capability discovery + ranking unit tests (M6.2 discovery).
//!
//! These pin down the behaviour of [`CapabilityRegistry::search`]: ranking order,
//! `limit` enforcement, risk penalty, deterministic tie-breaking, and the
//! exclusion of unknown/unavailable capabilities. Every downstream planner and
//! the LLM prompt builder depend on this being correct and predictable, so these
//! are treated as contract tests.

use friday_planner::execution::capability_registry::{
    CapabilityRegistry, CapabilitySource, CapabilitySpec, Risk,
};

fn spec(id: &str, name: &str, desc: &str, risk: Risk) -> CapabilitySpec {
    CapabilitySpec::native(id, name, desc, "adapter", "action", risk)
}

#[test]
fn ranks_most_relevant_first() {
    let reg = CapabilityRegistry::new();
    // "git push" is highly relevant to a "push code" goal.
    reg.register(spec(
        "git.push",
        "Push",
        "Push commits to remote",
        Risk::High,
    ));
    reg.register(spec(
        "filesystem.read_file",
        "Read",
        "Read a file",
        Risk::Safe,
    ));
    reg.register(spec(
        "terminal.run",
        "Run",
        "Run a shell command",
        Risk::Medium,
    ));

    let results = reg.search("push my code to the remote", 10);
    assert!(!results.is_empty(), "should surface something");
    // git.push must be the top result by relevance despite its high-risk penalty.
    assert_eq!(results[0].spec.id, "git.push");
    // Scores must be non-increasing.
    for w in results.windows(2) {
        assert!(
            w[0].score >= w[1].score,
            "results must be sorted desc by score"
        );
    }
}

#[test]
fn limit_is_enforced() {
    let reg = CapabilityRegistry::new();
    for i in 0..20 {
        reg.register(spec(
            &format!("tool.{i}"),
            &format!("Tool {i}"),
            "generic capability for testing discovery",
            Risk::Safe,
        ));
    }
    let results = reg.search("tool capability testing discovery generic", 5);
    assert_eq!(results.len(), 5, "search must respect the limit");

    let all = reg.search("tool capability testing discovery generic", 100);
    assert_eq!(all.len(), 20, "with a large limit everything is returned");
}

#[test]
fn risk_penalty_lowers_high_risk_capabilities() {
    let reg = CapabilityRegistry::new();
    // Two caps with *identical* textual relevance (same name/description, goal
    // matches both equally) but different risk. The only thing that can reorder
    // them is the risk penalty.
    reg.register(spec(
        "cap.safe",
        "Backup",
        "backup the database",
        Risk::Safe,
    ));
    reg.register(spec(
        "cap.high",
        "Backup",
        "backup the database",
        Risk::High,
    ));

    let results = reg.search("backup the database", 10);
    // Safe must outrank the high-risk twin because text relevance is identical.
    assert_eq!(
        results[0].spec.id, "cap.safe",
        "with equal relevance, the safe capability must rank above the high-risk one"
    );
    assert!(results[0].score > results[1].score);

    // The high-risk twin's score must equal the safe score minus the documented
    // 0.35 penalty (both have relevance 1.0, no substring boost).
    let safe = results
        .iter()
        .find(|r| r.spec.id == "cap.safe")
        .unwrap()
        .score;
    let high = results
        .iter()
        .find(|r| r.spec.id == "cap.high")
        .unwrap()
        .score;
    assert!(
        (high - (safe - 0.35)).abs() < 1e-9,
        "high-risk score should carry the 0.35 penalty"
    );
}

#[test]
fn tie_break_is_deterministic_and_stable() {
    let reg = CapabilityRegistry::new();
    // Identical relevance (no textual overlap at all) -> both get the same
    // base score and differ only by risk; ties must not reorder across runs.
    reg.register(spec("a.zzz", "Zzz", "zzz capability", Risk::Safe));
    reg.register(spec("b.zzz", "Zzz", "zzz capability", Risk::Safe));
    reg.register(spec("c.zzz", "Zzz", "zzz capability", Risk::Safe));

    let first = reg.search("qqq unrelated words here", 10);
    let second = reg.search("qqq unrelated words here", 10);
    let ids_first: Vec<&str> = first.iter().map(|r| r.spec.id.as_str()).collect();
    let ids_second: Vec<&str> = second.iter().map(|r| r.spec.id.as_str()).collect();
    assert_eq!(ids_first, ids_second, "tie-breaking must be deterministic");
}

#[test]
fn unknown_or_empty_goal_still_returns_something_or_empty() {
    let reg = CapabilityRegistry::new();
    reg.register(spec("git.push", "Push", "push commits", Risk::High));
    reg.register(spec("fs.read", "Read", "read file", Risk::Safe));

    // No overlap with registry vocabulary -> all scores low but still returned
    // (capped to limit). The contract is: never panic, never invent caps.
    let results = reg.search("make me a sandwich with pickles", 2);
    assert!(results.len() <= 2);
    for r in &results {
        assert!(
            reg.resolve(&r.spec.id).is_some(),
            "every result must be a real registered cap"
        );
    }

    // An empty registry returns nothing.
    let empty = CapabilityRegistry::new();
    assert!(empty.search("anything", 5).is_empty());
}

#[test]
fn mcp_capabilities_appear_automatically() {
    // Discovery must not special-case native vs MCP — same `register` call.
    let reg = CapabilityRegistry::new();
    let mut mcp = spec(
        "mcp:vscode.open_file",
        "Open",
        "open a file in vscode",
        Risk::Safe,
    );
    mcp.source = CapabilitySource::Mcp("vscode".into());
    reg.register(mcp);
    reg.register(spec("fs.read", "Read", "read file", Risk::Safe));

    let results = reg.search("open a file in vscode editor", 10);
    assert!(
        results.iter().any(|r| r.spec.id == "mcp:vscode.open_file"),
        "MCP-registered capability must surface through discovery"
    );
}
