# Chaos Lab Findings — Living Document

Each entry records one finding from the chaos lab. Fields:

- **Finding** — what the scenario exposed.
- **Impact** — why it matters for the inference scheduler.
- **Decision** — what we chose to do (or not do) and why.
- **Status** — `Fixed` / `Observing` / `Accepted` / `Open`.
- **Follow-up** — the next step or the trigger that will change the status.

---

## Phase 1 — Provider failures (committed `20443c0`)

Scenario matrix: `unavailable_before_request`, `dies_mid_stream`, `malformed_json`, `hangs_forever`, `extremely_slow`, `intermittent_timeouts`, `rate_limits_every_n`, `partial_responses`, `all_bad`. All 9/9 scenarios meet pass criteria.

### 1.1 Hung provider blocks the whole scheduler

| Field | Value |
| --- | --- |
| **Finding** | A provider that accepts a request then never returns (no per-request timeout) stalls the scheduler indefinitely. Failover can only start after the blocked attempt gives up. |
| **Impact** | Worst-case end-to-end latency becomes unbounded; one stuck provider kills every request that routes through it. |
| **Decision** | Added `InferenceFabric.request_timeout: Option<Duration>`. `None` preserves pre-chaos behavior; when set, it is enforced per-attempt in both `ModelClient::complete` and `complete_tier` via a shared `run_attempt()` helper. |
| **Status** | `Fixed` |
| **Follow-up** | Decide a sensible default timeout value for production (and whether it should be per-provider). Consider streaming keep-alive handling. |

### 1.2 Truncated responses are served as success

| Field | Value |
| --- | --- |
| **Finding** | `partial_responses` / `dies_mid_stream` deliver an incomplete body with a success code; the scheduler treats it as a usable response. |
| **Impact** | Content integrity is a correctness issue, not a scheduling one. Fixing it in the scheduler would conflate "did a provider produce a response?" with "is the response acceptable?". |
| **Decision** | Accepted for now. Kept out of the scheduler; belongs to a separate Verification subsystem that judges response acceptability. |
| **Status** | `Accepted` |
| **Follow-up** | Build the verification layer; wire a "reject + failover on unacceptable body" path once it exists. |

### 1.3 Transient failure permanently demotes a provider

| Field | Value |
| --- | --- |
| **Finding** | One transient failure demotes a provider for the rest of the run — there is no re-probe or recovery window (`intermittent_timeouts`, `rate_limits_every_n`). |
| **Impact** | Healthy-but-flaky providers are excluded long after they recover, reducing available capacity. |
| **Decision** | Observe before changing. Instrumented health changes (structured `[Provider Health]` trace + JSONL at `.fabric/health-changes.jsonl`: Provider / Health before→after / Reason / Decision) and collect ~1 week of telemetry before deciding on hysteresis or a re-probe policy. Decisions logged: `recovered` (score bump), `degraded` (failure), `demoted` (circuit opened), `probing` (re-probe after cooldown). |
| **Status** | `Observing` |
| **Follow-up** | Telemetry is flowing; revisit with a week of data, then decide on hysteresis. |

### 1.4 Failover latency accumulates across failed candidates

| Field | Value |
| --- | --- |
| **Finding** | When the primary fails, failover latency is the sum of the failed candidates' individual deadlines (`extremely_slow`, `hangs_forever`). |
| **Impact** | The more unhealthy providers are tried, the slower every request becomes; worst case is the full sum before any success. |
| **Decision** | Accepted for now; request timeout caps the contribution of each failed attempt. |
| **Status** | `Accepted` |
| **Follow-up** | Revisit with concurrent or hedged requests, and/or fail-fast on known-dead providers from the demotion state. |

---

## Phase 2 — Health persistence corruption

Status: `Complete` (9/9 scenarios: `corrupt_json`, `impossible_values`, `missing_file`, `truncated_file`, `empty_file`, `older_schema`, `newer_schema`, `duplicate_entries`, `partial_write`). Recovery contract verified: never panic, never poison scoring, fall back to defaults with a warning, and `persist_health` restores a complete valid file afterward.

