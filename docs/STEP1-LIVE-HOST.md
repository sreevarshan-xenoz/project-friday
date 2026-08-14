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

## Gap B — memory write + recall in production ✅ shipped

**Problem:** the Executive already runs an episodic admission gate every cycle
(`admission.admit` → `EpisodeStore::store` → `SemanticMemory::process_episodes`,
executive.rs:690-716), but it is **in-memory only** — restart the daemon and the
goldfish is back. And a bare statement ("My main project is FRIDAY") classified
to the `SearchMemory` capability found **no registered tool**, so its plan never
executed → `NoEvidence` → rejected → nothing remembered.

**Shipped design — one memory, made durable (no second store):**
1. **`MemoryJournal`** (goal-execution `src/journal.rs`): a durable, append-only,
   `Arc`-shareable JSONL journal of `CognitiveEpisode`s. `open()` creates the
   dir + replays valid lines (corrupt lines are skipped, never fatal); `append()`
   writes one line; `recall(query, limit)` scores by significant-token overlap
   (stopwords dropped, ≥4 chars) and returns top-k, newest-first.
2. **`EpisodeStore::with_journal`** (memory.rs): seeds the store from the journal
   at startup and appends every admitted episode to it — the SAME episodes the
   Semantic Memory derives patterns from. `Executive::with_episode_journal`
   (executive.rs) wires it. So "close FRIDAY, reopen it tomorrow" replays memory.
3. **Real memory tools** (mobile goal_runner.rs): `MemoryRecallTool` claims the
   built-in `SearchMemory` capability and reads the shared journal —
   unclassified/recall goals now **execute** and pass the evidence gate, so the
   episode (with the fact in its goal text) is admitted + journaled.
   `MemoryRememberTool` claims `memory.remember`; the classifier routes explicit
   "remember X" here (planning.rs rule 8: `remember` → `memory.remember`,
   `recall` stays `SearchMemory`).
