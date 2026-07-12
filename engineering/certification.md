# Certification

Certification is the broader engineering sign-off for a whole capability. It is
distinct from **Qualification** (recovery maturity only — see `qualification.md`)
and from architecture.

## Prerequisite

A capability must be **Qualified** (recovery coverage ≥ 85%, sustained) before it
can be Certified. Certification builds on qualification; it does not replace it.

## Requirements (all must hold)

A capability is **Certified** (e.g. `Filesystem v1.0 Certified`) only when:

- **Benchmarks pass** — its task suite meets the qualification bar.
- **Chaos tests pass** — injected failures are handled within policy.
- **Recovery passes** — Qualified (coverage ≥ 85%, sustained).
- **Documentation complete** — design in `specs/`, user-facing behaviour described.
- **Telemetry present** — health/events emitted (e.g. `set_provider_health`).
- **Observability present** — request traces (`trace_id`) and metrics are
  exportable for debugging. Distinct from telemetry: telemetry is the raw
  health/event data; observability is the ability to derive insight from it.

## Evidence must be sustained

A capability is **not** certified because it worked once. Evidence must hold over a
defined window (e.g. trailing N runs / chaos campaigns) before sign-off.

## Lifecycle impact

Capabilities do not start at `Prototype`. They start at **Certification Pending**.
Everything then follows the same engineering discipline, so a capability only
reaches users once it has evidence behind it — not merely a working demo.

## Relationship to other gates

- **Qualification** = does it survive failure? (recovery)
- **Certification** = is the whole capability engineered and evidenced?
- Certification is the gate that turns a technically interesting component into
  something people are willing to rely on daily.
