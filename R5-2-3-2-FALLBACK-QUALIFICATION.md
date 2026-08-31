# R5-2-3-2 — Failure/Fallback Qualification (FROZEN)

**Status:** FROZEN qualification slice for the R5-2-3-1 integration. **No architectural change.** Verifies that the failure/fallback path of `InferenceFabric` continues to honor the R5-2-1 hierarchy (local → free-remote → subscription → paid) after the Copilot account-auth + Groq free-remote wiring was added in R5-2-3-1.
**Inputs:** frozen `4fa848f` (R5-2-3-1), frozen `19b9173` (R5-2-3-0), frozen `2af4b31` (R5-2-1 architecture).
**Starting point:** verified clean `4fa848f`, working tree clean. This doc does **not** reopen R5-2-3-1.
**Sequence:** R5-2-3-2 qualifies → R5-2-4 Context Gate → R5-2-5 OS-level reasoning (per R5-2-1 §6).

---

## 1. What "fallback qualification" means here

Per R5-2-3-0 §4:

> **Fallback (R5-2-3-2):** free-remote (Groq) → user subscription (Copilot) → local (privacy=Required) uses the existing health-score + circuit-breaker + requirement-driven `select_for`; no pinning, no hard-coded dead endpoints (free tiers churn).

R5-2-3-2 is **not** a new architecture. The fallback architecture (health-score, circuit-breaker, requirement-driven `select_for`) is already implemented in `services/intelligence/src/providers.rs` and was already qualified by chaos Phase 1 (`services/provider-qual/src/chaos.rs`) and Phase 2 (`services/provider-qual/src/chaos2.rs`).

**What R5-2-3-1 changed that R5-2-3-2 must re-qualify:**

1. **New "Subscription" provider class member** — `CopilotCliProvider` (shells to official headless `copilot` CLI; account-auth, opt-in via `COPILOT_CLI_ENABLED=true|1`). The Copilot slot in `select_candidates(RoutingMode::Quality)` is new code; its behavior under failure must not break the existing fallback guarantee.
2. **Groq reclassification** — `is_free_provider("groq")` now returns `true` (per R5-2-3-0 §4). `select_candidates(RoutingMode::Free)` now selects Groq.
3. **Live Groq catalog** — `refresh_groq_catalog` replaces the hard-coded free model at runtime. If the catalog endpoint is down, fallback must still work via the non-blocking boot fallback (`llama-3.3-70b-versatile`).
4. **Credential hygiene** — `copilot_meaningful_stderr` strips `COPILOT_*` control-plane lines so tokens never surface in `CandidateFailure` strings. The same hygiene must apply to the fabric's other paths (no key in `error` fields, no key in `tracing` macros for `classify_failure_reason`).

---

## 2. The three contracts R5-2-3-2 pins

| # | Contract | Mechanism tested |
|---|---|---|
| **F1** | **Fallback never crosses the privacy boundary upward.** When `privacy=Required` is set, a remote failure must not silently retry against a different remote class. | `select_for(req)` with `Preference::Local` + all-remote-fails → Local-only candidates survive; remote class skipped, no escalation. |
| **F2** | **Copilot is reachable only under Quality/Hybrid and only when opted-in.** It must never appear under Free/Local, and it must not be tried when `COPILOT_CLI_ENABLED` is unset. | `select_candidates(RoutingMode::Free/Local)` excludes Copilot; opt-in env var is the only gate. |
| **F3** | **Credential hygiene survives the fallback path.** A 401/429/500 from any provider surfaces as a sanitized `CandidateFailure`. No raw API key fragment in `error`, in `classify_failure_reason`, or in the routing-decision audit trail. | Mock provider that returns `"Bearer ghp_xxx ... 401 Unauthorized"` → `error` field contains no `ghp_` substring; `classify_failure_reason` returns `"auth"`. |

