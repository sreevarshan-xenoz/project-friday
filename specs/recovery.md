# Recovery Specification (Reliability Sprint)

**Status:** ✅ implemented (`friday-recovery` + wired into `friday-benchmark`).
**Spec-first:** the design below is what was built and is now measured.

## Why

The north-star metric of Alpha Qualification is **graceful degradation**, not
feature count. When a provider dies (Chaos Mode), FRIDAY must continue via a
*deterministic, auditable* recovery path rather than crashing. Recovery is modelled
at the **action level** (`Browser.Navigate`), not the service level, so a single
capability can have several distinct recovery trees depending on *why* it failed.

## Taxonomy

```rust
pub enum FailureCategory {
    Network, Capability, Permission, Resource,
    Provider,      // owning provider offline/unavailable (what Chaos Mode induces)
    Timeout, Planner, Simulation, Execution, Unknown,
}
```

`FailureCategory::from_kind(kind)` maps a Simulation Engine validation issue kind
onto a category (`unavailable_service` → `Provider`, `permission` → `Permission`,
`state`/`service` → `Simulation`, `capability` → `Capability`).

## Recovery actions

```rust
pub enum RecoveryAction {
    Retry { attempts: u32 },
    RestartService { provider: String },
    WaitAndRetry { ms: u64 },
    UseProvider { capability: String },     // reroute to another provider, same cap
    UseAlternateCapability { capability: String }, // achieve goal via different cap
    RestoreSession,                         // restore killed shell/agent state
    CleanupAndRetry { cleanup: String },    // clear blocking artifact (git lock)
    SuggestElevation,                       // UAC / sudo prompt
    AskUser,                                // human-in-the-loop
    Escalate,                               // higher authority / operator
}
```

Every action carries two semantic methods:

- `confidence() -> f64` — base belief the step resolves the failure.
  Reroute/alternate (0.85–0.90) > restart/restore/cleanup (0.80) > retry/wait (0.70)
  > elevation (0.60) > ask (0.95) > escalate (0.90). This is the **Recovery
  Confidence** (a prior; see Learning below).
- `resolves(category, available_caps) -> bool` — would the step actually work *now*?
  Critically, **not every action can paper over every failure**:

  | Recovery Action        | Can auto-resolve?                              |
  |------------------------|-----------------------------------------------|
  | `UseAlternateCapability` | **Yes** — iff target capability is available |
  | `UseProvider`            | **Yes** — iff target capability is available |
  | `Retry`                  | **Conditional** — only transient categories   |
  | `WaitAndRetry`           | **Conditional** — only transient categories  |
  | `RestartService`         | **Conditional** — only transient categories   |
  | `RestoreSession`         | **Conditional** — only transient categories   |
  | `CleanupAndRetry`        | **Conditional** — only transient categories   |
  | `SuggestElevation`       | **No** — needs human / creds                 |
  | `AskUser`                | **No** — human-in-the-loop                    |
  | `Escalate`               | **No** — higher authority                     |

  "Transient" categories are `Network`, `Timeout`, `Resource` — ones where a bounded
  retry/wait/restart plausibly helps (e.g. an HTTP 429 you can wait out, a stale lock
  you can clear). **A dead provider is NOT healed by retrying it**: `Retry`/`Restart`
  on a `Provider`-offline failure do **not** resolve; only an alternate provider or
  capability does. This is what stops the benchmark being "gamed".

## Recovery Graph

```rust
pub struct RecoveryPolicy {
    pub by_category: HashMap<FailureCategory, Vec<RecoveryAction>>, // tried in order
}
pub struct RecoveryGraph {
    pub policies: HashMap<String /* action id */, RecoveryPolicy>,
}
```

`RecoveryGraph::recover(action, category, available_caps) -> RecoveryResult`:

- Walks the policy's ordered steps for `category`.
- Returns the **first step that `resolves()`** with the current availability, its
  `confidence()`, and a `needs_approval` flag (`confidence < APPROVAL_THRESHOLD`
  (0.5) or the step is `AskUser`).
