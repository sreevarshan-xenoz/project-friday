# ADR-0006: Persistent Cognitive State Architecture (Design)

**Status:** Design — research/design document. Registers the direction and the
open questions. **No implementation decisions are made here.**
**Date:** 2026-08-06
**Scope:** a conceptual API over the existing trust/state surface
(`friday-experience` stores: `Experience`, `Reputation`, `CalibrationStore`;
`ExperienceEngine` procedural memory; `MemoryLayerKind` tiers); world-model
read models; the planner/cognitive layer
**Modifies/Supersedes:** none by itself. It positions a *future* extension;
any eventual change to the frozen contract surface requires its own accepted
ADR and re-validation (frozen-contract #8).
**Predecessor:** ADR-0005 (Windowed Observability) — this ADR's validation
machinery (observation coverage, calibration) is intended to be reused.

## Context

FRIDAY's documents are **memory-centric**: "what happened." `Memory.md`
describes tiers of increasingly durable event storage; `DecisionIntelligence.md`
describes stores (`Experience`, `Reputation`, `CalibrationStore`) derived from
`DecisionRecord`s; ADR-0003 mandates "snapshots, not references" so read models
carry the state of those stores at a decision's point in time.

Two gaps remain:

1. **Memory is not State.** Memory answers *"what happened?"* State answers
   *"what is true right now?"* — a structured assertion like:

   ```
   workspace.editor      = VS Code
   workspace.build       = failed
   workspace.build.reason = missing crate
   workspace.build.confidence = 0.96
   workspace.build.last_verified = 8 s ago
   ```

   Today FRIDAY has no first-class "current state" substrate. The closest
   relatives are reputation/experience stores, but they are operator/capability
   telemetry, not a structured world model, and they do not continuously evolve
   from sensors independent of conversations.

2. **The trust stores are one-dimensional for state purposes.** `Reputation`
   already pairs `reliability` (frequency-like) with `confidence`
   (evidence-strength), and `CalibrationStore` keeps that confidence honest.
   But for *arbitrary state assertions* there is no explicit, orthogonal
   frequency/confidence pair, no evidence/source attribution, and no decay
   policy per domain.

A proposed name surfaced repeatedly in roadmap discussion: "Belief Engine."
The risk is that it reads as a **new service**, which would duplicate the
existing stores. This ADR treats it as a **conceptual API**, not a runtime
module.

## Decision (design direction — not yet made)

### 1. "Belief Engine" is an abstraction, not a service

```
Belief Engine  =  unified abstraction over  Experience · Reputation ·
                                            Calibration · Evidence
```

No new runtime component. Existing stores remain the persistence layer; the
"engine" is the shared vocabulary and evaluation rules over them.

### 2. State is a derived read model, never a second source of truth

Persistent cognitive state must obey frozen-contract #4: **"History is
immutable. Intelligence is derived."** Every state assertion is a *projection*
over (immutable history + current sensor input) and must remain **recomputable**
from those inputs. State is therefore never written authoritatively; it is
re-derived, and it decays unless re-verified. This preserves ADR-0003's
"pure projection" guarantees and ADR-0005's windowed-observability machinery.

### 3. Four domain models with independent update dynamics — not one graph

| Model | Contents | Refresh cadence | Intended maintainer |
| ----- | -------- | --------------- | ------------------- |
| Environment | desktop, filesystem, network, browser | every second (sensors) | M7-style world-model/vision pipeline |
| User | goals, habits, preferences, attention | every interaction | context builder |
| Cognitive | plans, hypotheses, predictions, beliefs | every reasoning cycle | planner + calibration |
| Knowledge | facts, skills, concepts, procedures | only through consolidation | experience/learning layer |

Forcing these into a single synchronized graph creates cross-domain
synchronization problems at the slowest cadence. Instead, each domain evolves
independently and shares through the **Global Workspace** (roadmap ACOS
direction) via queries and projections, with no backward dependency edges
(frozen-contract #2).

### 4. Orthogonal frequency/confidence for state assertions

For state assertions, adopt the NARS-style separation the roadmap already
gestures at:

```
frequency = 0.98     how often this has been true
confidence = 0.63    how much evidence supports it
```

Orthogonal: a single fresh observation is `frequency = 1.0,
confidence = 0.05` — true once, barely evidenced. This prevents the system
from becoming overconfident after minimal experience. For the existing
trust stores this is a **generalization**: `Reputation` already carries
`reliability` (frequency-like) and `confidence` (evidence-strength); the
extension makes the pair explicit and applies it to arbitrary state
assertions, with first-class `evidence`, `source`, `last_verified`, and
per-domain `decay`.

### 5. A storage-independent State contract

Defined early as a contract only — deliberately free of any storage or
backend detail (an in-memory graph, `redb`, PostgreSQL, Neo4j are all
implementation concerns; the contract does not change):

```rust
struct State {
    id: String,               // stable identity of the assertion
    domain: Domain,           // environment | user | cognitive | knowledge
    value: serde_json::Value, // structured value being asserted
    frequency: f64,           // how often this has been true        [0, 1]
    confidence: f64,          // how much evidence supports it       [0, 1]
    evidence_sources: Vec<SourceRef>, // refs into immutable history / sensors
    last_verified: DateTime<Utc>,     // when this was last confirmed
    decay_policy: DecayPolicy,        // per-domain decay / re-verification cadence
    prediction_model: Option<PredictionRef>, // how this state projects forward
}
```

The contract carries no conversation text: state lives independently of
conversations and evolves from sensors and derived projections.

### 6. State Lifecycle

Every state object follows a deterministic lifecycle — it is not a static
record:

```
Observation          (sensor / derived projection)
    ↓
Evidence             (referenced, never duplicated — points at immutable history)
    ↓
Belief Update        (revises frequency / confidence)
    ↓
Current State        (what agents and the planner read)
    ↓
Prediction           (projects forward via prediction_model)
    ↓
Verification         (re-checked against reality / actuals)
    ↓
Reinforcement / Revision / Decay
```

`Reinforcement` and `Revision` are exactly where `CalibrationStore` and
`Reputation`'s decay machinery already live — the abstraction **routes to**
those existing stores rather than reimplementing them. `Verification` feeds
calibration convergence (the ADR-0005 / VP-002-style protocol); `Decay` is
governed by `decay_policy` and `last_verified` staleness.

## Consequences (expected, not yet measured)

- **Positive:** FRIDAY gains a structured "what is true right now" substrate
  independent of conversation text; sensors drive state continuously.
- **Positive:** confidence/calibration discipline (existing `CalibrationStore`
  philosophy) extends to the world model, not just provider reputation.
- **Positive:** domain separation keeps refresh rates independent and avoids a
  monolithic world-graph bottleneck.
- **Positive:** staying a derived read model preserves frozen-contract #4 and
  makes the whole substrate re-derivable from the immutable ledger.
- **Negative:** the "recomputable state" rule means state has a cost to rebuild
  after restart — it is a projection, not a cache, so deep state must be
  re-derived (at CPU cost) rather than loaded.
- **Negative:** decay/last-verified policy adds lifecycle machinery to stores
  that are currently append/aggregate only.
- **Negative:** if implemented naively it could duplicate `Reputation`'s decay
  machinery — the abstraction must *route to* the existing stores, not reimplement
  them.

## Alternatives Considered

- **Add a "Belief Engine" service.** Rejected — duplicate persistence, violates
  modularity, and splits trust state across two owners.
- **One monolithic world graph.** Rejected — four refresh cadences under one
  graph force the slowest-consolidating domain (Knowledge) to gate the fastest
  (Environment), and make per-domain validation impossible.
- **Confidence as a single calibrated scalar (status quo).** Kept for existing
  stores; extended only where state assertions need the frequency/confidence
  pair. The scalar is not wrong — it is insufficient for state.
- **State as authoritative storage (write-through "facts").** Rejected —
  violates "History is immutable. Intelligence is derived." and silently turns
  stale assertions into truth.

## Validation gate (deferred — no implementation now)

- **No implementation decisions** are made by this ADR and no code changes
  result from it.
- The gate for implementation is: (1) ADR-0005 resolved from run-005 evidence —
  **met 2026-08-06 (accepted)**; (2) an **interface-impact analysis** — does
  the abstraction *extend* existing stores/interfaces or *change* them? Any
  change to a frozen contract requires its own accepted ADR.
- The eventual validation protocol (VP-002-style) reuses the windowed
  observability machinery from ADR-0005: it must demonstrate **calibration
  convergence** for frequency/confidence assertions under continuous
  observation (coverage ≥ 99%) — i.e. predicted confidence matches observed
  truth rates over a defined window.

## Open questions (for the design phase, not answered here)

- Where do state projections live relative to `ExperienceEngine`/read-model
  bridge projections, and how are they versioned?
- Which existing store first gains the explicit frequency/confidence pair —
  `Reputation` (already closest) or a new assertion-specific record type?
- Per-domain decay and last-verified policy (who re-verifies, how often).
- How JEPA / latent world-model integration (future) maps onto the four-domain
  split without re-architecture — the name "Persistent Cognitive State" is
  intentionally broad enough to absorb it.
