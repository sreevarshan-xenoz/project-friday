# TRUE FRIDAY Integration Matrix — the six user paths

> Companion to [`TRUE-FRIDAY-AUDIT.md`](TRUE-FRIDAY-AUDIT.md). That doc is the
> inventory of what exists vs. what is stubbed vs. what is missing. This doc is
> the **integration contract**: six user-level paths that must actually work on
> the laptop before FRIDAY earns the "TRUE" label. Each path lists the organs
> that exist, the seams that are still dead, and the slice that makes it live.
>
> A path is **LIVE** only when a human on this laptop can trigger it and watch
> it complete against the real world (not a test harness).

---

## Path 1 — Voice → Brain → Action → Verification → Voice

> "Hey FRIDAY, open Chrome and search for Rust async traits" → FRIDAY answers in voice.

| Layer | State | Evidence |
| ----- | ----- | -------- |
| Mic capture + VAD + turn detection | EXISTS (real WASAPI) | `services/sensors/src/audio.rs:378-505`, `53-116` |
| Speech→Text (ASR) | STUB — `SpeechToText` seam, only `MockTranscriber` | `services/sensors/src/audio.rs:218-249` |
| Wake word | MISSING | — |
| Brain (goal → plan → execute → verify) | EXISTS (real) | mobile `GoalRunner` → Executive, `services/mobile/src/goal_runner.rs:459-522` |
| Text→Voice (TTS) | MISSING | grep `tts|synthesizer|speak` → nothing |
| Continuous loop | MISSING — nothing runs voice→action continuously | — |

**Slice to make LIVE:** real ASR behind the `SpeechToText` seam (Windows Speech API or local whisper), a TTS adapter with audio output, and a small loop that turns detected speech turns into goals and speaks verified results back. This is the highest-fantasy path and the last to be fully live — it rides on Path 3's host.

---

## Path 2 — Screen → Perception → World Model → Decision → Action

> FRIDAY sees what's on the screen, updates its world model, decides, and acts.

| Layer | State | Evidence |
| ----- | ----- | -------- |
| Screen capture + diff (focus/content) | EXISTS (real) | `services/sensors/src/screen.rs`, `windows_focus.rs` |
| Camera + frame diff | EXISTS (real) | `services/sensors/src/vision.rs` |
| PerceptionEngine (real sensors) | STUB — `with_sensors` used only in tests | `services/engines/perception/src/lib.rs:15-56`, `tests/integration.rs:250` |
| Salience → decision rules | EXISTS (deterministic) | `services/cognitive/src/autonomy.rs:236-285` |
| World model observation feed | STUB — bridge fabricates a synthetic `Topic::Screen` | `services/bridge/src/source.rs:971-979` |
| Sensors constructed in any daemon | MISSING | sensors built only in unit tests |

**Slice to make LIVE:** construct the real sensor loop in the host, feed `Topic` events into a genuine WorldModel observation, and let the autonomous tick's salience decision act on it (currently one tick/process). See Path 5/6 host.

---

## Path 3 — User → Conversation → Memory → Reasoning → Action

> "Open Chrome." → "Search YouTube." → "No, the other one." → "Open the first result."
> Continuity, not six unrelated `goal.raw`s.

| Layer | State | Evidence |
| ----- | ----- | -------- |
| Session/working/long-term memory tiers | EXISTS (redb) | `core/src/memory/backends.rs:30-116` |
| ContextBuilder (memories + experiences + skills) | EXISTS | `services/context/src/lib.rs:124-163` |
| Recall into prompts (PromptCompiler) | EXISTS but **e2e-only** | `services/intelligence/src/compiler.rs`, `e2e/src/runner.rs` |
| Production memory **writer** | MISSING — no daemon writes the durable tiers | mobile daemon + bridge never call it |
| Conversational continuity (multi-turn, anaphora, refs) | MISSING — every goal is a fresh `goal.raw` | — |
| Live conversation surface | STUB — portal chat dead by protocol (`Control::Ignore`) | `services/bridge/src/connection.rs:401-419` |

