# architecture/

Architecture Decision Records (ADRs), sequence diagrams, and system topology
notes. Captures *why* FRIDAY is shaped the way it is.

## Where to look

| Need | Location |
| ---- | -------- |
| Narrative architecture | `docs/Architecture.md` |
| **Source-of-truth specs** (post-M4.7) | `specs/README.md` |
| ADRs | `specs/adr/` (e.g. ADR-0007 execution-runtime unification) |
| Subsystem contracts | `specs/*.md` |
| Process / qualification | `engineering/` |

New architectural decisions should land as ADRs under `specs/adr/` and be linked
from `specs/README.md`.
