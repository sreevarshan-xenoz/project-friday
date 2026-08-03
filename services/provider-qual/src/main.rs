//! `provider-qual` — Phase 1.5 Provider Qualification Harness.
//!
//! Usage:
//!   provider-qual                  # real mode: talks to the configured gateway
//!   provider-qual --mock           # in-process mock, no gateway required
//!   provider-qual --provider NAME  # omniroute (default) | ollama | openrouter | groq | cerebras | gemini | opengateway
//!   provider-qual --out DIR        # evidence pack directory
//!   provider-qual --scenario ID    # run only the scenario with this id
//!   provider-qual --replay PATH    # re-run the scenario recorded in PATH
//!
//! Pipeline: connect -> complete -> verify -> report (Evidence Packs + benchmark-
//! linked issues). No recovery, no provider evolution yet — just facts.
//!
//! Mock mode is unit testing. Real mode is qualification.

mod candidates;
mod corpus;
mod environment;
mod evidence;
mod harness;
mod issue;
mod mock;

use std::path::PathBuf;
use std::sync::Arc;

use friday_intelligence::{
    InferenceFabric, ModelCapability, ModelInfo, OmniRouteProvider, OmniRouteProviderV2,
    OpenAiProvider, Provider, ProviderV2, RoutingMode,
};

use candidates::{CandidateStore, GRADUATE_THRESHOLD};
use corpus::all;
use evidence::{aggregate, render, write_candidate, write_environment, write_issue, write_outcome};
use harness::{run_scenario, Mode, Target};
use mock::MockProviderV2;

fn model(provider: &str, id: &str) -> ModelInfo {
    ModelInfo {
        id: id.to_string(),
        family: id.to_string(),
        provider: provider.to_string(),
        latency_ms: 500,
        reliability: 1.0,
        capabilities: vec![
            ModelCapability::Json,
            ModelCapability::Tools,
            ModelCapability::LongContext,
        ],
        max_context: 200_000,
    }
}

/// Build the qualification target: a Fabric wrapping the single provider under
/// test. In mock mode a prompt-aware healthy provider is used; in real mode the
/// provider is constructed from the environment.
fn build_target(provider: &str, mock: bool) -> (Target, String, String) {
    if mock {
        let mut fabric = InferenceFabric::new();
        fabric.register_provider(Box::new(mock::MockProvider::healthy(provider.to_string())));
        fabric.register_model(model(provider, "auto"));
        fabric.default_mode = RoutingMode::Free;
        return (
            Target {
                fabric,
                provider_name: provider.to_string(),
                model_id: "auto".to_string(),
                v2: Some(Arc::new(MockProviderV2::healthy(provider.to_string()))),
            },
            "http://mock".to_string(),
            "auto".to_string(),
        );
    }

    let (pname, base, key, mid, mode) = match provider {
        "omniroute" => {
            let base = std::env::var("OMNIROUTE_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:20128/v1".to_string());
            let key = std::env::var("OMNIROUTE_API_KEY").unwrap_or_default();
            ("omniroute", base, key, "auto", RoutingMode::Free)
        }
        "ollama" => (
            "ollama",
            "http://localhost:11434/v1".to_string(),
            "ollama".to_string(),
            "qwen2.5-coder:latest",
            RoutingMode::Local,
        ),
        "openrouter" => (
            "openrouter",
            "https://openrouter.ai/api/v1".to_string(),
            std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
                "google/gemini-2.5-flash",
            RoutingMode::Free,
        ),
        "groq" => (
            "groq",
            "https://api.groq.com/openai/v1".to_string(),
            std::env::var("GROQ_API_KEY").unwrap_or_default(),
            "llama-3.3-70b-versatile",
            RoutingMode::Fast,
        ),
        "cerebras" => (
            "cerebras",
            "https://api.cerebras.ai/v1".to_string(),
            std::env::var("CEREBRAS_API_KEY").unwrap_or_default(),
            "llama3.3-70b",
            RoutingMode::Fast,
        ),
        "gemini" => (
            "gemini",
            "https://generativelanguage.googleapis.com/v1beta/openai/".to_string(),
            std::env::var("GEMINI_API_KEY").unwrap_or_default(),
            "gemini-2.5-flash",
            RoutingMode::Quality,
        ),
        "opengateway" => (
            "opengateway",
            std::env::var("OPENGATEWAY_BASE_URL")
                .unwrap_or_else(|_| "https://opengateway.gitlawb.com/v1".to_string()),
            std::env::var("OPENGATEWAY_API_KEY").unwrap_or_default(),
            "auto",
            RoutingMode::Free,
        ),
        other => panic!(
            "unknown provider: {other} (supported: omniroute, ollama, openrouter, groq, cerebras, gemini, opengateway)"
        ),
    };

    let provider_box: Box<dyn Provider> = match provider {
        "omniroute" => Box::new(OmniRouteProvider::new(base.clone(), key.clone())),
        _ => Box::new(OpenAiProvider {
            name: pname.to_string(),
            base_url: base.clone(),
            api_key: key.clone(),
        }),
    };

    // V2 capability surface: OmniRoute is the reference V2 provider; other gateways
    // are not yet migrated, so their V2 benchmarks are gated out by filter_for_mode.
    let v2: Option<Arc<dyn ProviderV2>> = match provider {
        "omniroute" => Some(Arc::new(OmniRouteProviderV2::new(
            base.clone(),
            key.clone(),
            None,
        ))),
        _ => None,
    };

    let mut fabric = InferenceFabric::new();
    fabric.register_provider(provider_box);
    fabric.register_model(model(pname, mid));
    fabric.default_mode = mode;

    (
        Target {
            fabric,
            provider_name: pname.to_string(),
            model_id: mid.to_string(),
            v2,
        },
        base,
        mid.to_string(),
    )
}