**Slice to make LIVE:** a host that (a) owns a session context keyed to a conversation, (b) persists episodic + timeline entries, (c) recalls them into the prompt, (d) accepts goals over a live protocol. **This is the foundation the other paths stand on.**

---

## Path 4 — Goal → Planner → Browser/Desktop → Verification → Learning

> "Do this multi-step task with tools on the laptop."

| Layer | State | Evidence |
| ----- | ----- | -------- |
| Full adapter catalog (fs/git/terminal/web/docs/codeintel/MCP/webagent) | EXISTS (real) | `services/action-runtime/src/adapters/*` |
| Browser adapter (CDP) | EXISTS (real, egress-hardened) | `services/action-runtime/src/adapters/browser.rs` |
| Windows desktop host (click/type/scroll/clipboard) | EXISTS but **UNWIRED** from kernel | `plugins/windows/`, `windows-host/`; no production kernel registration |
| Browser session runtime (tab/page abstraction) | PARTIAL — adapter verbs exist; no persistent tab/session model | `browser.rs` |
| Planner (rule + single-shot LLM) | EXISTS | `planner/src/planning/planners/rule.rs`, `llm.rs:321-345` |
| Verification (real command/evidence) | EXISTS and **wired** | `services/goal-execution/src/verifiers.rs` |
| Learning from completed goals | STUB — completion sink → skill learning only in tests; `learn()` never called at runtime | `services/context/src/skill_learning_observer.rs:14-24` |

**Slice to make LIVE:** register the `WindowsAdapter` on the real kernel (permission + audit + egress already in front), and wire the planner → capability registry → kernel → Windows/browser path for laptop tasks. Proves "Open VS Code and create this file" end-to-end.

---

## Path 5 — Failure → Recovery → Replan → Verification

> An action fails; FRIDAY classifies it, recovers, replans, and verifies — without hiding the failure.

| Layer | State | Evidence |
| ----- | ----- | -------- |
| Recovery graph + policy | EXISTS (small, real) | `services/recovery/src/policy.rs`, `defaults.rs` |
| Runtime integration | STUB — shadow-mode only; observes what it *would* do | `services/recovery/` docs |
| Retry / alternate-capability / replan | EXISTS partially (bounded retry in workflows) | `services/goal-execution/src/workflow.rs:1718-1778` |
| Replanning on failure (LLM repair) | STUB — `Repair` treats failure as `Retry`; `Risk::Safe` placeholder | `planner/src/execution/reflection.rs:27-29,89` |

**Slice to make LIVE:** call `RecoveryPolicy::classify` at the action-failure point in the host, and let it produce real decisions (retry / alternate / replan / escalate-to-human) that flow back into the loop. **Must not swallow failures** — escalated failures surface to the human surface.

---

## Path 6 — Multi-step goal → Director → Skills → Memory → Future reuse

> A complex goal spawns subagents, uses learned skills, and the whole thing makes future runs better.

| Layer | State | Evidence |
| ----- | ----- | -------- |
| AgentDirector (roles, bounds, gates, budget, cancel) | EXISTS (complete) | `services/agent-runtime/src/director.rs`, `runtime.rs` |
| Workforce (persistent registry + task board) | EXISTS | `services/agent-runtime/src/workforce.rs` |
| Durable workflows (PD-01..07) | EXISTS but **test-only**; daemons use in-memory Executive | `services/goal-execution/src/workflow.rs` |
| Skill registry + learning | EXISTS | `services/skills/src/`, `services/context/src/skill_learning.rs` |
| Learned-skill **admission** in production | STUB — `admit()`/`enable_learned` only in tests | `services/context/src/skill_learning_observer.rs` |
| Reuse (recall past trajectories into prompts) | MISSING in production (timeline write path test-only) | `planner/src/experience/timeline.rs` |

**Slice to make LIVE:** the host routes multi-step goals through `WorkflowEngine` (durable) with a completion sink that actually admits learned skills and records the timeline, so the next goal recalls it.

---

## Build order (recommended)

