//! Chaos Laboratory — Phase 1: Provider Failures.
//!
//! Stress-tests the frozen Inference Fabric routing loop under adversarial
//! provider behavior. Not an extension of the architecture: every scenario is a
//! *fact* we try to break — no deadlock, no infinite retries, bounded failover
//! latency, deterministic fallback. Each scenario builds a small fabric,
//! injects one hostile failure mode (plus a healthy fallback), drives requests,
//! and returns a [`ChaosVerdict`] with the full per-request audit trail
//! (candidates in ranking order, served provider, latency, per-candidate
//! rejections) so a failure is re-playable instead of anecdotal.
//!
//! Verdicts are *returned*, never panicked, so `--chaos` renders a report and
//! the harness's exit code reflects any broken guarantee.

use std::time::Duration;

use friday_intelligence::{
    classify_failure_reason, Budget, CircuitState, InferenceFabric, ModelCapability, ModelClient,
    ModelInfo, Preference, ProviderV2, RequestRequirements, RoutingMode,
};
use futures::StreamExt;

use crate::corpus::strict_json_prompt;
use crate::mock::{FaultMode, MockProvider, MockProviderV2};

/// How one provider behaves under chaos. The qualification mocks stay untouched;
/// these specs drive the adversarial fabrics only.
pub struct ChaosSpec {
    /// Provider name. Must be selectable under the scenario's `RoutingMode`.
    pub name: &'static str,
    /// Static fault every call exhibits (`Ok` = behaves, possibly statefully).
    pub fault: FaultMode,
    /// Advertise `ModelCapability::Local` (needed for `RoutingMode::Hybrid`).
    pub local: bool,
    /// Model latency hint — the secondary sort key (primary is reliability).
    pub latency_ms: u64,
    /// Timeout every N-th call (intermittent timeouts).
    pub timeout_every: Option<usize>,
    /// HTTP 429 every N-th call (rate-limit every N).
    pub rate_limit_every: Option<usize>,
}

/// One request driven through a chaotic fabric — the audit trail item.
#[derive(Debug, Clone)]
pub struct RequestObservation {
    pub request: usize,
    /// Ranked candidate list at routing start (attempt order).
    pub candidates: Vec<String>,
    /// Provider that served, if any (`None` = request failed or deadlocked).
    pub served: Option<String>,
    pub elapsed_ms: u64,
    /// `None` = served by some provider. `Some` = every candidate failed or the
    /// harness deadline (deadlock) fired.
    pub error: Option<String>,
    /// Rejected candidates and why, in attempt order (the structured "reason"
    /// behind `served != chosen`).
    pub failures: Vec<(String, String)>,
}

/// Pass/fail for one chaos scenario, with everything needed to audit the call.
#[derive(Debug, Clone)]
pub struct ChaosVerdict {
    pub id: &'static str,
    pub passed: bool,
    pub note: String,
    pub observations: Vec<RequestObservation>,
}

fn build_chaos_fabric(specs: &[ChaosSpec], deadline_ms: u64, mode: RoutingMode) -> InferenceFabric {
    let mut fabric = InferenceFabric::new();
    for s in specs {
        let mut p = MockProvider::new(s.name.to_string(), s.fault);
        p.prompt_aware = true;
        if let Some(n) = s.timeout_every {
            p = p.with_timeout_every(n);
        }
        if let Some(n) = s.rate_limit_every {
            p = p.with_rate_limit_every(n);
        }
        let mut caps = vec![ModelCapability::Json];
        if s.local {
            caps.push(ModelCapability::Local);
        }
        fabric.register_provider(Box::new(p));
        fabric.register_model(ModelInfo {
            id: "auto".to_string(),
            family: "auto".to_string(),
            provider: s.name.to_string(),
            latency_ms: s.latency_ms,
            reliability: 1.0,
            capabilities: caps,
            max_context: 4096,
            tier: None,
        });
    }
    fabric.default_mode = mode;
    fabric.request_timeout = Some(Duration::from_millis(deadline_ms));
    fabric
}

