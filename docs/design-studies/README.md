# Design Studies

A library of architectural decisions drawn from real-world systems — the *why*
behind each adoption, rejection, or deferral, recorded so it does not have to
live in anyone's head.

## Rule

> Every study must answer: **"What concrete change could this cause in FRIDAY?"**
> If the answer is "none," archive it and move on.

A study is **not** a decision. Decisions live in `docs/adr/` as ADRs, linked
from the study that motivated them.

## Workflow

1. Study an external project.
2. Correct any wrong assumptions about it.
3. Extract reusable patterns.
4. Reject what does not fit — and record **why**.
5. Create the smallest possible follow-up (ADR candidate, manifest field,
   milestone, crate, interface).
6. End with a **Revisit Trigger** so future-you knows when the assumptions
   stop holding.

## Template

Start from [`_TEMPLATE.md`](./_TEMPLATE.md).

## Studies

| Study | System | Date | Concrete change |
|-------|--------|------|-----------------|
| [cloudflare-computer.md](./cloudflare-computer.md) | Cloudflare Computer (`@cloudflare/computer`) | 2026-08-08 | Two-phase execution ADR (deferred); skill-manifest `readonly` field |
