# Design Study Template

Every entry in `docs/design-studies/` follows this structure. Copy this file to
`<system>.md`, fill it in, and delete this header block.

> **Purpose of a design study:** a record of *why* we adopted or rejected a
> pattern from an external system. It is **not** a decision — that happens in
> `docs/adr/`. If a pattern is adopted, open an ADR and link it from here.

---

# Design Study: <System Name>

**Status:** Design Study
**Date:** YYYY-MM-DD
**Source:** <url> (license, language, release status)
**Reviewed:** <date> (commit/version)

## TL;DR

Three-to-five lines: what the system actually is, the single most important
correction to any common assumption about it, and what (if anything) FRIDAY
should take from it.

## Concrete change

> Every study must answer: **"What concrete change could this cause in FRIDAY?"**
> If the answer is "none," archive this study and move on.

- [ ] <new ADR / new milestone / new manifest field / new crate / new interface
      — the smallest possible follow-up>

## Problem it solves

What engineering problem does the system attack? Not a feature list — the
problem. Include the failure mode it prevents if relevant.

## How it works (architecture)

The shape of the system, enough to reason about it later. Diagrams welcome.
Be faithful — a design study that mischaracterizes its subject is worse than
none.

## Patterns worth adopting

| # | Pattern | Why it's valuable | FRIDAY mapping |
|---|---------|-------------------|----------------|
| 1 | <pattern> | <rationale> | <existing file/crate, or "new"> |

For each adopted pattern, note the decision status:
- **Adopt now** → reference the follow-up in "Concrete change".
- **Defer** → state the trigger condition (mirror in "Revisit Trigger").
- **Reject** → move to "Patterns rejected".

## Patterns rejected

| Pattern | Why rejected |
|---------|--------------|
| <pattern> | <reason, tied to FRIDAY's actual constraints> |

## What it is *not*

Deliberately correct misconceptions: "Not a GUI framework." "Not a cognitive
architecture." "Not production-ready." This section is what saves future-you
from re-deriving the wrong conclusion.

## How it maps to FRIDAY

Where the ideas land in FRIDAY's architecture diagram. If the system is a
component FRIDAY could embed, say so and name the crate. If it is only a
reference for behavior, say that instead.

## Future applicability

Conditions under which this becomes more relevant than today (distribution,
new organs, substrate changes). Forward-looking only — no promises.

## Related notes

Cross-links to other design studies, specs, and ADRs that touch the same area.

## Status of decisions

- [ ] <decision seed, exactly one per concrete change — ADR proposal, manifest
      field, milestone, etc.>

## Revisit Trigger

Re-evaluate this study when:

- <trigger 1: a milestone or workstream begins>
- <trigger 2: a capability or substrate changes>
- <trigger 3: the external system changes materially>
