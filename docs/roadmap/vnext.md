# FRIDAY vNext: your work, remembered

Direction agreed with the owner on 2026-11-21, after v0.6.0, and settled
with a second review the same day. This is the plan for AGENTS.md phases
3 to 5. It doesn't replace their rules: a subsystem still earns its place,
and each change is verified and says what wasn't.

**The promise:** FRIDAY remembers your work and restores your context.
Your intentions don't disappear between sessions, your thinking doesn't
disappear between conversations, and your working environment doesn't
reset when your goals change. (Internal north star: a "personal reality
engine": computing around what you're trying to make, not around apps.)

**The test that decides N1:** shut everything down; tomorrow say "Friday,
let's continue building FRIDAY". Within five seconds FRIDAY knows which
project you mean, restores as much of your working environment as it can
(and says what it couldn't), shows where you stopped, and brings back the
unfinished thoughts that belong to it.

## The idea in one paragraph

Chatbots answer questions; agents complete tasks. FRIDAY should know what
you are working on, across apps and days, and help when invited: it
preserves context and stays out of the way otherwise. It is not a
life-management supervisor: it never tells you you're behind or questions
your priorities. It stays model-independent (identity, memory, goals and
permissions live in FRIDAY, models are replaceable), local-first (cloud
when useful, never required), and quiet: it acts on events, not in an
endless model loop, and it never watches the camera, microphone or screen
unless you turned that on.

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
- Every factual answer about your work traces to a file, a log line or a
  recorded event. No progress invented by a model.
- No new architectural expansion until the flagship (N1) works end to end.

## The shared foundation: a project context index

Workspaces, thought threads and project answers must not each remember a
slightly different version of reality. Underneath them is one **project
context index**: computed from the files FRIDAY already writes (answers,
actions, agent runs, lessons, the timeline, workspaces, threads), never a
second store that can drift from them. Entities: project, workspace,
session, thought, decision, task, file, event. Links: belongs to, created
during, derived from, depends on, resumes, contradicts (only with the
evidence shown). A project is a folder (a git repo, the terminal's
directory, the editor's file), named by it. The timeline (`timeline.rs`)
is its first slice.

## Milestones, in build order

Each has an exit test; the acceptance suite at the end collects them.

### N0 · Stable base (now)

Merge the open work, the owner's real-hardware pass, the Windows local-AI
page. Exit: a green `dev`, the hardware checklist done.

### N1 · Context: workspaces, session resurrection, thought threads

- **Workspaces with session resurrection.** `friday workspace save
  <name>` keeps the arrangement *and* the work: windows and their
  workspaces and positions, terminals in their project folders, browser
  tabs, editor projects and open files where the editor tells, notes, and
  the project's unfinished items. `restore` brings it back and reports what
  it restored and what it couldn't (a tab gone, an app that doesn't expose
  its state). A session is saved on its own when you log out.
- **Thought threads.** "Keep this idea" files the thought under its
  project with why it came up, then later decisions, evidence and open
  questions join the same thread, versioned, so you can see how the idea
  changed. FRIDAY never rewrites a thought, and only points out a conflict
  with the two entries side by side.
- **"Continue building X" / "prepare my world for X"**: the index finds the
  project, restores its workspace, and the answer shows where you stopped
  (timeline, #262) and the open threads.
- Exit test (*resume*): the N1 test above, scripted in the VM: save,
  reboot, ask, and count what came back.

**Status (2026-11-21):** built and merged: workspaces with session
resurrection (#264; restore by window address, terminals as themselves,
#265), "continue building X" (#265), thought threads (#268), opt-in
session autosave (#269), answers that keep their project (#270), on the
timeline (#262). Checked live in the FRIDAY OS VM: save and restore
(windows back on their workspaces, the floating one in place, the editor on
its file, 0.08 s, no duplicates), and the autosave save from systemd without
Hyprland's environment. Owed on real hardware: the whole N1 test after a
reboot, and the project in front (the VM gives no window focus). Not yet:
browser tabs (the browser's own session restore for now), GUI editors' open
files, niri.

Two outside audits of v0.7.1 (2026-11-21) led to #277 and its follow-up:
resume asks get the timeline and thoughts, a named project never gets
another project's history, home paths stay out of prompts, and page reads
refuse local-network redirects. Since then (after-0.7.3 #2) one project
identity is carried through the timeline, thoughts, lessons and git facts,
`friday ask` and the daemon share one resume grammar
(`tests/fixtures/resume-phrases.tsv`), and lessons keep their project.
Browser page reads now refuse a redirect to the local network before Chrome
contacts it too (CDP request interception in the browser adapter). Gaps found
since are in `after-0.8.0.md`.

### N2 · Continuity: project understanding

- "What's left on FRIDAY", "what did we decide about memory", "why did we
  drop X": answered from the index with each fact's source (file, log line,
  thread entry). Decisions and how they evolved, unfinished work,
  related files.
- Exit test (*context*): a seeded project's questions are answered with
  correct, cited sources, and "I don't know" where the logs don't say.

### N3 · Environment: modes and attention

- Modes (creation, learning, focus, system care) are saved workspaces plus
  an attention policy: focus holds notifications for later, a full-screen
  app or a meeting means wait. One policy decides for every event: notify
  now, add to the morning brief, or drop.
- Exit test (*attention*): a day of recorded events yields a short brief
  and only the notifications the policy allows.

### N4 · Sovereignty

- A page that shows what FRIDAY believes about you and your projects, where
  each belief came from, and lets you correct or delete it; `friday export`
  writes everything (threads, workspaces, preferences, history) as plain
  files you can take to a new computer, where `friday import` rebuilds your
  environment.
- Exit test (*model independence*, *portability*): switching provider keeps
  memory, context and permissions; an export imported on a fresh install
  restores the workspaces and threads.

### After the flagship works

In this order, each with its own exit test:

- **Control centre**: one Activity page for what's running, what it read and
  sent, approvals, cancel and undo; permission levels (observe, suggest,
  prepare, execute, autonomous for bounded tasks) per kind of action.
- **Computer runtime**: browser tasks in a visible window over the adapter's
  verbs, nothing submitted without a yes, each step verified (pages: open in
  your browser and read for an ask are done, #260).
- **Multimodal**: images and files as input; region ask with screen text.
- **Adaptive**: repeated work offered as a routine; lessons that change the
  next attempt.
- **Future branches** ("what if I spend two weeks on voice"): only once
  months of reliable history exist, and then showing assumptions, never
  predictions dressed up as simulation.

## Acceptance suite (the qualification for vNext)

| Test | Passes when |
|---|---|
| Resume | "Let's continue building FRIDAY" after a reboot: project found, workspace restored (with a report of what wasn't), where you stopped, open threads; under five seconds |
| Context | Project questions answered with cited sources, "I don't know" where the logs are silent |
| Thought | A kept idea and a later decision show up in one thread, in order, unedited |
| Governance | Every action is visible with its level; cancel and undo work |
| Recovery | A broken browser step stops with a reason; a retried task resumes |
| Attention | A day of events yields a brief and few, allowed notifications |
| Privacy | Private mode and private windows send nothing (egress log stays empty) |
| Learning | A kept lesson changes the next attempt, visibly |
| Model independence | Switching provider keeps memory, context and permissions |
| Portability | An export imported on a fresh install restores workspaces and threads |

## Not now

`docs/roadmap/ecosystem-backlog.md` stays parked: no new memory engine,
orchestration framework, providers without a need, or research crates.
Research into cognitive architectures can happen beside FRIDAY; it ships
into it only through the rules above.
