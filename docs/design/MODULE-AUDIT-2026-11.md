# Module audit (2026-11-20)

Which workspace crates take part in answering a question, and which should.
Method: `cargo tree -p friday-mobile` for what the daemon links, a trace of one
Super+Y ask from the hotkey to the answer, and a read of each crate. A crate
earns a place in the daemon only with one concrete job a person would notice.

## The ask path today

Super+Y → `friday ask` → `friday-ctl goal` → daemon (`commands.rs` → `plan.goal`)
→ `goal_runner.rs` (conversation turns + memory recall from `episodes.jsonl`)
→ `goal-execution` executive (investigation) → `intelligence` fabric (provider)
→ answer in `state.json` → FRIDAY Shell's answer card or a notification.

Since #222 the last four turns of the conversation ride along with each ask.

## Not linked into the daemon (11 crates, about 21,500 lines)

| Crate | Lines | Used by | What it is | Recommendation |
|---|---|---|---|---|
| autonomy | 4,435 | nothing | Bounded autonomous ticks (select goal, plan, run, verify) | Archive: standing tasks (`standing.rs`) do the job a person sees |
| engines/perception | 277 | nothing | Sensor polling into the old Global Workspace loop | Archive: the daemon reads sensors and context itself |
| persona | 1,134 | nothing | Mood and style signals turned into a prompt style | Candidate: a user-set tone/verbosity for answers, if asked for |
| project-cognition | 1,161 | nothing | Project milestones, blockers and decisions journal | Candidate: "what was I doing on X" across days, if asked for |
| observability | 1,572 | autonomy | OpenTelemetry export of cognitive traces | Keep as a library; goes with autonomy if that is archived |
| recovery | 806 | benchmark | Failure category → recovery policy | Candidate: today's failover is in the fabric and `failures.rs`; merge only if it replaces them, not alongside |
| skill-forge | 764 | nothing | Scaffold and validate third-party skills | Keep as a developer tool |
| experience | 4,127 | bridge | Statistics over plugin-runtime decisions | Archive with plugin-runtime |
| lesson-store | 1,408 | autonomy | redb store of prediction-error lessons | Archive with autonomy |
| plugin-runtime | 5,148 | experience, bridge | Sandboxed dynamic plugin execution | Archive: tools run through `friday-actions` |
| simulation | 715 | benchmark, e2e | Dry-run risk and duration estimates | Keep for benchmarks |

## Linked but barely used on an ask

| Crate | Lines | Note | Recommendation |
|---|---|---|---|
| cognitive | 3,112 | The daemon uses its `Topic` enum; its heartbeat loop doesn't run | Keep the types; the loop is superseded by the daemon's async runner |
| engines/memory | 427 | In-memory event store; the daemon's memory is `MemoryJournal` | Fold into goal-execution or drop |
| reflection | 350 | Insight types over outcomes | Fold into goal-execution |
| knowledge | 989 | Workflow patterns and a triple graph | Candidate, same rule as above |
| agent-runtime | 3,768 | Sub-agent runtime; FRIDAY's agent runs are `agent_runs.rs` | Keep only what action-runtime needs |

## Done (2026-11-20)

Why they were unused: they were built in July and August 2026 as research
prototypes (the R2 qualification campaign, milestones M4.7, 7A, 8B). When the
plan turned to "first hour works", the daemon grew its own smaller versions
(standing tasks, `MemoryJournal`, the fabric's failover) and these were never
wired in.

Archived (removed from the workspace): autonomy, engines/perception, persona,
project-cognition, observability, experience, lesson-store, plugin-runtime and
bridge, with `protocol-tests/` (bridge's fixtures). Restore any with
`git checkout dc501bd -- services/<crate>`.

Kept: skill-forge (ships in friday-git as `friday create-skill|validate|publish`),
recovery and simulation (used by the benchmark and e2e crates).

## The five "linked but barely used" crates, after v0.6.0 (2026-11-20)

Checked with `grep` over the workspace and `cargo tree -p friday-mobile`; the
call sites below were read, but no ask was run to watch them.

| Crate | Reached from the daemon how | Does a person notice? | Verdict |
|---|---|---|---|
| cognitive | `Topic` and `WorkspaceEvent` in `voice.rs`, `goal_runner.rs` and goal-execution's `execution.rs` | Types only; the heartbeat loop isn't run | Keep the types. Dropping the loop is a separate, safe cut. |
| engines/memory (`friday-memory`) | One call site: `Episode::from_event` in goal-execution's `create_episodes` | No; the daemon's memory is `MemoryJournal` | Fold `Episode` into goal-execution, then drop the crate |
| reflection | goal-execution `reflect()` runs after every executed action | No: the strongest insight is stored as the episode's `reflection` string, and only tests read it back (`goal_runner.rs` tests) | Candidate to fold or remove together with knowledge |
| knowledge | `KnowledgeEngine::analyze` feeds `reflect()`; `KnowledgeGraph` in `world.rs`, `planning.rs` | Only through reflection (above) | As reflection |
| agent-runtime | `SecurityScope` is used by action-runtime's request, egress, kernel and browser code; `AgentDriver`/`AgentDirector` only by goal-execution's `autonomy.rs` and tests | `SecurityScope` yes (egress rules); the driver no | Keep `SecurityScope`; the driver side is the part to cut if `autonomy.rs` goes |

None of these is dead: goal-execution links all five, so archiving one means
editing goal-execution first. The order that removes the most with the least
risk: (1) `autonomy.rs` in goal-execution (the only user of the agent driver),
(2) reflection + knowledge together with their `reflect()` call, (3) fold
`Episode` and drop `engines/memory`. Each needs the goal-execution and
goal_runner tests re-run, and each is the owner's call.

Not checked: whether stored `reflection` strings feed a later prompt through
`MemoryJournal` recall (grep found no reader outside tests); whether
`autonomy.rs` is called from anything outside tests.

## Rules for what comes next

- Nothing gets wired in to raise a count. A candidate goes in when a person
  asks for its job and it replaces code, not when it sits beside it.
- Archiving means moving out of the workspace (or deleting), so CI stops
  building it. Each is the owner's call; none is done by this audit.
- Re-run `cargo tree -p friday-mobile` after any change to keep this table true.
