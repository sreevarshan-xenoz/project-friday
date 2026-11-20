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

## Rules for what comes next

- Nothing gets wired in to raise a count. A candidate goes in when a person
  asks for its job and it replaces code, not when it sits beside it.
- Archiving means moving out of the workspace (or deleting), so CI stops
  building it. Each is the owner's call; none is done by this audit.
- Re-run `cargo tree -p friday-mobile` after any change to keep this table true.