4. **Context Builder** (S1's `compose_goal_with_context`): now appends a
   `[memory]` block — recall runs on the RAW goal, then goal + `[session
   context]` + `[memory]` are assembled into the single context the planner and
   the LLM investigation prompt read. The classifier reads only the user's
   **goal segment** (`planning.rs::goal_segment`, everything before the first
   `\n\n[` marker) so echoed context can never re-classify the goal.
5. **Wiring:** `GoalRunner::with_memory` / `with_fabric_and_memory` attach the
   journal (existing constructors stay `None` = in-memory, backwards compatible);
   the daemon opens `FRIDAY_MEMORY_DIR` (default `friday-data`) +
   `episodes.jsonl` and logs how many episodes were replayed.

**Acceptance (tests, green):** `memory_recall_surfaces_prior_fact_across_restart`
— tell FRIDAY "My main project is FRIDAY", start a brand-new runner (fresh
Executive) on the same journal, ask "What was that project I told you about?" →
the result is `recalled … FRIDAY`. `remember_goal_writes_fact_and_later_goal_recalls_it`
covers the explicit remember→recall loop. Journal round-trip/corrupt-line tests
and classifier-immunity tests in goal-execution. The demo is the daemon + portal:
tell it a fact today, restart, ask tomorrow.

## Gap C — human approve / pause / cancel actually consumed ✅ shipped

**Problem:** `task.cancel` / `task.pause` / `task.approve` are published as bus
events (commands.rs:58-80) and acked `{accepted:true}`, but **zero subscribers**
act on them. Permission engine auto-approves `AllowOnce`
(action-runtime/src/permission.rs:9-13). A human cannot stop FRIDAY.

**Design (implemented):**
1. `GoalRunner` runs a **control subscriber loop** (goal_runner.rs `start()`):
   - `task.cancel` → when the id matches the in-flight goal id (registered
     before `goal.started`), flips the Executive's `abort` flag.
   - `task.pause` → same, via the `pause` flag. Pause is **terminal in S3** (no
     `task.resume` protocol method yet; `Executive::resume()` exists for tests).
   - `task.approve` → `Executive::approve_pending(id)` admits the held episode;
     success publishes `goal.approved { task_id }`; a miss is ignored (never a
     fabricated transition).
2. Cooperative cancellation: the Executive cycle checks the flags at three
   checkpoints — post-investigate (before Plan), in the action loop (before
   each action), and post-execute (before Verify). A stopped cycle records no
   terminal episode and returns with phase=Idle.
3. The runner publishes real state transitions, not acks: `goal.cancelled`
   (reason `cancelled_by_human`) and `goal.paused` (reason `paused_by_human`),
   each carrying the run id, phases completed, and session turn — the portal
   card closes on these, never hangs, never resolves as success/failure.
4. Held (sensitive) episodes: `goal.completed` carries `pending_approval: true`
   + `pending_approval_id`; the portal renders an "Approve & remember" button
   that sends `task.approve` with that exact id.

**Acceptance:** unit + integration tests in `executive.rs` (cancel mid-execution
with a gated tool, pause-then-resume, approve-pending) and mobile E2E tests in
`goal_runner.rs` (cancel/pause against a delayed hermetic mock web server;
approve against a sensitive "reset my password" goal; unknown-approve ignored).
Frontend: `protocol.ts` parses `cancelled`/`paused`/`pending_approval_id`;
`ChatScreen.tsx` renders cancel/pause controls on running cards, an approve
button on held goals, and terminal cancelled/paused badges.

## Gap D — browser + Windows adapters on the real kernel path ✅ shipped

**Problem:** the daemon's GoalRunner registers fs/git/terminal/web/docs/
codeintel/webagent — **not** BrowserAdapter (real CDP, egress-hardened) and
**not** the Windows desktop adapter (real C# host, previously only on the qual
harness). "Open notepad" fell through the classifier to read-only memory recall.

**Design (implemented):**
1. Intent routing in `planning.rs` `capability_for_description`:
   - a URL intent (scheme / `www` / known domain suffix) + open/browse/
     navigate/go-to → `web.open`;
   - a bare open/launch with no file target, URL, or web framing →
     `windows.open` (rule order keeps file reads and web search intact).
2. Two dedicated tools on the daemon's Executive, wired by the live-host
   constructor `with_fabric_and_memory` (hermetic constructors stay `None` so a
   test never talks to a real pipe or launches Chrome):
   - `WindowsOpenTool` → `WindowsAdapter` → named pipe to the C# host
     (`friday-windows-default`), resolving friendly names to executables
     (`open notepad` → `notepad.exe`);
   - `BrowserCapabilityTool` → `BrowserAdapter` (CDP Chrome). Only the safe
     `web.open` verb is advertised — `web.evaluate` (Risk::High) is off the
     surface entirely, not merely denied.
3. Both capabilities join the advertised world state so evidence/feasibility
   checks accept them.

**Honest degradation (deviation from the draft):** when the C# host isn't
running, the pipe transport returns `HostUnavailable` and the goal fails
honestly — the daemon never falls back to a mock that fakes a launch. The mock
transport is a test stand-in only.

**Acceptance (tests, green):** mobile E2E `open_notepad_goal_completes_through_windows_host`
runs the whole live-kernel path — goal → classifier (`windows.open`) →
Executive → `WindowsAdapter` → `MockTransport` → verified → `goal.completed`,
asserting the adapter sent `windows.open:notepad.exe`. `browser_tool_advertises_safe_surface_and_needs_a_url`
proves evaluate is unadvertised and a URL-less instruction is refused, never a
browser launch. Classifier regression tests (`open_goals_route_to_windows_and_browser`,
`open_goals_do_not_hijack_existing_intents`) in goal-execution. Real Chrome
execution remains covered by the existing env-gated browser tests. Demo: run
the C# `windows-host` (`FRIDAY_SESSION=default`), start the mobile daemon, tell
the portal "open notepad".

## Build order (each lands green independently)

| # | Slice | Files | Ships with | Status |
| - | ----- | ----- | ---------- | ------ |
| 1 | Gap A — session continuity | `commands.rs`, `goal_runner.rs`, `client.ts` | portal conversation continuity | ✅ `7882658` |
| 2 | Gap B — memory write+recall | `journal.rs`, `memory.rs`, `executive.rs`, `planning.rs`, `goal_runner.rs`, daemon main | durable memory, no goldfish on restart | ✅ |
| 3 | Gap C — cancel/pause/approve consumed | `executive.rs`, `goal_runner.rs`, `protocol.ts`, `ChatScreen.tsx` | human can stop/admit FRIDAY | ✅ `e571a3c` |
| 4 | Gap D — browser+windows on kernel | `planning.rs`, `goal_runner.rs`, `Cargo.toml` | laptop control (open apps / URLs) | ✅ shipped |

## Non-goals for Step 1

- No new daemon binary (extend `friday-mobile-daemon`).
- No voice, no TTS, no wake loop (that's Step 7, rides on this host).
- No perception→action (Step 5).
- No durable WorkflowEngine swap of the Executive (Step 6).
- No 24 h endurance / Server FRIDAY (parked).