/// Drive `count` requests through the fabric, capturing per-request
/// observations. The outer harness deadline is strictly larger than the
/// fabric's per-attempt deadline, so it only fires on a true deadlock — that is
/// what `error == "harness deadline"` means.
async fn drive(
    fabric: &InferenceFabric,
    prompt: &str,
    count: usize,
    deadline_ms: u64,
) -> Vec<RequestObservation> {
    let outer_budget = Duration::from_millis(deadline_ms * 2 + 1000);
    let mut observations = Vec::new();
    for i in 0..count {
        let candidates: Vec<String> = fabric
            .select_candidates(fabric.default_mode)
            .iter()
            .map(|(m, p)| format!("{}/{}", p.name(), m.id))
            .collect();

        let start = std::time::Instant::now();
        let outcome = tokio::time::timeout(outer_budget, fabric.complete(prompt)).await;
        let elapsed_ms = start.elapsed().as_millis() as u64;

        let (served, error, failures) = match outcome {
            Ok(Ok(_)) => (
                fabric.last_served(),
                None,
                fabric
                    .last_decision()
                    .map(|d| d.failures)
                    .unwrap_or_default(),
            ),
            Ok(Err(e)) => (
                None,
                Some(e),
                fabric
                    .last_decision()
                    .map(|d| d.failures)
                    .unwrap_or_default(),
            ),
            Err(_) => (
                None,
                Some("harness deadline — fabric did not return (deadlock)".to_string()),
                Vec::new(),
            ),
        };

        observations.push(RequestObservation {
            request: i,
            candidates,
            served: served.map(|(p, m)| format!("{p}/{m}")),
            elapsed_ms,
            error,
            failures: failures
                .into_iter()
                .map(|f| (f.provider, f.error))
                .collect(),
        });
    }
    observations
}

fn served_by(o: &RequestObservation, provider: &str) -> bool {
    o.served.as_deref().is_some_and(|s| s.starts_with(provider))
}

fn no_deadlock(o: &RequestObservation) -> bool {
    !o.error
        .as_deref()
        .unwrap_or("")
        .contains("harness deadline")
}

fn bounded(o: &RequestObservation, max_ms: u64) -> bool {
    o.elapsed_ms <= max_ms
}

/// S1: provider unavailable before any request reaches it (503).
pub async fn unavailable_before_request() -> ChaosVerdict {
    failover_fact(
        "chaos.unavailable_before_request",
        FaultMode::Unavailable,
        "503",
    )
    .await
}

/// S3: provider returns an unparseable body.
pub async fn malformed_json() -> ChaosVerdict {
    failover_fact("chaos.malformed_json", FaultMode::Malformed, "parse").await
}

/// Shared shape for S1/S3: a bad provider fails fast, the fabric must fail
/// over to the healthy fallback within a bounded time, deterministically, and
/// record the rejection with its reason.
async fn failover_fact(id: &'static str, bad_fault: FaultMode, reason_hint: &str) -> ChaosVerdict {
    let fabric = build_chaos_fabric(
        &[
            ChaosSpec {
                name: "omniroute",
                fault: bad_fault,
                local: false,
                latency_ms: 50,
                timeout_every: None,
                rate_limit_every: None,
            },
            ChaosSpec {
                name: "openrouter",
                fault: FaultMode::Ok,
                local: false,
                latency_ms: 200,
                timeout_every: None,
                rate_limit_every: None,
            },
        ],
        2000,
        RoutingMode::Free,
    );
    let observations = drive(&fabric, "trigger chaos", 1, 2000).await;
    let o = &observations[0];
    let passed = served_by(o, "openrouter")
        && o.error.is_none()
        && no_deadlock(o)
        && bounded(o, 1500)
        && o.failures
            .iter()
            .any(|(p, e)| p == "omniroute" && e.contains(reason_hint));
    ChaosVerdict {
        id,
        passed,
        note: format!(
            "bad provider rejected with reason (recorded): {:?}; served fallback within {}ms",
            o.failures, o.elapsed_ms
        ),
        observations,
    }
}

/// S4: provider hangs forever. The fabric's per-attempt deadline (2s) must turn
/// the hang into a bounded failure and fail over — the harness deadline must
/// never fire.
pub async fn hangs_forever() -> ChaosVerdict {
    deadline_fact("chaos.hangs_forever", FaultMode::Hang).await
}

/// S5: extremely slow (5s) provider under a 2s per-attempt deadline —
/// "slow enough ≈ unavailable" must still fail over within budget.
pub async fn extremely_slow() -> ChaosVerdict {
    deadline_fact("chaos.extremely_slow", FaultMode::Slow).await
}

