# ADR-0004: Public API Stability Tiers

**Status:** Accepted
**Date:** 2026-07-31
**Scope:** `friday-experience` public surface

## Context

`friday-experience` is the integration surface for downstream consumers
(dashboards, monitoring, evaluations). Two distinct properties were being
conflated:

- **Visibility** — a type is `pub` because it crosses a crate boundary.
- **Compatibility** — whether downstream users should treat it as part of the
  long-term contract.

The Sprint 5 learning core had been stable and frozen for some time. The
Sprint 6 observability read models were new and still maturing (e.g.,
`ReplayReport` had just been restructured). Consumers needed to know which
APIs were safe to depend on over time.

## Decision

Annotate every public type in `friday-experience` with an intended stability
level. The annotation lives in the crate root docs
(`services/experience/src/lib.rs`) and is mirrored in
`DecisionIntelligence.md` §9:

- **Stable** — safe to depend on; breaking changes require a documented
  migration.
  - Sprint 5 learning core: `CalibrationOffsets`, `CalibrationStore`,
    `ExperienceStore`, `ExperienceEngine`, `ReputationStore`,
    `ReputationEngine`, `CostModel` (`invalidate` not yet implemented).
- **Experimental** — intentionally public but still maturing; breaking changes
  expected without notice.
  - Sprint 6 observability: `ExplanationEngine`, `DecisionExplanation` +
    factors + `ExplanationReason`, `DecisionTimeline`, `TimelineBuilder`,
    `MetricsEngine` + metric structs + `MetricsReport`, `ReplayEngine` +
    `ReplayReport` + snapshot/difference types.

No internal helpers are exposed; everything `pub` is a deliberate read model
or store.

## Consequences

- **Positive:** Separates visibility from compatibility — downstream users
  know exactly what they may depend on long-term.
- **Positive:** Experimental read models can iterate freely without falsely
  signaling stability.
- **Positive:** The stable/experimental line documents *where* the architecture
  has committed versus where it is still tuning (algorithms, heuristics,
  rendering, comparisons).
- **Negative:** The tiers must be maintained; promotions (experimental →
  stable) and demotions must be deliberate and recorded.
- **Negative:** An annotation is documentation, not enforcement; compliance is
  by convention until a real consumer exists.

## Alternatives Considered

- **Make everything `pub(crate)` until consumed.** Rejected: the crate is the
  integration surface by design; hiding the API would prevent the intended
  use.
- **Make everything `stable`.** Rejected: would falsely commit to churning
  observability read models.
- **Separate crates for stable vs experimental.** Rejected: premature;
  no consumers yet, and the split is cheap to do later.
