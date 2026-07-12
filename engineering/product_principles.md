# Product Principles

Product philosophy for PROJECT FRIDAY. This is not architecture (`docs/`) and not
engineering process (`engineering/coding_standard.md` and friends). It is the set
of product rules that should outlive any particular feature.

These principles are treated like the frozen architecture: stable until benchmark
evidence gives a reason to change them.

## The enemy

> **FRIDAY will not become another chat interface with tools attached.**

The runtime, the event bus, the planner, the recovery graph, and the qualification
framework all point to FRIDAY being an **operating-system intelligence layer**, not
a chatbot. The vision already describes FRIDAY as a desktop intelligence platform
rather than an assistant. This philosophy is made explicit here so it can be
defended in review.

## Principle 1 — Trust beats intelligence

A slightly less capable assistant that behaves predictably is better than a
brilliant assistant that surprises users.

## Principle 2 — Recovery beats perfection

Every subsystem will fail. The user should barely notice.

## Principle 3 — Evidence beats intuition

No architecture or product changes without benchmark evidence.

## Principle 4 — Capabilities are earned

Nothing becomes "Qualified" because it exists. It becomes qualified because it
survives testing.

## Principle 5 — The core remains boring

If the core changes frequently, the architecture isn't actually frozen. Defend the
event-driven boundary aggressively: plugins, execution, reasoning, providers, and
permissions each stay isolated. A pull request that reaches across the event
boundary — "this plugin just needs a direct reference to…" — is almost always "No."

## Principle 6 — Local First, Cloud When Better

FRIDAY should solve a problem locally whenever doing so provides comparable
quality, privacy, latency, and cost. Cloud models are enhancements, not
dependencies. Every cloud dependency must have a documented reason to exist.

This is already reflected in the architecture — local Ollama, provider
abstraction, failover, multiple providers, optional cloud routing — but the
*why* belongs here, as permanent philosophy.

## What we track

We do not optimize for crashes, benchmarks, or MTBF in isolation. We optimize for
**trust**, and the leading indicator of lost trust is **friction** — every time a
user bypasses FRIDAY, we record *why* (slower, misunderstood, unreliable, easier
manually, forgot). That dataset eventually becomes the roadmap.

## Milestone: Trust Week

Before Beta, run **Trust Week**: for one week, use FRIDAY for every reasonable
development task, no exceptions. At the end, produce a report — tasks attempted,
tasks completed, manual overrides, recovery events, failures, average latency,
user-frustration log.

Trust Week **passes only if**:

- FRIDAY is used for every practical development task.
- Manual bypasses are recorded.
- Every bypass has a reason.
- Every failure receives an issue.
- No failures are ignored.

Surviving Trust Week is the bar for Beta.

---

Every feature must increase trust, reduce friction, or provide evidence. If it does
none of those, it does not belong in FRIDAY.