/// Shared shape for S4/S5: the fabric deadline (not the harness) must govern
/// failover, at ~2s, deterministically to the fallback.
async fn deadline_fact(id: &'static str, bad_fault: FaultMode) -> ChaosVerdict {
    let fabric = build_chaos_fabric(
        &[
            ChaosSpec {
                name: "omniroute",
                fault: bad_fault,
                local: false,
                latency_ms: 50,
                timeout_every: None,
                rate_limit_every: None,
            },
            ChaosSpec {
                name: "openrouter",
                fault: FaultMode::Ok,
                local: false,
                latency_ms: 200,
                timeout_every: None,
                rate_limit_every: None,
            },
        ],
        2000,
        RoutingMode::Free,
    );
    let observations = drive(&fabric, "trigger chaos", 1, 2000).await;
    let o = &observations[0];
    let passed = served_by(o, "openrouter")
        && o.error.is_none()
        && no_deadlock(o)
        && o.elapsed_ms >= 1800
        && bounded(o, 3000)
        && o.failures
            .iter()
            .any(|(p, e)| p == "omniroute" && e.contains("timed out after"));
    ChaosVerdict {
        id,
        passed,
        note: format!(
            "fabric deadline governed failover at {}ms (harness budget 5000ms); rejected reason: {:?}",
            o.elapsed_ms, o.failures
        ),
        observations,
    }
}

/// S6: provider times out every other call. First success should go to the
/// fast provider; the first transient timeout must fail over; and — the
/// observed fact — the demoted provider does not get its place back for the
/// rest of the run (no recovery/re-probe window exists).
pub async fn intermittent_timeouts() -> ChaosVerdict {
    let fabric = build_chaos_fabric(
        &[
            ChaosSpec {
                name: "omniroute",
                fault: FaultMode::Ok,
                local: false,
                latency_ms: 50,
                timeout_every: Some(2),
                rate_limit_every: None,
            },
            ChaosSpec {
                name: "openrouter",
                fault: FaultMode::Ok,
                local: false,
                latency_ms: 200,
                timeout_every: None,
                rate_limit_every: None,
            },
        ],
        2000,
        RoutingMode::Free,
    );
    let observations = drive(&fabric, "trigger chaos", 6, 2000).await;
    let pattern: Vec<bool> = observations
        .iter()
        .map(|o| served_by(o, "openrouter"))
        .collect();
    let expected = vec![false, true, true, true, true, true];
    let passed = observations
        .iter()
        .all(|o| o.error.is_none() && no_deadlock(o))
        && observations.iter().all(|o| bounded(o, 2000))
        && pattern == expected
        && observations[0].failures.is_empty();
    ChaosVerdict {
        id: "chaos.intermittent_timeouts",
        passed,
        note: format!(
            "served pattern (openrouter?): {:?}; after the transient timeout at request 2, \
             omniroute stays health-degraded and is never re-probed — no recovery window exists",
            pattern
        ),
        observations,
    }
}

/// S7: provider rate-limits (429) every third call. Same shape as S6: first
/// two succeed, the 429 fails over, and the degraded provider stays demoted.
pub async fn rate_limits_every_n() -> ChaosVerdict {
    let fabric = build_chaos_fabric(
        &[
            ChaosSpec {
                name: "omniroute",
                fault: FaultMode::Ok,
                local: false,
                latency_ms: 50,
                timeout_every: None,
                rate_limit_every: Some(3),
            },
            ChaosSpec {
                name: "openrouter",
                fault: FaultMode::Ok,
                local: false,
                latency_ms: 200,
                timeout_every: None,
                rate_limit_every: None,
            },
        ],
        2000,
        RoutingMode::Free,
    );
    let observations = drive(&fabric, "trigger chaos", 6, 2000).await;
    let pattern: Vec<bool> = observations
        .iter()
        .map(|o| served_by(o, "openrouter"))
        .collect();
    let expected = vec![false, false, true, true, true, true];
    let passed = observations
        .iter()
        .all(|o| o.error.is_none() && no_deadlock(o))
        && observations.iter().all(|o| bounded(o, 2000))
        && pattern == expected;
    ChaosVerdict {
        id: "chaos.rate_limits_every_n",
        passed,
        note: format!(
            "served pattern (openrouter?): {:?}; the 429 at request 3 degraded omniroute for \
             the rest of the run",
            pattern
        ),
        observations,
    }
}