- If **no** step resolves → `recovered = false`, `needs_approval = true`,
  `chosen = Escalate`. This is the regression signal: a new capability without a
  recovery policy fails here and drags **Chaos MTBF** down.

Policies are pure data: `RecoveryGraph` derives `Serialize`/`Deserialize`, so the
graph can be authored in Rust (defaults) **or** loaded from JSON and overridden.

## Default policies (baseline)

| Action | Category | Plan (in order) |
|--------|----------|-----------------|
| `browser.navigate` | Provider | `UseAlternateCapability(windows.navigate)` → `RestartService(browser)` → `AskUser` |
| `filesystem.*` | Provider | `Retry{2}` → `AskUser` |
| `filesystem.*` | Permission | `SuggestElevation` → `AskUser` |
| `terminal.run` | Provider | `RestartService(terminal)` → `RestoreSession` → `Retry{1}` → `AskUser` |
| `git.*` | Provider | `WaitAndRetry{500}` → `CleanupAndRetry("stale lock")` → `Retry{1}` → `AskUser` |
| `inference.complete` | Provider | `Retry{2}` → `Escalate` |
| `windows.open` | Provider | `Retry{1}` → `AskUser` |

## How it is measured

`friday-benchmark` builds `RecoveryGraph::with_defaults()` and, for every task the
Simulation Engine predicts will **fail**, classifies the `FailureCategory`, calls
`recover_with()` (blending learned confidence), and records the outcome back into a
`ConfidenceModel`. **Recovery never hides failure** — the dashboard reports four
*independent* metrics:

- **Raw MTBF** — mean tasks between failures, counting *every* interruption. Recovery
  is ignored entirely; this is the engineering reality.
- **User Success Rate** — fraction of tasks the user ultimately got (predicted
  success **or** recovered). Can be high while Raw MTBF is poor.
- **First-Pass Success** — fraction completed with *no* recovery at all.
- **Recovery Effectiveness** — of recovery attempts, the fraction that resolved.
- **Recovery Confidence** (mean, learned) + **Recovery by Subsystem** coverage %.
- **Reliability Trend** across `benchmarks/<date>.json` historical day snapshots
  (Normal/Chaos Raw MTBF + Chaos User Success), with a `ConfidenceModel` carried
  forward so confidence learns across runs.

A capability lacking a recovery policy (or whose policy can't resolve the category)
yields `recovered = false` → Raw MTBF stays low and Recovery Effectiveness drops —
that is the honest regression signal, not a silent crash.

## Learning (Outcome → Experience → Updated Confidence)

`ConfidenceModel` records `(action, category, step) → (trials, successes)` and yields
an `effective()` confidence: a shrinkage estimator equal to the base prior with no
data that converges to the observed success rate as trials accumulate. So
`Restart Browser` predicted 90% but observed 73% will, after enough executions,
automatically read ~73%. The model is persisted in each `benchmarks/<date>.json` and
seeded from the previous day, so confidence compounds over time.

## Runtime integration (Shadow Mode — not yet live)

The graph is currently exercised by the benchmark only. The intended path to the
real `AgentKernel` is **shadow mode**, not immediate live recovery:

```
Benchmark → Replay → AgentKernel → Shadow Mode → enable live
```

During real executions the `RecoveryGraph` runs **without affecting execution**,
recording only *"I would have chosen …"*. Compare actual outcome vs the graph's
decision; once they consistently agree, flip on live recovery. This is how production
routing/recovery logic is introduced safely.

## Acceptance criteria (met)

- [x] `friday-recovery` builds clean (clippy/fmt/tests); `FailureCategory`,
      `RecoveryAction` (honest `resolves`), `RecoveryGraph`, `ConfidenceModel`.
- [x] Benchmark reports Raw MTBF, User Success Rate, First-Pass Success, Recovery
      Effectiveness, learned confidence, per-subsystem coverage, and a Reliability
      Trend from historical snapshots.
- [x] Killing a provider (Chaos Mode) honestly lowers Raw MTBF; recovery is shown as
      a *separate* number, never folded into MTBF.
