# Architecture Decision Records

FRIDAY records significant architectural decisions here. Each ADR answers:
**Context, Decision, Consequences, Alternatives considered.**

`DecisionIntelligence.md` explains **what** the architecture is; these records
explain **why** it became that way.

| #    | Title                          | Status   | Scope                                   |
| ---- | ------------------------------ | -------- | --------------------------------------- |
| 0001 | Core 1.0 Freeze                | Accepted | `services/goal-execution`               |
| 0002 | Learning Layer                 | Accepted | `friday-plugin-runtime`, `friday-experience` |
| 0003 | Observability Layer            | Accepted | `friday-experience` read models         |
| 0004 | Public API Stability Tiers     | Accepted | `friday-experience` public surface      |

## Frozen architectural contracts

The following are **contracts**, not implementation details. Changing any of
them requires a new ADR, not an ordinary implementation commit:

1. The three-layer architecture (Execution, Learning, Observability).
2. The one-way dependency graph (no backward edges).
3. The immutable `DecisionRecord` model.
4. **"History is immutable. Intelligence is derived."**
5. The separation between stable and experimental APIs.
6. The naming convention (`*Engine`, `*Builder`, `*Store`, `*Report`,
   `*Factor`, `*Timeline`).
7. The ADR process itself.
8. **The validation evidence gate** — no architectural changes without
   evidence from the [Validation Program](../VALIDATION.md).

New ADRs follow the format: **Context, Decision, Consequences, Alternatives
considered**. Next number: 0005.

## How to add an ADR

1. Create `docs/adr/NNNN-short-title.md` using the format above.
2. Add it to the index table.
3. Record what prompted the decision and which contracts (if any) it
   modifies or supersedes.