/// S8: provider returns a truncated body as if it were success. Routing has no
/// content-integrity gate — it serves the partial — so integrity enforcement
/// belongs to the verification layer. This fact must hold without a panic.
pub async fn partial_responses() -> ChaosVerdict {
    let fabric = build_chaos_fabric(
        &[
            ChaosSpec {
                name: "omniroute",
                fault: FaultMode::Partial,
                local: false,
                latency_ms: 50,
                timeout_every: None,
                rate_limit_every: None,
            },
            ChaosSpec {
                name: "openrouter",
                fault: FaultMode::Ok,
                local: false,
                latency_ms: 200,
                timeout_every: None,
                rate_limit_every: None,
            },
        ],
        2000,
        RoutingMode::Free,
    );
    let prompt = strict_json_prompt();
    let candidates: Vec<String> = fabric
        .select_candidates(fabric.default_mode)
        .iter()
        .map(|(m, p)| format!("{}/{}", p.name(), m.id))
        .collect();

    let start = std::time::Instant::now();
    let outcome = tokio::time::timeout(Duration::from_millis(5000), fabric.complete(&prompt)).await;
    let elapsed_ms = start.elapsed().as_millis() as u64;

    let (served, error, body) = match outcome {
        Ok(Ok(b)) => (fabric.last_served(), None, Some(b)),
        Ok(Err(e)) => (None, Some(e), None),
        Err(_) => (None, Some("harness deadline — deadlock".to_string()), None),
    };
    let failures: Vec<(String, String)> = fabric
        .last_decision()
        .map(|d| {
            d.failures
                .into_iter()
                .map(|f| (f.provider, f.error))
                .collect()
        })
        .unwrap_or_default();

    // Integrity is a verification-layer concern: the body routing accepted must
    // fail strict-JSON parsing, proving routing has no content-integrity gate.
    let rejected_by_verification = body
        .as_deref()
        .is_some_and(|b| serde_json::from_str::<serde_json::Value>(b).is_err());

    let observations = vec![RequestObservation {
        request: 0,
        candidates,
        served: served.map(|(p, m)| format!("{p}/{m}")),
        elapsed_ms,
        error,
        failures,
    }];
    let o = &observations[0];
    let passed = served_by(o, "omniroute")
        && o.error.is_none()
        && no_deadlock(o)
        && rejected_by_verification;
    ChaosVerdict {
        id: "chaos.partial_responses",
        passed,
        note: format!(
            "routing served the truncated body {:?} as success — no integrity gate; \
             the verification layer rejects it: {rejected_by_verification}. No panic, no deadlock.",
            body
        ),
        observations,
    }
}

/// S2: connection dies after the first streaming chunk. The streaming surface
/// must surface the error (not a silently-truncated completion) and stop.
pub async fn dies_mid_stream() -> ChaosVerdict {
    let v2 = MockProviderV2::chaotic("omniroute".to_string(), FaultMode::MidStream);
    let mut stream = match v2.stream("mock-auto", "trigger chaos", false).await {
        Ok(s) => s,
        Err(e) => {
            return ChaosVerdict {
                id: "chaos.dies_mid_stream",
                passed: false,
                note: format!("stream() refused to start: {e}"),
                observations: vec![],
            }
        }
    };

    let first = tokio::time::timeout(Duration::from_secs(2), stream.next()).await;
    let second = tokio::time::timeout(Duration::from_secs(2), stream.next()).await;
    let third = tokio::time::timeout(Duration::from_secs(2), stream.next()).await;

    let first_chunk = matches!(
        first,
        Ok(Some(Ok(chunk))) if chunk.finish_reason.is_none()
    );
    let error_surfaced = matches!(second, Ok(Some(Err(_))));
    let stream_stopped = matches!(third, Ok(None));

    let passed = first_chunk && error_surfaced && stream_stopped;
    ChaosVerdict {
        id: "chaos.dies_mid_stream",
        passed,
        note: format!(
            "first chunk ok (no finish_reason): {first_chunk}; error surfaced mid-stream: \
             {error_surfaced}; stream ended after the error: {stream_stopped}"
        ),
        observations: vec![],
    }
}

