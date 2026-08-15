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

## Gap E — in-cycle recovery on the kernel path ✅ shipped

**Problem:** FRIDAY already had recovery machinery — `AgentKernel` retries
(`action-runtime`), checkpointed `WorkflowEngine` recovery, `AutonomousTick`
crash-safe continuation — but **none of it sat on the live laptop path**
(`GoalRunner → Executive → GoalEngine`). A failed goal action was single-shot:
execute once → fail → terminal, with no re-observe, no retry, no re-plan. The
nasty case (action partially executes, then fails, then FRIDAY restarts) had no
answer on this path.

**Design (implemented):**
1. A plan-level execution policy, not a learning dependency: `Plan::max_retries`
   (default `DEFAULT_RECOVERY_MAX_ATTEMPTS = 2`, builder `with_max_retries`).
   The Executive reads this bound at the Execute boundary. Recovery never
   depends on the learning system; learning may later translate a `Retry`
   policy into the bound, not the other way around.
2. The Executive seam `execute_action_with_recovery`: when the goal's **own**
   action fails unexpectedly (tool or verification), the Executive **re-observes
   reality** (snapshots the world state, pushes a `TraceNodeKind::Recovery`
   node into the audit trail), then re-executes and re-verifies — bounded.
   Structural failures (no tool resolved for the capability) are never retried;
   a human cancel/pause landing mid-recovery halts further retries so the
   cooperative stop invariant (S3) holds.
3. Recovery evidence: the report gains `recovered`, `recovery_attempts`,
   `recoverable`, `failed_reason`, `last_observed_state`. `goal.completed`
   carries `recovered` / `recovery_attempts`; an exhausted recovery publishes
   `goal.failed` with the full recovery evidence — never a phantom completion.
   Every attempt's execution → verification → reflection nodes remain in the
   trace.

**Honest boundary (deviation from the draft):** S5 proves **in-cycle runtime
recovery** — failure → re-observation → bounded recovery → verification. It
does **not** claim crash/restart continuation (process dies → restart →
recover persisted world state → continue); that stays a later layer on the
durable workflow infrastructure. "Re-plan from fresh state" is satisfied by the
re-observe feeding the retry (re-execute against freshly observed reality);
full plan regeneration is not in this seam.

**Acceptance (tests, green):** S5-A and S5-B at both the kernel and runner
level. Kernel (`executive.rs`): `goal_action_recovers_after_a_transient_failure`
(fail once after a partial side effect → re-observe → retry → verified →
`recovered: true`, `recovery_attempts: 2`, a `Recovery` node in the trace) and
`goal_action_exhausts_recovery_and_fails_recoverable` (fails every attempt →
hard ceiling at 1 + max_retries, `recoverable: true` + `failed_reason` + last
observed state, no infinite retry machine). Runner (`goal_runner.rs` via
`MockTransport`): `windows_goal_recovers_after_a_transient_host_failure`
(`goal.completed` with `recovered: true`, both attempts reached the host) and
`windows_goal_exhausts_recovery_and_fails_recoverable` (`goal.failed` with
`recoverable: true`, `failed_reason`, `recovery_attempts: 3`,
`last_observed_state`).

## Gap F — laptop perception on the live kernel path ✅ shipped

**Problem:** FRIDAY already had real sensors (`WindowsScreenSensor` — Win32
foreground window + FNV-1a client-area diff; `WindowsFocusSensor` —
`GetForegroundWindow`; plus audio/camera pipelines) and a `SensorLoop`, but
**none of it sat on the live laptop path**. `friday-goal-execution` had zero
`sensors` dependency, the blackboard `WorldState` was written exactly once at
runner construction (the static capability surface) and never updated, and the
Executive's Observe phase read only that static snapshot. Perception existed;
nothing on the host consumed it.

**Design (implemented — the approved seam):**
1. **One observation path, one WorldState.** The existing `SensorLoop` gains a
   workspace-agnostic `poll_all_events()` (same loop, same ordering, no
   publishing). The new adapter `goal-execution/src/perception.rs` — `Observer`
   (owns the `SensorLoop`) plus a **pure** `fold_observation(state, events)`
   that maps observed `WorkspaceEvent`s into the *existing* blackboard
   `WorldState.variables` (`Topic::Screen → "screen"`, `Topic::WindowFocusChanged
   → "active_window"`, other topics ignored honestly) and returns one
   human-readable description per change. No second perception engine, no second
   world abstraction: the cognitive runtime's `PerceptionEngine` handler stays
   where it is.