fn select_scenarios(
    scenario_filter: &Option<String>,
    replay: &Option<String>,
) -> Vec<corpus::Scenario> {
    let mut scen = all();
    if let Some(id) = scenario_filter {
        scen.retain(|s| s.id == id.as_str());
    }
    if let Some(path) = replay {
        let id = read_replay_id(path);
        scen.retain(|s| s.id == id.as_str());
    }
    scen
}

fn read_replay_id(path: &str) -> String {
    let p = PathBuf::from(path);
    let f = if p.is_dir() {
        p.join("report.json")
    } else {
        p.clone()
    };
    std::fs::read_to_string(&f)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()))
        .unwrap_or_default()
}

/// Real-mode runs only exercise the live gateway, so failure-injection
/// (mock-only) scenarios are filtered out. V2 capability scenarios are also
/// filtered from real runs for providers that have no V2 surface yet.
fn filter_for_mode(
    mut scenarios: Vec<corpus::Scenario>,
    mode: Mode,
    v2_available: bool,
) -> Vec<corpus::Scenario> {
    if mode == Mode::Mock {
        scenarios
    } else {
        scenarios.retain(|s| !s.requires_mock);
        scenarios.retain(|s| !s.requires_v2 || v2_available);
        scenarios
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut mock = false;
    let mut provider = "omniroute".to_string();
    let mut out: Option<String> = None;
    let mut scenario_filter: Option<String> = None;
    let mut replay: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mock" => mock = true,
            "--provider" => {
                if let Some(v) = args.get(i + 1) {
                    provider = v.clone();
                }
                i += 1;
            }
            "--out" => {
                if let Some(v) = args.get(i + 1) {
                    out = Some(v.clone());
                }
                i += 1;
            }
            "--scenario" => {
                if let Some(v) = args.get(i + 1) {
                    scenario_filter = Some(v.clone());
                }
                i += 1;
            }
            "--replay" => {
                if let Some(v) = args.get(i + 1) {
                    replay = Some(v.clone());
                }
                i += 1;
            }
            other => eprintln!("ignoring unknown arg: {other}"),
        }
        i += 1;
    }

    let stamp = chrono::Utc::now().format("%Y-%m-%d_%H%M%S").to_string();
    let out_dir = PathBuf::from(out.unwrap_or_else(|| format!("benchmarks/provider-qual/{stamp}")));
    std::fs::create_dir_all(&out_dir).expect("create out dir");

    let mode = if mock { Mode::Mock } else { Mode::Real };
    let (target, base_url, model_id) = build_target(&provider, mock);

    let env = environment::capture(mode, &target.provider_name, &base_url, &model_id);
    write_environment(&out_dir, &env).expect("write environment");

    println!(
        "Provider Qualification Harness (Phase 1.5) — provider={} mode={}\n",
        target.provider_name,
        if mock { "mock" } else { "real" }
    );

    let scenarios = filter_for_mode(
        select_scenarios(&scenario_filter, &replay),
        mode,
        target.v2.is_some(),
    );
    if scenarios.is_empty() {
        eprintln!("no scenarios matched the filter");
        return;
    }

    let now = chrono::Utc::now().to_rfc3339();
    let cand_path = PathBuf::from("benchmarks/provider-qual/candidates.json");
    let mut store = CandidateStore::load(&cand_path);

    let mut outcomes = Vec::new();
    let mut real_issues: Vec<(issue::Issue, usize)> = Vec::new();
    let mut candidate_issues: Vec<(issue::Issue, usize)> = Vec::new();

    for s in &scenarios {
        let o = run_scenario(&target, s, mode).await;
        for iss in &o.issues {
            let (count, graduated) = store.observe(iss, &now);
            if graduated {
                let mut real = iss.clone();
                real.status = "open".into();
                write_issue(&out_dir, &real).expect("write issue");
                real_issues.push((real, count));
            } else {
                let mut cand = iss.clone();
                cand.status = format!("candidate (seen {count} time(s)) — pending human review");
                write_candidate(&out_dir, &cand).expect("write candidate");
                candidate_issues.push((cand, count));
            }
        }
        write_outcome(&out_dir, &o).expect("write evidence pack");
        println!(
            "  [{}] {:<28} ok={} verified={:?} {}",
            if o.passed { "PASS" } else { "FAIL" },
            o.id,
            o.action_ok,
            o.state_accuracy,
            o.note
        );
        outcomes.push(o);
    }

    store.save(&cand_path).expect("save candidate store");

    let metrics = aggregate(&outcomes);
    let summary = render(&metrics);
    println!("\n{}", summary);

    if !real_issues.is_empty() {
        println!("## Benchmark-linked issues filed (graduated)\n");
        for (iss, count) in &real_issues {
            println!("  {}  {}  (seen {} times)", iss.id, iss.title, count);
        }
        println!("\nSee {}/issues/", out_dir.display());
    }
    if !candidate_issues.is_empty() {
        println!("## Candidate issues (pending human review)\n");
        for (iss, count) in &candidate_issues {
            println!(
                "  {}  {}  (seen {} time(s); graduates after {})",
                iss.id, iss.title, count, GRADUATE_THRESHOLD
            );
        }
        println!(
            "\nSee {}/candidates/  and  benchmarks/provider-qual/candidates.json",
            out_dir.display()
        );
    }

    println!("\nEvidence packs: {}", out_dir.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock mode must qualify every corpus scenario end-to-end (content verifiers
    /// + Fabric failure handling) with zero network access.
    #[tokio::test]
    async fn mock_qualification_passes_all_scenarios() {
        let (target, _, _) = build_target("omniroute", true);
        for s in all() {
            let o = run_scenario(&target, &s, Mode::Mock).await;
            assert!(
                o.passed,
                "scenario {} failed in mock mode: {}",
                s.id, o.note
            );
        }
    }

    /// Failure-injection scenarios are gated out of real-mode runs.
    #[test]
    fn real_mode_skips_failure_injection_scenarios() {
        let scenarios = filter_for_mode(all(), Mode::Real, false);
        assert!(
            scenarios.iter().all(|s| !s.requires_mock),
            "real mode must not run mock-only scenarios"
        );
    }
}