| Step | Slice | Unlocks | Laptop status |
| ---- | ----- | ------- | ------------- |
| 1 | **Live host** — one daemon owning the loop: session context, memory write + recall, goal intake over a live protocol, approve/cancel surface | Path 3 core; the power line | ✅ Gap A–C (`7882658` / `e571a3c`) |
| 2 | **Live portal** — wire the existing chat UI to the host (goal/approve/cancel/events), retire the dead bridge `Control` path | Path 3 user-visible | ✅ Assistant tab live (Gap A) |
| 3 | **Desktop on the kernel** — register WindowsAdapter + browser session on the real kernel path | Path 4 | ✅ `8f19e2f` |
| 4 | **Live recovery** — classify failures at the action-failure point, real decisions, escalate to human | Path 5 | ✅ `3e63361` |
| 5 | **Perception into the loop** — real sensor loop → WorldModel observation → salience decision → action | Path 2 | ✅ `3bf4645` |
| 6 | **Learning closed** — workflow durability + completion sink admits skills + timeline records | Path 6 | ✅ `07f8bb9` / `9ff78fb` |
| 7 | **Voice I/O** — ASR + TTS + wake loop | Path 1 (rides on host) | 🔨 S8-A `957b93e` (spoken turn → goal intake) + S8-B `34adb79` (whisper.cpp behind `SpeechToText`) + S8-C `29a72c3`/`930a0ff` (real mic wired into the live loop; live capture fixed) |

Steps 1–2 make the biggest user-visible jump from "organs" to "creature"; 3–6 deepen it; 7 is the fantasy layer. The 24 h endurance and Server FRIDAY stay parked until Paths 1–6 are live on this laptop.

> Authoritative per-gap record (design + acceptance): [`docs/STEP1-LIVE-HOST.md`](docs/STEP1-LIVE-HOST.md) Gaps A–G. `07f8bb9` = S7 code, `9ff78fb` = S7 receipt (Gap G). S8-A = spoken `TurnDetected` folded into perception + `VoiceIntake` (mic → `plan.goal`); benchmark (STT/TTS, `C:\Users\<user>\voice-bench`) chose whisper.cpp-base+CUDA and Audio8-0.6B/SAPI-fallback. S8-B = `WhisperCppTranscriber` (deployment config, not hardcoded paths) behind the frozen `SpeechToText` seam; GPU + CPU builds live-verified (`Open Chrome.`). S8-C = the real microphone wired into the live loop: `VoiceConfig` (Enabled/Disabled/Misconfigured, `FRIDAY_VOICE` + `FRIDAY_WHISPER_*` env), mic + whisper in `live_observer()` (WorldState fold) and the daemon's `VoiceIntake` spawn (plan.goal), voice is configuration never a requirement (no mic / misconfig logs loudly, daemon survives).

> **S8-C live (laptop, 2026-08-15):** ✅ **CLOSED.** Implementation ✅, boot ✅, real capture ✅, real transcription ✅, blank-audio rejection ✅ (`930a0ff`: whisper `[BLANK_AUDIO]` markers → silence, never a goal; journal wiped after debug runs). `930a0ff` fixed the dead-silent capture: the Realtek endpoint *accepted* the preferred 16 kHz mono client format but delivered only SILENT packets (endpoint meter moved, stream was digital zeros); capture now prefers the engine's mix format (48 kHz stereo float) and resamples to the 16 kHz contract, and reads the endpoint's mute/level/peak-meter so silence is attributable to the device. **Live acceptance passed (15:08, user-run daemon, quiet room, normal volume):** `"What is 2+2?"` → plan.goal → GoalRunner → `goal.completed` (plus `"Can you hear me?"`, `"(laughs)"`, `"ready?"` all transcribed accurately). Two external prerequisites had to be fixed first: Windows input volume → 100% (the endpoint meter now moves with normal speech; the VAD was never touched, stays 0.02) and no background audio (a YouTube video was being transcribed instead of the user). The goal pipeline answers via **heuristic fallback** because ollama is not running — a real answer needs an inference provider, which is S8-D's terrain. Journal wiped; S8-D starts clean.