2. **The Executive observes.** `Executive` gains an optional
   `observer: Option<Observer>` (`with_observer` builder; default `None` = the
   exact pre-S6 cycle). The Observe phase polls the loop, folds events into the
   world state, and surfaces each change as an `AttentionItemKind::Observation`
   (importance 0.5 / urgency 0.4 / novelty 0.5 → aggregate 0.325) — scored to
   **always lose focus to a fed goal** (0.395), so a sensor observation can
   never hijack a user goal. `ExecutiveReport.observations_folded` counts the
   observations actually folded (never a guessed count).
3. **The runner wires the machine.** The daemon's `with_fabric_and_memory`
   builds the real observer (cfg windows): one `SensorLoop` with
   `WindowsScreenSensor` + `WindowsFocusSensor`. `goal.completed` /
   `goal.failed` carry `observations_folded`. Hermetic runners pass `None` — a
   test never touches a real window or screen.

**Honest boundary:** S6 delivers **screen + focus perception into the cognitive
world state with attention surfacing and goal-safe focus ordering**. It does
**not** claim OCR, vision-language reasoning, full UI-element understanding, or
autonomous (unsupervised) goal formation — those are later layers on this same
observation path (audio/vision objects fold through the identical
`fold_observation` match).

**Acceptance (tests, green):** S6-A at the kernel (`executive.rs`):
`observe_folds_sensor_events_into_world_state_and_attention` — a real
`SensorLoop` + `MockSensor` screen event, one Observe phase → `WorldState`
gains `variables["screen"]`, the change surfaces as an `Observation` attention
item, `observations_folded == 1`, and existing world capabilities are never
dropped; plus `cycle_without_observer_is_unchanged` (no observer → no variables,
no sensor attention item, pre-S6 behavior preserved). S6-B at the runner
(`goal_runner.rs` via `MockTransport` + observer with a mock screen sensor):
`runner_observes_sensors_into_world_state_without_hijacking_the_goal` — the real
Observe phase through the actual `GoalRunner` lands the observation in the
blackboard WorldState, surfaces it in attention alongside the fed goal, the
`goal.completed` payload reports `observations_folded: 1`, and the host launch
proves the user goal (not the observation) drove execution.

## Gap G — learning closes the loop on the live kernel path ✅ shipped

**Problem:** the Executive already consolidated every admitted episode through
`SemanticMemory` and promoted patterns into `Policy`s, but on the laptop runner
the loop was **structurally inert**: learned policies could never influence a
real goal. Evidence actions carry the channel skill ids
(`Capability::Custom("web.search")`, `docs.search`, …) while the planner's
classification infers built-in verbs (`SearchWeb`, `ReadFile`, …). The promotion
gate compared capabilities by **exact equality**, so no promoted `Sequence`
policy ever qualified for a runner goal — `policies_applied` was always 0. And
the runner's tool registry resolved actions by capability alone, so a policy's
goal-level `web.search` action collided with the evidence tool and was refused
(evidence-only instructions).

**Design (implemented — the approved seam):**
1. **Relevance gate by capability overlap** (`planning.rs`): `capabilities_overlap`
   maps exactly the five approved channel⇄verb pairs — `web.*⇄SearchWeb`,
   `docs.*⇄ReadFile`, `memory.recall⇄SearchMemory`,
   `filesystem.list_directory⇄ListDirectory`, `filesystem.read_file⇄ReadFile`.
   Deliberately **not** mapped: `git.*`, `codeintel.*`, `external.*`,
   `windows.open`, `web.open` — so a learned `git.*` sequence can never qualify
   an arbitrary shell goal. `policy_applies` / `qualifying_policy_count` use it.
2. **Well-formed Sequence policies** (`experience.rs`): workflow patterns record
   their real `from → then` transition; the promoted `Sequence` policy carries
   `capability: from` + `config["then"]` so the planner can actually consume it
   (no degenerate `execute_command then execute_command`).
3. **Real reflection** (`executive.rs`): the Reflect phase takes the strongest
   real GoalEngine insight (`GoalReport.insights`) instead of the old fabricated
   `builtin` knowledge record; a goal with no insight records no reflection
   (honest empty — never fiction).
