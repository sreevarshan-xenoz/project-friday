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
  Confidence**.
- `resolves(available_caps) -> bool` — would the step actually work *now*? Steps that
  do not depend on another live provider (Retry, Restart, Wait, Restore, Cleanup,
  Elevate, Ask, Escalate) always resolve; reroute steps only resolve if their target
  capability is currently `available()` in the registry.

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
Simulation Engine predicts will **fail**, classifies the `FailureCategory` and calls
`recover()`. If `recovered`, the task is **not** counted against **MTBF** (the
headline metric). The dashboard reports:

- **Recovery Success** = resolved / attempts.
- **Recovery Confidence** (mean) over resolved recoveries.
- **Recovery by Subsystem** = per-capability-group coverage % + mean confidence.
- **MTBF Trend** across `benchmarks/<date>.json` historical day snapshots.

## Acceptance criteria (met)

- [x] `friday-recovery` builds clean (clippy/fmt/tests), `FailureCategory` +
      `RecoveryAction` + `RecoveryGraph` implemented.
- [x] Benchmark reports Recovery Success, mean Recovery Confidence, per-subsystem
      recovery %, and an MTBF trend from historical snapshots.
- [x] A capability lacking a recovery policy yields `recovered = false` → MTBF drops
      (regression detector), not a silent crash.