If any of F1/F2/F3 regresses, R5-2-3-1 is broken at the fallback layer and the slice is reopened (without touching R5-2-3-1's correctness — that already passes 81 lib + 6 integration tests).

---

## 3. What R5-2-3-2 adds (minimal, no churn)

### 3.1 F3 credential hygiene hardening (`providers.rs`)

The one piece of `4fa848f` that **cannot** verify-clean is F3: only the Copilot-specific `copilot_meaningful_stderr` strip existed, so a generic provider that embeds `Bearer ghp_...` in its failure string would leak that fragment into `CandidateFailure.error`, the routing audit trail, the returned `Err`, and telemetry. R5-2-3-2 adds **one** minimal, in-scope hardening:

- `pub fn sanitize_candidate_error(err: &str) -> String` — strips raw bearer tokens and known credential prefixes (`ghp_`, `gho_`, `github_pat_`, `sk-`, `sk_`, `gsk_`) → `[REDACTED]`. Applied at the four candidate-error recording sites and to the final `All candidates failed. Last error:` string. `classify_failure_reason` runs on the raw string, so the honest `auth`/`timeout`/`rate_limit` bucket is preserved.
- `classify_failure_reason` widened to `pub` (returns `&'static str`); both it and `sanitize_candidate_error` are re-exported from `lib.rs` so the boundary/chaos tests can assert on them.

This is failure-path hygiene, not behavior or architecture — the fallback/selection logic is untouched.

### 3.2 Boundary tests in `services/intelligence/tests/intelligence_tests.rs`

Extend the existing suite (no new files) with 5 R5-2-3-2 boundary tests, all using the existing `TestProvider` mock harness and `InferenceFabric::select_candidates` / `select_for` / `last_decision`:

| Test | Contract | What it pins |
|---|---|---|
| `fallback_skips_copilot_when_optout` | F2 | Without `COPILOT_CLI_ENABLED`, the real registration path (`default_from_env`) does not register Copilot; with it set, Copilot is registered (R5-2-3-1 opt-in gate). |
| `fallback_routes_copilot_only_under_quality_or_hybrid` | F2 | Copilot must never appear under `RoutingMode::Free` or `RoutingMode::Local`, and must appear under `Quality`/`Hybrid` (R5-2-1 §2 hierarchy slot). |
| `fallback_groq_is_free_provider` | R5-2-3-0 §4 | `is_free_provider("groq") == true`; observable via `select_candidates(Free)` selecting Groq. |
| `fallback_classify_failure_reason_does_not_contain_key` | F3 | `classify_failure_reason("Bearer ghp_secret123 ... 401 Unauthorized") == "auth"` and the output contains no `ghp_` fragment. |
| `fallback_does_not_leak_credential_in_decision_audit` | F3 | A 401 from a provider whose `complete()` returns `"unauthorized: Bearer ghp_secret123 ..."` produces a `RoutingDecision` whose `failures[i].error` does NOT contain `ghp_secret123`. |

### 3.3 Chaos scenarios in `services/provider-qual/src/chaos.rs`

Add a new tagged scenario group (`r5_2_3_2::*`) to the existing `chaos.rs` file (no new files). Five scenarios:

| Scenario | Failure mode | What it qualifies |
|---|---|---|
| `r5_2_3_2::groq_429_does_not_pin_endpoint` | Groq returns 429 every call; OpenGateway answers | Fabric must surface the 429 honestly as a `CandidateFailure` (`reason="rate_limit"`) and fail over to OpenGateway within the bounded deadline. No infinite retry, no pin to the dead endpoint. |
| `r5_2_3_2::groq_429_circuit_trips_after_threshold` | Groq returns 429 three times consecutively | After 3 failures, Groq's circuit breaker must open (`Closed` → `Open`); the audit trail must show the failures and later requests must not retry the dead endpoint. (Half-open is unreachable within the 30s cooldown in a short run.) |
| `r5_2_3_2::all_remote_fail_local_succeeds` | All remote (OpenRouter, OpenGateway) fail; Local Ollama succeeds | Fallback reaches Local without crossing the privacy boundary (F1). No deadlock, bounded total latency, deterministic. |
| `r5_2_3_2::local_only_privacy_required_blocks_remote` | `Preference::Local` (Required) + all-remote-available | `select_for(req)` returns only Local candidates; the fabric never selects a remote, even if it would otherwise rank higher — the privacy boundary blocks escalation at selection. Pins R5-2-1 §2 hierarchy. |
| `r5_2_3_2::credential_in_error_does_not_leak` | Mock returns `"HTTP 401: invalid Bearer ghp_supersecret123 token"` | `CandidateFailure.error` must not contain the key fragment; `classify_failure_reason` must still bucket as `"auth"`. Pins F3 at the chaos layer (regression guard). |

These five are added alongside the existing nine Phase 1 scenarios (`chaos::run_all` is extended, not replaced — old scenarios stay green). `MockProvider` gains one `FaultMode::CredentialLeak` variant to drive the F3 leak deterministically; `MockProviderV2` is untouched.

### 3.4 What R5-2-3-2 does **not** do

- **No architectural churn.** R5-2-1's `InferenceFabric` abstraction stays untouched; selection/fallback logic is unchanged.
- **No new chaos module.** Reuses the existing `chaos.rs` `ChaosSpec` + `MockProvider` + `build_chaos_fabric` + `drive` infrastructure.
- **No new providers.** R5-2-3-2 qualifies the wiring of R5-2-3-1; it does not add Gemini, OpenRouter, or anything else.
- **No unrelated workspace reformatting.** R5-2-3-2 does not run `cargo fmt` over crates it does not touch (the previous failed attempt did and polluted `mobile/`).
- **No Context Gate work.** R5-2-4 owns that.

---

## 4. The "swap rule" carried from R5-2-3-0

If any R5-2-3-2 scenario fails:

1. **First:** confirm the failure is in R5-2-3-1's wiring, not in the chaos harness or mock. Re-run with `cargo test -- --nocapture` and inspect the `ChaosVerdict.note`.
2. **Second:** decide if it's a wiring bug (R5-2-3-1 fix-up commit) or a contract gap (R5-2-3-2 enhancement). Architecture stays untouched.
3. **Third:** if Copilot's account-auth wiring is the blocker (e.g. the `copilot` CLI bundling decision gets revisited), substitute the next-best account-auth seam verified in R5-2-3-0 (Codex device-code) without reopening R5-2-3-0's verification.
4. **Fourth:** if Groq's free-tier reclassification is the blocker, drop Groq from `is_free_provider` (and the live catalog) — the existing free-remote floor (OpenRouter/SambaNova/OpenGateway/GitHub Models) absorbs the slack per R5-2-2.

---

## 5. Acceptance boundary for R5-2-3-2

- [x] F1, F2, F3 contracts pinned (§2).
- [x] 5 boundary tests added to `intelligence_tests.rs`; 81 existing lib tests + 6 existing integration tests still pass.
- [x] 5 chaos scenarios added to `chaos.rs`; 9 existing Phase 1 scenarios + 4 existing Phase 2 scenarios still pass.
- [x] `cargo test -p friday-intelligence -p friday-provider-qual` green.
- [x] `cargo clippy -p friday-intelligence -p friday-provider-qual --all-targets -- -D warnings` clean.
- [x] `cargo fmt --check` clean for every file R5-2-3-2 touches (no unrelated workspace reformatting).
- [x] Committed as this freeze point.

**The only production change vs `4fa848f` is the F3-mandated `sanitize_candidate_error` hardening in `providers.rs` (+ the `pub` widening of `classify_failure_reason` and its `lib.rs` re-export).** Everything else — the fallback architecture, hierarchy, selection, and failover logic — stays exactly as frozen in R5-2-3-1.

**Next:** R5-2-4 — Context Gate (per-use, itemized consent + revoke + disclosure; this is the OS security boundary between context and remote providers).
