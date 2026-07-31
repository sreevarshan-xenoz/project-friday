# ADR-0001: Freeze Core 1.0

**Status:** Accepted
**Date:** 2026-07-31
**Scope:** `services/goal-execution`

## Context

FRIDAY's execution core (Core 1.0, `services/goal-execution`) was built and
validated first: 199 tests, clippy clean, behavior frozen. Sprints 5 and 6
introduced adaptive decision intelligence in new crates
(`friday-plugin-runtime`, `friday-experience`).

Early in the project, the core was the fastest-changing thing. Once the
adaptive loop and observability layer landed, the cost of reshaping the core
began to exceed its benefit: every change risked destabilizing validated
behavior, and no feature so far required a core change.

## Decision

Freeze Core 1.0. Zero modifications are allowed to
`services/goal-execution`. All new work lives in the newer crates or at their
boundaries.

## Consequences

- **Positive:** A stable foundation. New features integrate at defined
  boundaries rather than rewriting validated behavior.
- **Positive:** Attention shifts from structural redesign to algorithmic
  improvement and real-world validation.
- **Negative:** The core cannot absorb future improvements directly; those
  must be expressed at the boundary (e.g., adapters, wrapping crates).
- **Risk:** Boundary impedance — if a future need genuinely requires core
  change, the freeze must be revisited deliberately, not quietly eroded.

## Alternatives Considered

- **Refactor the core in place.** Rejected: high risk to validated behavior;
  no concrete feature required it.
- **Version-fork the core.** Rejected: premature; no consumers need two
  versions yet.
- **No freeze.** Rejected: the architecture was becoming a moving target;
  iteration velocity on the platform depended on a fixed substrate.
