# Persistent Project Cognition — Long-Term Goal Memory

**Status:** 🚧 Spec-first — implemented against this file (v1 shipped 2026-08-09).
**Depends on:** none (pure data; `serde` + `chrono` timestamps). No LLM inside.
**Crate:** `services/project-cognition` — crate `friday-project-cognition`

## 1. Purpose

FRIDAY has a brain, but until now nothing owned the **project narrative**:
*where the project is going, and what state it is in right now*. goal-execution
remembers episodes, the reflection engine derives insights from outcomes, and
memory stores beliefs — none of them answer

> "We're 82% through the Agent Runtime milestone. One blocker remains: the
> Desktop Agent. 2 decisions are still open: provider routing, persona
> blending. Next up: Web Agent."

That answer is **persistent project cognition**: long-term goal memory
(primary goal / current milestone / blockers / open decisions / progress), the
proactive status briefing (the "FRIDAY speaks" moment, before the user says
anything), and the reflection digest (what changed, for the scheduled
introspection pass).

### 1.1 Core-abstraction rule

The architectural-gravity question — "new core abstraction, or just another
feature?" — answers *core abstraction*: it is the first owner of the
**project-level state** that persists across sessions, complementing the
planner (what to do), skills (how), investigation (what to know), personas
(expression), and episodic memory (what happened on one goal).

```text
Primary Goal → Current Milestone → Progress → Blockers → Decisions
                       │                              │
                       └────────  journal (immutable)  ─┘
                                    │         │
                     status_briefing()   reflection_digest()
```

## 2. Model

**History is immutable, intelligence is derived** (the repo's top invariant,
applied to the project narrative). Every change appends a `ProjectEvent` to
the journal; the `ProjectState` snapshot and the `MilestoneTransition` read
model are projections of that journal, never independent sources of truth.

```rust
pub struct ProjectState {
    pub project_id: String,
    pub name: String,
    pub primary_goal: String,
    pub current_milestone: String,
    pub next_milestone: Option<String>,
    pub progress_pct: f32,             // clamped 0.0..=100.0
    pub milestone_started_at_ms: u64,
    pub blocked_by: Vec<String>,       // deduplicated
    pub updated_at_ms: u64,
}

pub enum ProjectEvent {                // tagged serde, rename_all = snake_case
    PrimaryGoalSet    { goal, at_ms },
    MilestoneAdvanced { from: Option<String>, to, reason, at_ms },
    ProgressUpdated   { pct, milestone, at_ms },  // milestone = context for the digest
    BlockerAdded      { text, at_ms },
    BlockerResolved   { text, at_ms },
    DecisionOpened    { text, at_ms },
    DecisionClosed    { text, resolution, at_ms },
}

pub struct MilestoneTransition { from: Option<String>, to, reason, at_ms } // derived
pub struct DecisionRecord { text, opened_at_ms, closed_at_ms: Option<u64>, resolution: Option<String> }
```

Open decisions are the projection `closed_at_ms == None`; a closed decision
carries its resolution. The persisted artifact is schema-versioned:

```json
{ "schema": 1, "state": { … }, "journal": [ … ], "decisions": [ … ],
  "digest_watermark": 0 }
```

## 3. API

| Method | Effect |
|---|---|
| `new(project_id, name, primary_goal)` | fresh in-memory cognition; watermark 0 |
| `with_path(path)` / `open(path)` | attach persistence / load-or-fresh (warns on corrupt) |
| `save()` | atomic write (temp + rename), creates parent dirs |
| `set_primary_goal(g)` / `set_progress(pct)` | clamp `[0,100]`, ignore non-finite; event carries the current milestone |
| `advance_milestone(to, reason)` | records transition; **resets per-milestone progress to 0** (journaled); consumes a matching `next_milestone` |
| `set_next_milestone(m)` / `clear_next_milestone()` | soft field, not journaled |
| `set_name(n)` / `set_project_id(id)` | soft identity fields, not journaled |
| `add_blocker(d)` / `resolve_blocker(d) -> bool` | dedup on add; `false` if absent |
| `open_decision(t)` / `close_decision(t, resolution) -> bool` | dedup on open; `false` if no open match |
| `state()` / `journal()` / `decisions()` / `open_decisions()` / `transitions()` | queries |
| `status_briefing() -> String` | the "FRIDAY speaks" paragraph (deterministic prose) |
| `facts_since(ms) -> Vec<String>` | rendered journal facts strictly after `ms` (timestamp query) |
| `reflection_digest() -> Vec<String>` | facts since the watermark (**journal index**), then advance it |