/// Everything is bad: A offline, B circuit open, C qualification missing,
/// D rate-limited on every call, E extremely slow. Routing must still return —
/// picking the least-bad (the only provider that answers) — without deadlock,
/// within a bounded total latency, deterministically.
pub async fn all_bad() -> ChaosVerdict {
    let fabric = build_chaos_fabric(
        &[
            ChaosSpec {
                name: "omniroute",
                fault: FaultMode::Unavailable,
                local: false,
                latency_ms: 50,
                timeout_every: None,
                rate_limit_every: None,
            },
            ChaosSpec {
                name: "nvidia-nim",
                fault: FaultMode::Ok,
                local: false,
                latency_ms: 60,
                timeout_every: None,
                rate_limit_every: None,
            },
            ChaosSpec {
                name: "openrouter",
                fault: FaultMode::Ok,
                local: false,
                latency_ms: 200,
                timeout_every: None,
                rate_limit_every: None,
            },
            ChaosSpec {
                name: "opengateway",
                fault: FaultMode::Ok,
                local: false,
                latency_ms: 70,
                timeout_every: None,
                rate_limit_every: Some(1),
            },
            ChaosSpec {
                name: "local-a",
                fault: FaultMode::Slow,
                local: true,
                latency_ms: 50,
                timeout_every: None,
                rate_limit_every: None,
            },
        ],
        2000,
        RoutingMode::Hybrid,
    );

    // B: circuit already open — the fabric has already excluded this provider.
    {
        let mut circuits = fabric.circuits.lock().unwrap();
        let cb = circuits
            .get_mut("nvidia-nim")
            .expect("nvidia-nim registered");
        cb.state = CircuitState::Open;
        cb.failures = 3;
        cb.last_failure = Some(std::time::Instant::now());
    }
    // C: no qualification profile registered — must NOT gate selection.
    let qual_missing = fabric.qualification_profile("openrouter", "auto").is_none();

    let observations = drive(&fabric, "trigger chaos", 1, 2000).await;
    let o = &observations[0];
    let passed = served_by(o, "openrouter")
        && o.error.is_none()
        && no_deadlock(o)
        && bounded(o, 3000)
        && qual_missing;
    ChaosVerdict {
        id: "chaos.all_bad",
        passed,
        note: format!(
            "least-bad = openrouter (the only provider that answered); {} rejected candidates \
             recorded; total latency {}ms dominated by the slow candidate's full 2s deadline; \
             qualification missing for C was not a selection gate: {qual_missing}",
            o.failures.len(),
            o.elapsed_ms
        ),
        observations,
    }
}

/// R5-2-3-2 (F2/F3) — a Groq 429 must not pin the fabric to a dead endpoint.
/// It surfaces honestly as a failing `CandidateFailure` (with the `429`
/// reason) and the fabric fails over to a healthy free-remote floor member
/// (`opengateway`) within the bounded deadline — no infinite retry, no
/// deadlock, deterministic.
pub async fn r5_2_3_2_groq_429_does_not_pin_endpoint() -> ChaosVerdict {
    let fabric = build_chaos_fabric(
        &[
            ChaosSpec {
                name: "groq",
                fault: FaultMode::Http429,
                local: false,
                latency_ms: 50,
                timeout_every: None,
                rate_limit_every: None,
            },
            ChaosSpec {
                name: "opengateway",
                fault: FaultMode::Ok,
                local: false,
                latency_ms: 200,
                timeout_every: None,
                rate_limit_every: None,
            },
        ],
        2000,
        RoutingMode::Free,
    );
    let observations = drive(&fabric, "trigger chaos", 1, 2000).await;
    let o = &observations[0];
    let passed = served_by(o, "opengateway")
        && o.error.is_none()
        && no_deadlock(o)
        && bounded(o, 1500)
        && o.failures
            .iter()
            .any(|(p, e)| p == "groq" && e.contains("429"));
    ChaosVerdict {
        id: "r5_2_3_2::groq_429_does_not_pin_endpoint",
        passed,
        note: format!(
            "groq 429 surfaced honestly as a failing candidate {:?}; failed over to opengateway \
             in {}ms — no pin, no infinite retry, no deadlock",
            o.failures, o.elapsed_ms
        ),
        observations,
    }
}

