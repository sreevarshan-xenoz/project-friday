# Step 1 — Live Host: design around the existing boot entrypoint

> Design doc for turning the existing architecture into the daily laptop host.
> Follows the trace of the **actual** executable seam — no invented daemon.
> Companion to [`TRUE-FRIDAY-INTEGRATION-MATRIX.md`](TRUE-FRIDAY-INTEGRATION-MATRIX.md).

## The seam we extend: `friday-mobile-daemon` (NOT the bridge)

`services/mobile/src/bin/friday-mobile-daemon.rs` is already a **live interactive
host**. Trace of the actual running path:

```text
portal (ws://localhost:18432)  -- the Assistant tab's default URL
    │  { type:'command', method:'planner.goal', params:{ goal } }
    ▼
WsServer (transport.rs) → Command{session_id, id, method, params}
    ▼
CommandGateway::dispatch (commands.rs:44) → publish plan.goal { goal }
    ▼
GoalRunner::start (goal_runner.rs:631) → Executive::cycle (REAL adapters)
    ▼
goal.started → goal.completed | goal.failed  (bus events)
    ▼
BusBridge → outbound → portal chat renders task cards
```

The audit's "portal chat is dead by protocol" finding applies to the **bridge**
console (`:18433`, read-only by construction, `connection.rs:401-419`). The
**Assistant tab** uses the mobile protocol (`:18432`) and this path is live and
tested (goal_runner.rs:750-827, transport.rs:635-758).

**Correction to the matrix:** Step 1 is NOT "make the chat work" (it works). It
is closing four specific gaps on this existing seam:

## Gap A — conversation continuity (session threaded through the loop)

**Problem:** every `planner.goal` is a fresh `plan.goal { goal }`; the
`session_id` the transport already carries (`Command.session_id`) is **dropped**
in `CommandGateway` (commands.rs:44-56). The Executive clears attention every
cycle (`exec.attention_manager_mut().clear()`, goal_runner.rs:669) — "Search
YouTube" → "No, the other one" → "Open the first result" are unrelated goals.

**Design:**
1. `CommandGateway`: keep `cmd.session_id` in the published event →
   `plan.goal { goal, session_id }` (one-line, backwards compatible).
2. `GoalRunner`: maintain `SessionContext` map
   (`session_id → { last_goal, last_result, turn }`) guarded by the existing
   mutex; seed the Executive's attention with the prior turn so anaphora
   ("the other one", "the first result") resolve against what the session just
   did. Cap at N sessions (LRU) to stay bounded.
3. Portal: `sendGoal` already has the session via `Welcome.session_id` — thread
   it into `planner.goal` params (frontend one-liner).

**Acceptance:** over one WS connection, "List the files in this project" then
"Show me planning.rs from that listing" reaches a verified file read without
re-deriving the directory (or failing). Test in `goal_runner.rs`.

## Gap B — memory write + recall in production

**Problem:** the durable tiers exist (redb Session/LongTerm, ExperienceEngine,
planner TimelineStore) and the read path exists (`ContextBuilder` →
`PromptCompiler`, context/src/lib.rs:124-163, intelligence/src/compiler.rs) but
**only e2e** uses them. The daemon's GoalRunner never writes an episode and
never recalls into the prompt.

**Design (small, on the existing host):**
1. A `MemoryWriteback` subscriber on the daemon: on `goal.completed`, append a
   `CognitiveEpisode`/`MemoryEntry` to the redb Session layer (goal, phases,
   result text, timestamp) and record the goal on the planner `TimelineStore`.
   Reuses `friday_core::memory` + `planner::experience` — no new store.
2. A `MemoryRecall` hook before each `Executive::cycle`: build a minimal
   `AgentContext` (memories via `MemoryEngine::search`, experiences via
   `ExperienceEngine::query_by_intent`) and inject it into the Executive's
   working context so the planner sees prior related goals.
3. Wire the daemon's `WorkspaceDataDir` (env `FRIDAY_DATA_DIR`, default
   `~/.friday`) so redb/JSON paths are stable across restarts — the "goldfish
   on reboot" fix.

**Acceptance:** run the daemon, complete a goal, restart the daemon, ask
"what did we do last session" — the recalled entry appears in the response.
Test: `MemoryWriteback` appends + `MemoryRecall` returns the entry across an
in-process restart of the memory store.

## Gap C — human approve / pause / cancel actually consumed

**Problem:** `task.cancel` / `task.pause` / `task.approve` are published as bus
events (commands.rs:58-80) and acked `{accepted:true}`, but **zero subscribers**
act on them. Permission engine auto-approves `AllowOnce`
(action-runtime/src/permission.rs:9-13). A human cannot stop FRIDAY.

**Design:**
1. `GoalRunner`: subscribe to `task.cancel`; when the id matches the in-flight
   goal id, cancel the Executive cycle (the goal ids already flow in
   `goal.started`/`goal.completed` payloads — goal_runner.rs:654-663).
2. Add `task.cancel` to the goal lifecycle: emit `goal.cancelled` (new terminal
   kind) so the portal card closes instead of hanging.
3. Permission: leave `AllowOnce` auto-approve (current behavior is by design
   for the bounded demo) but surface a `pending_approvals` bus event so a real
   prompt flow can attach later — no behavior change now, just observability.

**Acceptance:** send `planner.goal` with a slow goal (terminal sleep), send
`task.cancel`, observe `goal.cancelled` and no further execution. Test in
`goal_runner.rs`.

## Gap D — browser + Windows adapters on the real kernel path

**Problem:** the daemon's GoalRunner registers fs/git/terminal/web/docs/
codeintel/webagent (goal_runner.rs:575-589) — **not** BrowserAdapter (real CDP,
egress-hardened) and **not** the Windows desktop adapter (real C# host, currently
only on the qual harness).

**Design:**
1. Register `BrowserAdapter` on the daemon's Executive with the permission gate
   (evaluate/High still denied by default — browser.rs permission already
   handles this). Cap one session, reuse the existing egress boundary.
2. Register `WindowsAdapter` (plugins/windows) with real transport by default,
   mock when the C# host isn't running (graceful degrade like sensors). The
   kernel's permission/audit/egress stack sits in front automatically.

**Acceptance:** "Open Chrome" / "Open notepad" goals route planner →
capability → kernel → browser/windows host → verified result, surfaced as
`goal.completed`. Start with the already-working real Chrome tests as the
browser baseline; add one daemon-level test for a windows mock transport.

## Build order (each lands green independently)

| # | Slice | Files | Ships with |
| - | ----- | ----- | ---------- |
| 1 | Gap A — session continuity | `commands.rs`, `goal_runner.rs`, `client.ts` | portal conversation continuity |
| 2 | Gap B — memory write+recall | new `memory.rs` in mobile, `goal_runner.rs`, daemon main | durable memory, no goldfish on restart |
| 3 | Gap C — cancel consumed | `goal_runner.rs`, protocol event `goal.cancelled` | human can stop FRIDAY |
| 4 | Gap D — browser+windows on kernel | `goal_runner.rs` | laptop control (browser first) |

## Non-goals for Step 1

- No new daemon binary (extend `friday-mobile-daemon`).
- No voice, no TTS, no wake loop (that's Step 7, rides on this host).
- No perception→action (Step 5).
- No durable WorkflowEngine swap of the Executive (Step 6).
- No 24 h endurance / Server FRIDAY (parked).
