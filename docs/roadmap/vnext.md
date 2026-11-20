# FRIDAY vNext: continuity

Direction agreed with the owner on 2026-11-21, after v0.6.0. This is the
plan for AGENTS.md phases 3 to 5. It doesn't replace their rules: a
subsystem still earns its place, and each change is verified and says what
wasn't.

## The idea in one paragraph

Chatbots answer questions; agents complete tasks. FRIDAY should know what
you are working on, across apps and days, and move it forward when you ask,
with as few instructions as possible. It stays model-independent (identity,
memory, goals and permissions live in FRIDAY, models are replaceable),
local-first (cloud when useful, never required), and quiet: it acts on
events, not in an endless model loop, and it never watches the camera,
microphone or screen unless you turned that on.

## What already exists, and what's missing

The vision names ten parts. Most have a working start in the code; the
work is joining them, not adding frameworks.

| Part | In FRIDAY today | Missing |
|---|---|---|
| Personal context | `conversations.json` (15-minute conversations), `MemoryJournal` (`episodes.jsonl`), screen recall (`recall.rs`, 3 h, opt-in), answers log, `actions.jsonl`, `lessons.jsonl`, routines | One timeline over all of them; projects as a thing FRIDAY knows; "continue where we stopped" |
| Intent and goals | Standing tasks (`standing.rs`), routines (`routine.rs`), coding-agent runs (`agent_runs.rs`) | Goals that outlive one ask: open tasks per project, what's next |
| World model | goal-execution's `WorldModel` and `KnowledgeGraph`, context collector, sensors, `local_ai` hardware facts | Kept up to date from real events; facts vs. guesses marked |
| Cognitive workspace | goal-execution's `CognitiveBlackboard`, attention manager, event bus | Organs other than the ask path publishing into it |
| Memory | Working (session context), episodic (journal), semantic (patterns → policies), procedural (routines, lessons) | Nothing new; tie them to projects |
| Perception | Selection, screen capture (per-use consent), AT-SPI/UIA screen text, captions, wake word, `watch.rs` | Files and app events per project; images as input |
| Agent director | `agent_runs.rs` + supervisor (opencode, agy, Claude Code), approval rules (`agent_policy.rs`) | Delegation from an ask, with the run visible |
| Model fabric | Inference fabric with failover, local AI ladder and presets, OpenCode Zen free models | Weekly model check, better-model retry (queued) |
| Execution runtime | Action runtime (permission gate, audit), desktop actions, system helper, browser adapter (CDP), page reading (#260) | Multi-step browser tasks; checkpoints for long work |
| Trust and governance | Confirmation gate, risk levels, undo (`friday undo`), egress log, private mode, secret masking, private windows | One place to see and steer all of it; permission levels per kind of action |

## Principles (owner's guardrails)

- FRIDAY core owns identity, goals, state and governance; models, organs and
  backends are replaceable.
- Facts, guesses and unknowns are kept apart. A world model full of guesses
  is worse than none.
- Proactive means an attention policy: act, notify, wait or stay silent.
  Never a stream of interruptions.
- No second memory system, no competing orchestration framework, no
  recursive agents, no always-on sensors by default, no self-editing of
  safety rules, no cloud requirement for the basics.
- One agent with the right context and checks beats twenty without.
- Every feature ships with a working implementation, a test that proves
  it, and a demonstration a person can repeat.

## Milestones, in build order

Each has an exit test; the acceptance suite at the end collects them.

### N0 · Stable base (now)

Merge the open work (#257 to #260), the owner's real-hardware pass, and the
Windows local-AI page. Exit: a green `dev`, the hardware checklist done.

### N1 · Context: "continue where we stopped"

- A **timeline**: one read-only view over conversations, answers, actions,
  agent runs, lessons and (if on) screen recall, grouped by day and by
  project. A project is a folder FRIDAY sees you working in (the terminal's
  directory, the editor's file, a git repo), named by its folder.
- **Asks that use it**: "what was I doing yesterday", "continue where we
  stopped", "what's left on <project>": the timeline for that project goes
  to the model with the question, and the answer ends with a next step to
  confirm.
- Exit test (*resume*): after a restart, "continue where we stopped" names
  the last project, what was done and what failed, from real logs.

### N2 · Control centre (before more autonomy)

- One **Activity** page (FRIDAY Shell, tray on Windows): what is running,
  why, which tools it used, what it read and sent (egress), what's waiting
  for approval, what can be undone; cancel and undo from there.
- **Permission levels** per kind of action, from the risk levels the action
  runtime already has: Observe, Suggest, Prepare, Execute, Autonomous
  (bounded tasks only, e.g. standing tasks). Defaults stay where they are
  today; the page shows and changes them.
- Exit test (*governance*): every action of a goal shows up there with its
  level, and a refused or cancelled one leaves nothing behind.

### N3 · Computer runtime

- Pages: open in your browser and read for an ask (#260, done).
- **Browser tasks**: "find the cheapest flight on …", "fill this form from
  my notes": FRIDAY's browser in a visible window, steps planned by the
  model one at a time over the adapter's verbs (open, extract, click, type,
  submit), nothing submitted without a yes, a step limit, the person's
  logins never used unless they connect a profile.
- **Verify**: each step checks its effect (the page changed, the field
  holds the text), and a failure becomes a lesson.
- Exit test (*recovery*): a scripted form task completes in CI against a
  local site, and a broken step stops with a reason instead of guessing.

### N4 · Proactive, with an attention policy

- One policy decides for every event (a build failed, a long command
  finished, a deadline in a note, an unfinished task from yesterday):
  notify now, add to the morning brief, or drop. Do-not-disturb, full-screen
  apps and meetings mean wait.
- A **morning brief** built from the timeline and standing tasks.
- Exit test (*attention*): a day of recorded events produces a short brief
  and at most the notifications the policy allows.

### N5 · Multimodal

- Images and files as input to an ask; region ask; screen text and a
  screenshot together in one context.
- Exit test: "what's wrong in this screenshot" with a dropped file answers
  from it, locally when a vision model is installed.

### N6 · Adaptive

- Lessons and routines turn repeated work into proposals ("you do this
  every Monday: make it a routine?"), tested before they are kept and
  easy to undo. Skill manifests (`services/skills`, skill-forge) and MCP
  (`mcp/`) carry new abilities in.
- Exit test (*learning*): a task that failed twice the same way succeeds
  the third time because of a kept lesson, shown in the answer.

## Acceptance suite (the qualification for vNext)

| Test | Passes when |
|---|---|
| Resume | After a restart, FRIDAY recovers the last project's state from real logs |
| Context | It links a conversation, the files and the failed command of one project |
| Governance | Every action is visible with its level; cancel and undo work |
| Recovery | A broken browser step stops with a reason; a retried task resumes |
| Attention | A day of events yields a brief and few, allowed notifications |
| Privacy | Private mode and private windows send nothing (egress log stays empty) |
| Learning | A kept lesson changes the next attempt, visibly |
| Model independence | Switching provider keeps memory, context and permissions |

## Not now

`docs/roadmap/ecosystem-backlog.md` stays parked: no new memory engine,
orchestration framework, providers without a need, or research crates.
Research into cognitive architectures can happen beside FRIDAY; it ships
into it only through the rules above.
