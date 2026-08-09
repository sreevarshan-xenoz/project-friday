//! Provider Qualification harness core: for each [`Scenario`], connect to / complete
//! against the target provider (or a mock Fabric for failure injection), measure
//! latency, verify the result, and emit an [`Outcome`] plus any benchmark-linked
//! [`Issue`].
//!
//! Deliberately contains **no recovery and no provider evolution** — only facts.

use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use futures::StreamExt;

use friday_intelligence::{
    CircuitState, InferenceFabric, ModelCapability, ModelClient, ModelInfo, ProviderV2,
    RoutingDecision, RoutingMode,
};

use crate::corpus::{Scenario, Verify};
use crate::issue::Issue;
use crate::mock::{FaultMode, MockProvider};

/// Transport mode. `Real` talks to the configured gateway; `Mock` uses in-process
/// mock providers so the harness is runnable without network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Real,
    Mock,
}

/// One recorded provider call.
#[derive(Debug, Clone)]
pub struct Call {
    pub intent: String,
    pub ok: bool,
    pub latency_ms: u128,
    pub ts: String,
}

/// The qualification target: a Fabric wrapping the single provider under test (for
/// the V1 corpus), plus an optional V2 provider surface (for Streaming / Dynamic
/// Discovery benchmarks). The V2 surface is `None` for providers that have not yet
/// been migrated to the capability API.
pub struct Target {
    pub fabric: InferenceFabric,
    pub provider_name: String,
    pub model_id: String,
    pub v2: Option<Arc<dyn ProviderV2>>,
}