## 4. Persistence & safety

- **Atomic**: write to `<path>.json.tmp.<pid>`, then rename (unique temp name
  per process — two concurrent saves can never interleave on the same tmp
  file). A crash never leaves a truncated state file.
- **Sanitized load** (the chaos-Phase-2 `load_health` discipline): schema
  mismatch → warn + start fresh; unreadable/corrupt → warn + start fresh;
  non-finite `progress_pct` → reset to 0; out-of-range → clamp; empty/dup
  blockers dropped. Corrupt files are never trusted.
- **Reconcile-on-load** (the top invariant, enforced): after sanitizing, the
  snapshot and decision log are **re-derived from the journal** — the journal
  is the single source of truth. A hand-edited or future-written snapshot
  that drifted from its own history is corrected, with a warning. Soft
  (non-journaled) fields — `next_milestone`, name, project id — survive.
- **Default path**: `FRIDAY_PROJECT_STATE_PATH`, else `.friday/project-state.json`.
- **Watermark persistence**: `digest_watermark` (a journal *index*, not a
  timestamp — exact even when mutations and a digest share one wall-clock
  millisecond) survives save/load, so a digest across a restart still reports
  only what changed since the last one.

## 5. Invariants

1. The journal is append-only; nothing mutates or deletes a `ProjectEvent`.
2. `ProjectState`, `MilestoneTransition`, and the decision log are derived
   projections — the journal is the single source of truth, enforced by
   reconcile-on-load.
3. An event is reported by the first `reflection_digest()` after it occurs
   (index-based watermark, exactly-once by construction; a fresh cognition
   starts at watermark 0, so its first digest reports everything).
4. Event timestamps are strictly increasing (`max(now, last + 1)`) — ordering
   is preserved and small clock regressions cannot reorder history.
5. `progress_pct` is always in `[0.0, 100.0]` (clamped on write and on load).
6. Progress is per-milestone: `advance_milestone` resets it to 0.
7. Blockers and open decisions are deduplicated by text.
8. The briefing and digest are deterministic prose — an LLM may write events
   through the API, never decide the narrative.

## 6. Acceptance criteria (v1 shipped)

- [x] `friday-project-cognition` builds clean (fmt/clippy/tests) — **21 unit + 1 doc test**.
- [x] Milestone transitions recorded with `from`/`to`/`reason`/timestamp; first transition has `from: None`.
- [x] Progress clamped (`150 → 100`, `-5 → 0`); non-finite ignored; per-milestone reset on advance.
- [x] Blocker dedup + resolve; open-decision dedup; close carries resolution.
- [x] `status_briefing()` covers progress/blockers/decisions/next; "complete" wording at 100%; empty-project wording.
- [x] `reflection_digest()` renders all eight fact kinds (incl. milestone-context progress) and advances the index watermark (second call empty).
- [x] **Exactly-once digest across a same-millisecond boundary** (mutation → digest → mutation → digest yields both events).
- [x] **Reconcile-on-load**: a hand-edited snapshot drifting from its own journal is corrected (journal wins, warn); soft fields survive.
- [x] Save/load round-trip preserves state, journal, and decisions; temp file cleaned up.
- [x] Corrupt file / schema mismatch → warn + fresh; out-of-range progress → clamped through the journal fold.
- [x] `save()` without a path errors explicitly.
- [x] `examples/briefing.rs` demonstrates the FRIDAY-speaks moment + digest.

## 7. Wiring seam (next slices)

- **Nightly reflection**: a scheduled pass calls `reflection_digest()` and
  feeds the facts to the reflection/knowledge layer (`friday-reflection`
  already turns outcomes into insights — this crate supplies the
  *project-state* facts that layer currently lacks).
- **Proactive briefing surface**: a daemon (or the bridge's `RuntimeSource`)
  can expose `status_briefing()` as a first message when a session opens.
- **Consumers write events**: the executive may call `advance_milestone` /
  `resolve_blocker` / `close_decision` on goal outcomes — the API is the only
  mutation path (one owner).
- **Progress deltas**: the digest currently renders absolute
  `Progress (<milestone>): N%`; a delta (prev → new) would need a
  prior-value field on the event — parked, not built, until a consumer asks
  for it.
- **Schema migration**: `schema: 1` today; bumping it rejects old files
  (warn + fresh). A migration path is a later concern (same rule as skills).