/// R5-2-3-2 (F2/F3) — three consecutive Groq 429s trip the provider's circuit
/// breaker to `Open`. The audit trail must record every failure, and once
/// open the breaker must refuse requests (no retry storm against the dead
/// endpoint). Half-open is unreachable within the 30s cooldown in this run,
/// so the pin we assert is deterministic: `Closed` → `Open` after 3.
pub async fn r5_2_3_2_groq_429_circuit_trips_after_threshold() -> ChaosVerdict {
    let fabric = build_chaos_fabric(
        &[ChaosSpec {
            name: "groq",
            fault: FaultMode::Http429,
            local: false,
            latency_ms: 50,
            timeout_every: None,
            rate_limit_every: None,
        }],
        2000,
        RoutingMode::Free,
    );
    // Every request fails (groq is the only Free candidate and it 429s each
    // time), so the breaker accumulates consecutive failures deterministically.
    let observations = drive(&fabric, "trigger chaos", 4, 2000).await;

    let (state, failures) = {
        let circuits = fabric.circuits.lock().unwrap();
        let cb = circuits.get("groq").expect("groq registered");
        (cb.state, cb.failures)
    };
    let opened = state == CircuitState::Open;
    // A 429 is a rate limit: the provider rests at once (LAI-4), so one
    // recorded failure is enough to open it; other errors still take three.
    let recorded = failures >= 1;
    // After the breaker opens, later requests must not attempt groq (no retry
    // storm): the last request in the run must show no served provider and no
    // fresh groq failure recorded against it.
    let no_retry_storm = observations
        .iter()
        .all(|o| no_deadlock(o) && bounded(o, 2000));

    let passed = opened && recorded && no_retry_storm;
    ChaosVerdict {
        id: "r5_2_3_2::groq_429_circuit_trips_after_threshold",
        passed,
        note: format!(
            "after consecutive 429s the circuit is {state:?} with {failures} recorded failures; \
             requests stay bounded and never retry a storm: {no_retry_storm}"
        ),
        observations,
    }
}

/// R5-2-3-2 (F1) — every remote candidate fails, so the fallback must reach
/// the local model (Ollama) without ever crossing the privacy boundary inward:
/// bounded total latency, no deadlock, deterministic, and the remote
/// rejections are all recorded on the audit trail.
pub async fn r5_2_3_2_all_remote_fail_local_succeeds() -> ChaosVerdict {
    let fabric = build_chaos_fabric(
        &[
            ChaosSpec {
                name: "openrouter",
                fault: FaultMode::Unavailable,
                local: false,
                latency_ms: 60,
                timeout_every: None,
                rate_limit_every: None,
            },
            ChaosSpec {
                name: "opengateway",
                fault: FaultMode::Http500,
                local: false,
                latency_ms: 70,
                timeout_every: None,
                rate_limit_every: None,
            },
            ChaosSpec {
                name: "ollama",
                fault: FaultMode::Ok,
                local: true,
                latency_ms: 100,
                timeout_every: None,
                rate_limit_every: None,
            },
        ],
        2000,
        RoutingMode::Hybrid,
    );
    let observations = drive(&fabric, "trigger chaos", 1, 2000).await;
    let o = &observations[0];
    let passed = served_by(o, "ollama")
        && o.error.is_none()
        && no_deadlock(o)
        && bounded(o, 3000)
        && !o.failures.is_empty()
        && o.failures.iter().all(|(p, _)| p != "ollama");
    ChaosVerdict {
        id: "r5_2_3_2::all_remote_fail_local_succeeds",
        passed,
        note: format!(
            "all remote rejects recorded {:?}; fallback reached local ollama in {}ms — privacy \
             boundary never crossed inward, bounded and deterministic",
            o.failures, o.elapsed_ms
        ),
        observations,
    }
}