4. **Evidence-aware dispatch** (`verification.rs` + `execution.rs`):
   `ToolRegistry::resolve_action` routes `gather evidence:` actions to their
   channel's evidence tool **by name** (`evidence:web`, `evidence:docs`, …) —
   preserving the normalized / health-gated evidence seam — while goal-level
   actions resolve by capability. The runner registers dedicated
   `web.search` / `docs.search` goal tools ahead of the evidence wrappers, so a
   learned workflow policy executes real searches.
5. **Learning receipts** (`executive.rs` + `goal_runner.rs`): `policies_derived`
   (consolidated knowledge after the cycle's Learn) and `policies_applied`
   (how many qualified for THIS goal's planning) ride every `goal.completed`.

**Honest boundary:** S7 proves learning closes the loop on the laptop runner —
workflow → semantic memory → relevance-gated policy → applied plan → verified
execution → real reflection — behind a deliberately narrow equivalence surface.
It does **not** claim blind policy injection (every application is relevance-
gated and rank-competitive), a reflection→skill compiler, a persistence redesign
of the pattern store (journal-backed episodes already replay), or the 24 h
endurance / Server FRIDAY layers (parked).

**Acceptance (tests, green):** S7-A at the kernel (`executive.rs`):
`learning_closes_the_loop_into_policy_driven_planning` (evidence plans →
verified execution → semantic memory → support 3 → `policies_derived` ≥ 1 → the
matching goal's planning qualifies → the PolicyDriven plan wins the ranking),
`unrelated_goal_is_not_hijacked_by_a_learned_policy` (a non-matching goal plans
Direct with `policies_applied == 0`), and
`web_workflow_policy_qualifies_only_matching_web_goals` (the safety triangle:
`open notepad` and `build the project` never qualify the web policy; the matching
web goal does). S7-B at the runner (`goal_runner.rs` against a hermetic mock
search endpoint): `runner_learns_and_reuses_a_web_workflow_policy` — the same
web-research goal runs four times; runs 1–3 execute evidence-gated plans with
`policies_derived` filling at support 3; run 4's goal qualifies the learned
policy (`policies_applied` ≥ 1), plans
`SearchWeb ⇄ web.search → docs.search` as `PolicyDriven` (2 actions at 0.75
confidence beating the 3-step evidence plan at 0.769), executes against the same
hermetic tools, verifies, and completes; the stored episode's reflection is the
real insight naming the goal — never "builtin".

## Build order (each lands green independently)

| # | Slice | Files | Ships with | Status |
| - | ----- | ----- | ---------- | ------ |
| 1 | Gap A — session continuity | `commands.rs`, `goal_runner.rs`, `client.ts` | portal conversation continuity | ✅ `7882658` |
| 2 | Gap B — memory write+recall | `journal.rs`, `memory.rs`, `executive.rs`, `planning.rs`, `goal_runner.rs`, daemon main | durable memory, no goldfish on restart | ✅ |
| 3 | Gap C — cancel/pause/approve consumed | `executive.rs`, `goal_runner.rs`, `protocol.ts`, `ChatScreen.tsx` | human can stop/admit FRIDAY | ✅ `e571a3c` |
| 4 | Gap D — browser+windows on kernel | `planning.rs`, `goal_runner.rs`, `Cargo.toml` | laptop control (open apps / URLs) | ✅ `8f19e2f` |
| 5 | Gap E — in-cycle recovery | `planning.rs`, `trace.rs`, `executive.rs`, `goal_runner.rs` | failure → re-observe → bounded retry → verify | ✅ `3e63361` |
| 6 | Gap F — laptop perception | `sensors/src/lib.rs`, `goal-execution/src/perception.rs`, `executive.rs`, `goal_runner.rs`, `Cargo.toml` | screen/focus → WorldState → attention | ✅ `3bf4645` |
| 7 | Gap G — learning closed | `planning.rs`, `experience.rs`, `executive.rs`, `verification.rs`, `execution.rs`, `goal_runner.rs` | workflow → policy → applied plan (relevance-gated) | ✅ `07f8bb9` |

## Non-goals for Step 1

- No new daemon binary (extend `friday-mobile-daemon`).
- No voice, no TTS, no wake loop (that's Step 7, rides on this host).
- No OCR / vision-language reasoning / full UI-element understanding (later
  layers on the same observation path).
- No durable WorkflowEngine swap of the Executive (Step 6).
- No 24 h endurance / Server FRIDAY (parked).