/// Result of running one scenario.
#[derive(Debug, Default)]
pub struct Outcome {
    pub id: String,
    pub group: String,
    pub action_ok: bool,
    pub latency_ms: u128,
    pub state_accuracy: Option<bool>,
    pub passed: bool,
    pub trace: Vec<Call>,
    pub issues: Vec<Issue>,
    pub note: String,
    /// The injected fault, when this was a failure-injection scenario.
    pub fault: Option<String>,
    /// V2 capability fact observed by this scenario (tool_calling), if applicable.
    pub tool_calling: Option<bool>,
    /// The (provider, vendor model id) that actually served the completion, when
    /// the fabric's routing fell back through the catalog.
    pub served_model: Option<String>,
    /// The full routing audit trail (requested tier, candidates considered with
    /// scores, chosen, served, and why they differed) — so qualification reports
    /// are auditable, not just reportable by outcome.
    pub routing_decision: Option<RoutingDecision>,
    /// Mean time-to-first-token observed by this scenario (streaming only).
    pub ttft_ms: Option<f64>,
    /// Generation throughput observed by this scenario (streaming only).
    pub tokens_per_second: Option<f64>,
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

/// Strip ```json fences so a fenced model response can still be parsed as JSON.
fn strip_fences(s: &str) -> String {
    let t = s.trim();
    if let (Some(rest), Some(_)) = (t.strip_prefix("```json"), t.strip_suffix("```")) {
        return rest.trim().to_string();
    }
    if let (Some(rest), Some(_)) = (t.strip_prefix("```"), t.strip_suffix("```")) {
        return rest.trim().to_string();
    }
    t.to_string()
}

/// Run one scenario and return its outcome (with an issue if it failed).
pub async fn run_scenario(target: &Target, scenario: &Scenario, _mode: Mode) -> Outcome {
    let trace: Vec<Call> = Vec::new();
    let mut o = match scenario.verify {
        Verify::Reachable => run_connectivity(target, scenario, trace).await,
        Verify::Completion => run_completion(target, scenario, trace).await,
        Verify::Json => run_json(target, scenario, trace).await,
        Verify::LongContext => run_long_context(target, scenario, trace).await,
        Verify::FailureHandled => run_failure(scenario, trace).await,
        Verify::Streaming => run_streaming(target, scenario, trace).await,
        Verify::DynamicDiscovery => run_dynamic_discovery(target, scenario, trace).await,
    };

    // Record which vendor model actually answered (failover may differ from the
    // declared routing target, e.g. auto-route quota → free-model fallback).
    if o.served_model.is_none() {
        if let Some((provider, model)) = target.fabric.last_served() {
            o.served_model = Some(format!("{provider}/{model}"));
        }
    }

    // Attach the full routing audit trail so evidence shows why (failover, quota,
    // transient reset) in addition to what (which vendor model) served.
    if o.routing_decision.is_none() {
        o.routing_decision = target.fabric.last_decision();
    }

    if !o.passed {
        let (expected, actual) = expected_actual(scenario, o.action_ok, o.state_accuracy);
        o.issues
            .push(issue_for(scenario, &o.note, &expected, &actual));
    }
    o
}

async fn run_connectivity(target: &Target, scenario: &Scenario, mut trace: Vec<Call>) -> Outcome {
    let provider = target.fabric.providers.get(&target.provider_name);
    let start = Instant::now();
    let reachable = match &provider {
        Some(p) => p.check_health().await,
        None => false,
    };
    let latency = start.elapsed().as_millis();
    trace.push(Call {
        intent: scenario.action.intent.to_string(),
        ok: reachable,
        latency_ms: latency,
        ts: now(),
    });
    let state_accuracy = Some(reachable);
    Outcome {
        id: scenario.id.into(),
        group: scenario.group.into(),
        action_ok: reachable,
        latency_ms: latency,
        state_accuracy,
        passed: state_accuracy == Some(true),
        trace,
        issues: vec![],
        note: format!("reachable={reachable}"),
        fault: None,
        tool_calling: None,
        served_model: None,
        routing_decision: None,
        ttft_ms: None,
        tokens_per_second: None,
    }
}

async fn run_completion(target: &Target, scenario: &Scenario, mut trace: Vec<Call>) -> Outcome {
    let prompt = scenario
        .action
        .params
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let start = Instant::now();
    let res = target.fabric.complete(&prompt).await;
    let latency = start.elapsed().as_millis();
    trace.push(Call {
        intent: scenario.action.intent.to_string(),
        ok: res.is_ok(),
        latency_ms: latency,
        ts: now(),
    });
    match res {
        Ok(text) => {
            let ok = !text.trim().is_empty();
            Outcome {
                id: scenario.id.into(),
                group: scenario.group.into(),
                action_ok: true,
                latency_ms: latency,
                state_accuracy: Some(ok),
                passed: ok,
                trace,
                issues: vec![],
                note: format!("response_len={}", text.len()),
                fault: None,
                tool_calling: None,
                served_model: None,
                routing_decision: None,
                ttft_ms: None,
                tokens_per_second: None,
            }
        }
        Err(e) => Outcome {
            id: scenario.id.into(),
            group: scenario.group.into(),
            action_ok: false,
            latency_ms: latency,
            state_accuracy: Some(false),
            passed: false,
            trace,
            issues: vec![],
            note: format!("error: {e}"),
            fault: None,
            tool_calling: None,
            served_model: None,
            routing_decision: None,
            ttft_ms: None,
            tokens_per_second: None,
        },
    }
}

async fn run_json(target: &Target, scenario: &Scenario, mut trace: Vec<Call>) -> Outcome {
    let prompt = scenario
        .action
        .params
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let start = Instant::now();
    let res = target.fabric.complete(&prompt).await;
    let latency = start.elapsed().as_millis();
    trace.push(Call {
        intent: scenario.action.intent.to_string(),
        ok: res.is_ok(),
        latency_ms: latency,
        ts: now(),
    });
    match res {
        Ok(text) => {
            let clean = strip_fences(&text);
            let parsed = serde_json::from_str::<serde_json::Value>(&clean);
            let valid = parsed.map(|v| {
                v.is_object()
                    && v.get("name").map(|x| x.is_string()).unwrap_or(false)
                    && v.get("age").map(|x| x.is_i64()).unwrap_or(false)
                    && v.get("tags").map(|x| x.is_array()).unwrap_or(false)
            });
            let ok = valid.unwrap_or(false);
            Outcome {
                id: scenario.id.into(),
                group: scenario.group.into(),
                action_ok: true,
                latency_ms: latency,
                state_accuracy: Some(ok),
                passed: ok,
                trace,
                issues: vec![],
                note: format!("json_valid={ok}"),
                fault: None,
                tool_calling: None,
                served_model: None,
                routing_decision: None,
                ttft_ms: None,
                tokens_per_second: None,
            }
        }
        Err(e) => Outcome {
            id: scenario.id.into(),
            group: scenario.group.into(),
            action_ok: false,
            latency_ms: latency,
            state_accuracy: Some(false),
            passed: false,
            trace,
            issues: vec![],
            note: format!("error: {e}"),
            fault: None,
            tool_calling: None,
            served_model: None,
            routing_decision: None,
            ttft_ms: None,
            tokens_per_second: None,
        },
    }
}

async fn run_long_context(target: &Target, scenario: &Scenario, mut trace: Vec<Call>) -> Outcome {
    let prompt = scenario
        .action
        .params
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let start = Instant::now();
    let res = target.fabric.complete(&prompt).await;
    let latency = start.elapsed().as_millis();
    trace.push(Call {
        intent: scenario.action.intent.to_string(),
        ok: res.is_ok(),
        latency_ms: latency,
        ts: now(),
    });
    match res {
        Ok(text) => {
            let clean = strip_fences(&text);
            let parsed = serde_json::from_str::<serde_json::Value>(&clean);
            let ok = parsed.map(|v| {
                v.as_array()
                    .map(|a| {
                        a.len() == 50
                            && a.iter()
                                .enumerate()
                                .all(|(i, x)| x.as_i64() == Some((i + 1) as i64))
                    })
                    .unwrap_or(false)
            });
            let ok = ok.unwrap_or(false);
            Outcome {
                id: scenario.id.into(),
                group: scenario.group.into(),
                action_ok: true,
                latency_ms: latency,
                state_accuracy: Some(ok),
                passed: ok,
                trace,
                issues: vec![],
                note: format!("array_complete={ok}"),
                fault: None,
                tool_calling: None,
                served_model: None,
                routing_decision: None,
                ttft_ms: None,
                tokens_per_second: None,
            }
        }
        Err(e) => Outcome {
            id: scenario.id.into(),
            group: scenario.group.into(),
            action_ok: false,
            latency_ms: latency,
            state_accuracy: Some(false),
            passed: false,
            trace,
            issues: vec![],
            note: format!("error: {e}"),
            fault: None,
            tool_calling: None,
            served_model: None,
            routing_decision: None,
            ttft_ms: None,
            tokens_per_second: None,
        },
    }
}

/// Failure-injection scenario: build a fresh Fabric around a mock provider that
/// fails with the injected fault, drive it to failure, and verify the Fabric
/// reacted (circuit opened, health score degraded). Mock-only.
async fn run_failure(scenario: &Scenario, mut trace: Vec<Call>) -> Outcome {
    let injection = scenario
        .injection
        .expect("failure scenario must carry an injection");
    let mut fabric = InferenceFabric::new();
    // Use a provider name that is selectable under RoutingMode::Free so the Fabric's
    // failover/circuit path is actually exercised (the name is cosmetic here — we are
    // qualifying Fabric behavior, not the provider).
    let provider_name = "omniroute".to_string();
    let fault = MockProvider::new(provider_name.clone(), FaultMode::from_injection(injection));
    fabric.register_provider(Box::new(fault));
    fabric.register_model(ModelInfo {
        id: "auto".into(),
        family: "auto".into(),
        provider: provider_name.clone(),
        latency_ms: 100,
        reliability: 1.0,
        capabilities: vec![ModelCapability::Json],
        max_context: 4096,
        tier: None,
    });
    fabric.default_mode = RoutingMode::Free;

    let mut last_err = String::new();
    for _ in 0..3 {
        if let Err(e) = fabric.complete("trigger injected failure").await {
            last_err = e;
        }
    }

    let circuit_open = {
        let guards = fabric.circuits.lock().unwrap();
        matches!(
            guards.get(&provider_name).map(|c| c.state),
            Some(CircuitState::Open)
        )
    };
    let health_dropped = {
        let scores = fabric.health_scores.lock().unwrap();
        scores.get(&provider_name).copied().unwrap_or(1.0) < 1.0
    };

    let reacted = circuit_open && health_dropped;
    trace.push(Call {
        intent: "provider.failure".into(),
        ok: false,
        latency_ms: 0,
        ts: now(),
    });

    Outcome {
        id: scenario.id.into(),
        group: scenario.group.into(),
        action_ok: false,
        latency_ms: 0,
        state_accuracy: Some(reacted),
        passed: reacted,
        trace,
        issues: vec![],
        note: format!(
            "circuit_open={circuit_open}, health_dropped={health_dropped}, last_err={last_err}"
        ),
        fault: Some(format!("{:?}", injection)),
        tool_calling: None,
        served_model: None,
        routing_decision: None,
        ttft_ms: None,
        tokens_per_second: None,
    }
}

/// Streaming benchmark (M5.2): drive the V2 provider's `stream()` and reassemble
/// the incremental chunks. Passes when the provider exposes streaming, emits at
/// least one chunk, and the concatenated deltas equal a non-empty completion.
async fn run_streaming(target: &Target, scenario: &Scenario, mut trace: Vec<Call>) -> Outcome {
    let fail = |note: String, trace: Vec<Call>| Outcome {
        id: scenario.id.into(),
        group: scenario.group.into(),
        action_ok: false,
        latency_ms: 0,
        state_accuracy: Some(false),
        passed: false,
        trace,
        issues: vec![],
        note,
        fault: None,
        tool_calling: None,
        served_model: None,
        routing_decision: None,
        ttft_ms: None,
        tokens_per_second: None,
    };

    let Some(v2) = &target.v2 else {
        return fail("no V2 provider surface available".into(), trace);
    };
    let caps = v2.capabilities();
    if !caps.streaming {
        return fail("provider declares streaming=false".into(), trace);
    }

    let prompt = scenario
        .action
        .params
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let start = Instant::now();
    let stream_res = v2.stream(&target.model_id, &prompt, false).await;
    let latency = start.elapsed().as_millis();

    let mut stream = match stream_res {
        Ok(s) => s,
        Err(e) => return fail(format!("stream() error: {e}"), trace),
    };

    let mut chunks: usize = 0;
    let mut reassembled = String::new();
    let mut saw_finish = false;
    let mut ttft_ms: Option<f64> = None;
    while let Some(item) = stream.next().await {
        match item {
            Ok(ck) => {
                if ttft_ms.is_none() {
                    ttft_ms = Some(start.elapsed().as_secs_f64() * 1000.0);
                }
                chunks += 1;
                reassembled.push_str(&ck.delta);
                if ck.finish_reason.is_some() {
                    saw_finish = true;
                }
            }
            Err(e) => return fail(format!("stream chunk error: {e}"), trace),
        }
    }

    let ok = chunks >= 1 && !reassembled.trim().is_empty() && saw_finish;
    let elapsed_secs = latency as f64 / 1000.0;
    // The stream API does not expose usage tokens; estimate throughput from
    // reassembled characters (≈4 chars/token) rather than fabricate a number.
    let tokens_per_second = if elapsed_secs > 0.0 {
        Some(reassembled.chars().count() as f64 / 4.0 / elapsed_secs)
    } else {
        None
    };
    trace.push(Call {
        intent: scenario.action.intent.to_string(),
        ok,
        latency_ms: latency,
        ts: now(),
    });
    Outcome {
        id: scenario.id.into(),
        group: scenario.group.into(),
        action_ok: true,
        latency_ms: latency,
        state_accuracy: Some(ok),
        passed: ok,
        trace,
        issues: vec![],
        note: format!(
            "chunks={chunks}, reassembled_len={}, saw_finish={saw_finish}",
            reassembled.len()
        ),
        fault: None,
        tool_calling: Some(caps.tool_calling),
        served_model: None,
        routing_decision: None,
        ttft_ms,
        tokens_per_second,
    }
}

/// Dynamic Discovery benchmark (M5.2): call the V2 provider's `list_models()` and
/// verify the catalog is non-empty and every entry carries a usable id. This is the
/// provider's source of truth for routing-by-capability, so it must be well-shaped.
async fn run_dynamic_discovery(
    target: &Target,
    scenario: &Scenario,
    mut trace: Vec<Call>,
) -> Outcome {
    let fail = |note: String, trace: Vec<Call>| Outcome {
        id: scenario.id.into(),
        group: scenario.group.into(),
        action_ok: false,
        latency_ms: 0,
        state_accuracy: Some(false),
        passed: false,
        trace,
        issues: vec![],
        note,
        fault: None,
        tool_calling: None,
        served_model: None,
        routing_decision: None,
        ttft_ms: None,
        tokens_per_second: None,
    };

    let Some(v2) = &target.v2 else {
        return fail("no V2 provider surface available".into(), trace);
    };
    let caps = v2.capabilities();
    if !caps.dynamic_models {
        return fail("provider declares dynamic_models=false".into(), trace);
    }

    let start = Instant::now();
    let models_res = v2.list_models().await;
    let latency = start.elapsed().as_millis();

    let models = match models_res {
        Ok(m) => m,
        Err(e) => return fail(format!("list_models() error: {e}"), trace),
    };

    let well_shaped = !models.is_empty() && models.iter().all(|m| !m.id.trim().is_empty());
    trace.push(Call {
        intent: scenario.action.intent.to_string(),
        ok: well_shaped,
        latency_ms: latency,
        ts: now(),
    });
    Outcome {
        id: scenario.id.into(),
        group: scenario.group.into(),
        action_ok: true,
        latency_ms: latency,
        state_accuracy: Some(well_shaped),
        passed: well_shaped,
        trace,
        issues: vec![],
        note: format!("models_discovered={}", models.len()),
        fault: None,
        tool_calling: Some(caps.tool_calling),
        served_model: None,
        routing_decision: None,
        ttft_ms: None,
        tokens_per_second: None,
    }
}

fn issue_for(scenario: &Scenario, note: &str, expected: &str, actual: &str) -> Issue {
    let id = format!(
        "PRV-{}",
        scenario.id.to_uppercase().replace(['.', '-'], "_")
    );
    let benchmark = format!(
        "Provider Qualification / {} / {}",
        scenario.group,
        scenario
            .injection
            .map(|_| "Failure Injection".to_string())
            .unwrap_or_else(|| "Corpus".to_string())
    );
    Issue {
        id,
        title: format!("Provider qualification failure: {}", scenario.id),
        observed: format!("corpus {} ({})", scenario.id, scenario.group),
        expected: expected.to_string(),
        actual: format!("{actual} — {note}"),
        benchmark,
        status: "Open".into(),
    }
}

fn expected_actual(
    scenario: &Scenario,
    action_ok: bool,
    state_accuracy: Option<bool>,
) -> (String, String) {
    let expected = match scenario.verify {
        Verify::Reachable => "gateway is reachable",
        Verify::Completion => "completion returns a non-empty response",
        Verify::Json => "response is valid, schema-shaped JSON",
        Verify::LongContext => "long-context response is complete (no truncation)",
        Verify::FailureHandled => "Inference Fabric opens the circuit and degrades health",
        Verify::Streaming => "provider streams completions incrementally without loss",
        Verify::DynamicDiscovery => "provider exposes a non-empty, well-shaped model catalog",
    };
    let actual = match scenario.verify {
        Verify::FailureHandled => {
            format!("action_ok={action_ok}, fabric_reacted={:?}", state_accuracy)
        }
        _ => format!("action_ok={action_ok}, verified={:?}", state_accuracy),
    };
    (expected.into(), actual)
}