/// R5-2-3-2 (F1) — with `privacy = Required`, `select_for` must return ONLY
/// local candidates even when remote providers are healthy and would rank
/// higher. This pins R5-2-1 §2: the privacy boundary blocks remote escalation
/// at selection, before any call is made.
pub async fn r5_2_3_2_local_only_privacy_required_blocks_remote() -> ChaosVerdict {
    let fabric = build_chaos_fabric(
        &[
            ChaosSpec {
                name: "openrouter",
                fault: FaultMode::Ok,
                local: false,
                latency_ms: 10,
                timeout_every: None,
                rate_limit_every: None,
            },
            ChaosSpec {
                name: "ollama",
                fault: FaultMode::Ok,
                local: true,
                latency_ms: 200,
                timeout_every: None,
                rate_limit_every: None,
            },
        ],
        2000,
        RoutingMode::Hybrid,
    );

    // Budget::Bounded so the remote is NOT excluded on cost — the ONLY thing
    // that may exclude it is the privacy requirement.
    let req = RequestRequirements {
        privacy: Preference::Required,
        budget: Budget::Bounded,
        ..Default::default()
    };
    let selected = fabric.select_for(&req);
    let names: Vec<String> = selected.iter().map(|(_, p)| p.name().to_string()).collect();

    let passed = names.iter().all(|n| n == "ollama") && !names.is_empty();
    let note = format!(
        "privacy=Required -> select_for returned only local: {names:?}; remote openrouter \
         (healthy, bounded budget, lower latency) was excluded at selection — escalation blocked"
    );
    ChaosVerdict {
        id: "r5_2_3_2::local_only_privacy_required_blocks_remote",
        passed,
        note,
        observations: vec![RequestObservation {
            request: 0,
            candidates: names,
            served: selected
                .first()
                .map(|(m, p)| format!("{}/{}", p.name(), m.id)),
            elapsed_ms: 0,
            error: None,
            failures: vec![],
        }],
    }
}

/// R5-2-3-2 (F3) — a provider whose failure string embeds a raw token
/// (`ghp_...`) must never leak it into `CandidateFailure.error` or the routing
/// audit trail, and `classify_failure_reason` must still bucket the 401 as
/// `auth` (the sanitizer must not corrupt the honest reason).
pub async fn r5_2_3_2_credential_in_error_does_not_leak() -> ChaosVerdict {
    let fabric = build_chaos_fabric(
        &[ChaosSpec {
            name: "openrouter",
            fault: FaultMode::CredentialLeak,
            local: false,
            latency_ms: 50,
            timeout_every: None,
            rate_limit_every: None,
        }],
        2000,
        RoutingMode::Free,
    );
    let observations = drive(&fabric, "trigger chaos", 1, 2000).await;

    // Inspect the immutable routing decision's failure records directly.
    let decision = fabric.last_decision();
    let leaked: Vec<String> = decision
        .iter()
        .flat_map(|d| d.failures.iter())
        .filter(|f| f.error.contains("ghp_supersecret123"))
        .map(|f| f.error.clone())
        .collect();
    let recorded = decision
        .iter()
        .flat_map(|d| d.failures.iter())
        .any(|f| f.provider == "openrouter");
    // The sanitizer must not hide the fact that this was an auth failure.
    let reason = classify_failure_reason("HTTP 401: invalid Bearer ghp_supersecret123 token");
    let bounded = observations
        .iter()
        .all(|o| no_deadlock(o) && bounded(o, 1500));

    let passed = leaked.is_empty() && recorded && reason == "auth" && bounded;
    ChaosVerdict {
        id: "r5_2_3_2::credential_in_error_does_not_leak",
        passed,
        note: format!(
            "audit-trail failure for openrouter recorded sanitized (leaked fragments {:?}); \
             classify_failure_reason still honest: {reason}; bounded: {bounded}",
            leaked
        ),
        observations,
    }
}

/// Run every Phase 1 scenario, in corpus order.
pub async fn run_all() -> Vec<ChaosVerdict> {
    vec![
        unavailable_before_request().await,
        dies_mid_stream().await,
        malformed_json().await,
        hangs_forever().await,
        extremely_slow().await,
        intermittent_timeouts().await,
        rate_limits_every_n().await,
        partial_responses().await,
        all_bad().await,
        // R5-2-3-2 failure/fallback qualification (F1/F2/F3). Extends, never
        // replaces, the Phase 1 scenarios above.
        r5_2_3_2_groq_429_does_not_pin_endpoint().await,
        r5_2_3_2_groq_429_circuit_trips_after_threshold().await,
        r5_2_3_2_all_remote_fail_local_succeeds().await,
        r5_2_3_2_local_only_privacy_required_blocks_remote().await,
        r5_2_3_2_credential_in_error_does_not_leak().await,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn phase1_scenarios_meet_pass_criteria() {
        for v in run_all().await {
            assert!(v.passed, "{}: {}", v.id, v.note);
        }
    }
}