### 2.1 Impossible reliability values poison scoring

| Field | Value |
| --- | --- |
| **Finding** | A persisted reliability outside `0.0..=1.0` (5.0, -0.5) was loaded verbatim into `health_scores`, corrupting the ranking sort. NaN is not representable in JSON but non-finite handling is defensive. |
| **Impact** | One bad value silently reorders every routing decision. |
| **Decision** | `load_health` sanitizes: finite values are clamped to `[0,1]`, non-finite values reset to the 1.0 default, with a warning naming the provider and the impossible value. |
| **Status** | `Fixed` |
| **Follow-up** | None. |

### 2.2 Unreadable health files failed silently

| Field | Value |
| --- | --- |
| **Finding** | Corrupt, truncated, empty, and older-schema files all failed `serde_json` parsing with no log — the operator was blind to a full health reset. |
| **Impact** | Silent state loss on every bad boot. |
| **Decision** | `load_health` now warns with the path and parse error, then falls back to default health (1.0 / circuit Closed). |
| **Status** | `Fixed` |
| **Follow-up** | None. |

### 2.3 Persistence was not atomic

| Field | Value |
| --- | --- |
| **Finding** | `File::create` truncated the previous snapshot before writing. A crash mid-write destroyed the last good health state, leaving a torn file for the next boot. |
| **Impact** | Crash during persist = guaranteed health reset (and a torn file until 2.2). |
| **Decision** | `persist_health` writes to `<path>.tmp` then renames (atomic on the same filesystem). A stale temp is harmless and gets overwritten. Verified by `chaos2.partial_write`: load falls back, persist restores a valid file, no stale temp remains. |
| **Status** | `Fixed` |
| **Follow-up** | None. |

### 2.4 Newer schemas are forward-compatible, older schemas fail closed

| Field | Value |
| --- | --- |
| **Finding** | Files with extra fields (version, timestamps, schema URL) load fine — unknown fields are ignored, known fields exact. Files missing required fields reject the whole file → defaults. |
| **Impact** | Newer builds can upgrade and read; older files never half-load (no partial state). |
| **Decision** | Accepted — default serde behavior is the correct contract for both directions. |
| **Status** | `Accepted` |
| **Follow-up** | If a schema version field is ever needed, it is forward-compatible to add (extra fields are already ignored). |

### 2.5 Duplicate provider entries: last entry wins

| Field | Value |
| --- | --- |
| **Finding** | A provider listed twice loads deterministically — the last entry wins (matches `HashMap::insert`), health and circuit state included. No panic. |
| **Impact** | Not a bug; a well-defined tie-break for a corrupt/merged file. |
| **Decision** | Accepted and locked in by `chaos2.duplicate_entries`. |
| **Status** | `Accepted` |
| **Follow-up** | None. |

### 2.6 Missing file is a normal first boot

| Field | Value |
| --- | --- |
| **Finding** | No file → silent defaults (1.0 / Closed), no warning. Routing works immediately. |
| **Impact** | Correct; `NotFound` is expected state, not corruption. |
| **Decision** | Accepted. Warnings are emitted only for files that exist but are wrong. |
| **Status** | `Accepted` |
| **Follow-up** | None. |

### 2.7 Persistence path was hardcoded and untestable

| Field | Value |
| --- | --- |
| **Finding** | `load_health`/`persist_health` wrote to a hardcoded cwd-relative `.fabric/provider_health.json`, so corruption could not be injected without touching the working directory. |
| **Impact** | The health persistence layer was untestable in isolation. |
| **Decision** | Added configurable `InferenceFabric.health_state_path` (defaults to the same path). Chaos scenarios point it at a controlled temp file. |
| **Status** | `Fixed` |
| **Follow-up** | Consider a state-dir/env override if multi-instance deployments ever need distinct state files. |
