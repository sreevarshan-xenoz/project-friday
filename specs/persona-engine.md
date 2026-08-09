# Persona Engine Specification

**Status:** 🚧 Spec-first — implement against this file (v1 shipped 2026-08-08).
**Depends on:** none (pure data; `serde` only). The planner/conversation layer
consumes `StyleSpec` via `render_instruction`.
**Crate:** `services/persona` — crate `friday-persona`

## 1. Purpose

FRIDAY has a brain (planner + inference fabric). It needs a **character
layer** — not a chatbot personality, not a system prompt, but a deterministic
**Behavior Engine** that decides *how FRIDAY expresses herself* while the
planner decides *what to do*.

One identity, many personas, one continuous mood:

```text
Identity          never changes (name, core values, baseline style)
    │
Persona           changes automatically (behavioral profiles, blended)
    │
Mood              changes continuously (PAD vector from OCC appraisals)
    │
Response Style    the deterministic spec handed to the LLM
```

### 1.1 Design rule: one identity, multiple personas

Never multiple personalities. Personas share memory, values, knowledge and the
long-term relationship — **only expression changes**. The engine enforces this
by construction: `Identity` is immutable (no mutation path exists), so "still
truthful" holds even when the playfulness dimension is 1.0.

### 1.2 Design rule: personas are pure data

Exactly like skills: a persona contains **no prompts and no personality
prose** — only style dimensions, signal affinities, and a cooldown. The LLM
never decides the persona; it **receives** the rendered behavioral spec, so
swapping Gemma for Qwen/Claude/GPT changes the identity layer zero times.

### 1.3 Design rule: the LLM is not in the loop

The engine is fully deterministic (no LLM inside). It observes signals and
produces a `StyleSpec`. The planner passes that spec to whichever model is
active.

## 2. Architecture

```text
Sensors → Perception → Intent / Activity Detectors
                               │  Signal
                               ▼
                       PersonaEngine
                               │  StyleSpec (deterministic)
                               ▼
                      Planner → LLM → Actions
```

Input signals (all treated identically via `Signal.id` + optional appraisal):

- `Signal::conversation("coding" | "research" | "casual" | "farewell" | …)` —
  from the intent detector.
- `Signal::activity("ide_focused" | "build_failed" | "gaming" | …)` — from the
  environment/activity detector (focused app, build result, media playing).
- `Signal::emotional(id, Appraisal)` — conversation turns carrying an OCC
  appraisal (desirability / praiseworthiness / appeal) that feeds the PAD mood.

The environment is a first-class persona signal: VS Code + a failed build
shifts toward Engineer; browser + papers toward Researcher; music + weekend
toward Companion; Steam toward Pookie — silently, without announcement.

## 3. Types

### 3.1 `Identity` (immutable)

```rust
pub struct Identity {
    pub name: String,              // "FRIDAY"
    pub core_values: Vec<String>,  // truthful, protective, curious, competent, loyal
    pub style_baseline: StyleProfile, // calm, observant
}
```

No mutation path. The engine exposes it only as `&Identity`.

### 3.2 `StyleProfile` (pure data)

Continuous dimensions (`0.0..=1.0`) + two switches: `warmth`, `humor`,
`verbosity`, `challenge`, `precision`, `empathy`, `storytelling`,
`playfulness`, `speculation`, `emoji: bool`, `citations: bool`.

- `blend(other, w)` — weighted average; booleans flip at the ≥0.5 threshold.
- `adjusted_for_mood(&Mood)` — small, clamped deltas: pleasure → warmth/
  playfulness, arousal → verbosity, dominance → challenge/precision.
- `render()` — the model-agnostic spec block (`warmth: 0.42` lines, never
  prose).

### 3.3 `Persona` (pure data)

```rust
pub struct Persona {
    pub id: String,                // "engineer"
    pub description: String,       // label only, never instructions
    pub style: StyleProfile,
    pub affinity: HashMap<String, f32>, // signal id → activation delta
    pub cooldown_ms: u64,          // hold window once dominant
}
```

Bundled set: `engineer`, `researcher`, `companion`, `pookie` (playful — still
truthful). Any number of personas may be loaded as data.

### 3.4 `Mood` + `Appraisal` (OCC → PAD)

The consciousness-paper mechanism (`Simulating Consciousness in AI Agents`):
OCC appraisal classes feed a continuous PAD vector:

| OCC appraisal | PAD contribution |
|---|---|
| `desirability` (events) | pleasure (+), arousal (|·|) |
| `praiseworthiness` (agents) | dominance, slight pleasure |
| `appeal` (objects) | pleasure, arousal |

`Mood.decay(dt, half_life)` returns it exponentially toward neutral, so mood
influences voice/pacing/autonomy continuously without accumulating forever.

## 4. Engine dynamics (all smooth — never a light switch)

- **Energy**: each signal adds `affinity × 0.5` to the persona's energy; energy
  leaks **with elapsed time** (`× 0.8` per second — one clock with mood, so a
  long silence cools a persona the way it cools a mood). Repeated signals
  build deep context (the research state); a single off-topic line only nudges.
- **Hold (cooldown)**: the last-dominant persona leaks at `× 0.93` per second
  for its `cooldown_ms` window — it decays gradually instead of vanishing on
  the next unrelated signal, and cannot re-spike instantly.
- **Blend**: `select()` normalizes energies into weights (dropping shares < 2%
  and renormalizing), blends `identity.style_baseline` toward each persona's
  profile by its share, then applies the mood adjustment.
- **Mood**: appraisals move the PAD vector; `tick` decays it.

Result: `Research 0.91 → 0.83 → 0.71 → 0.55 → Companion rises` — the user
perceives emotional continuity, not a mode switch.

## 5. Output

```rust
pub struct StyleSpec {
    pub persona: String,             // dominant id
    pub blend: Vec<(String, f32)>,   // normalized, descending
    pub style: StyleProfile,         // blended + mood-adjusted
    pub mood: Mood,
}
```

`render_instruction()` renders the block the planner hands to the LLM:

```text
You are FRIDAY.
Core values (never overridden): truthful, protective, curious, competent, loyal
Behavioral spec for this response:
  persona: engineer
  blend: engineer 0.70, companion 0.30
  warmth: 0.34
  ...
  mood: pleasure 0.12 / arousal 0.20 / dominance 0.05
```

## 6. Invariants

1. `Identity` never changes — enforced by construction (no mutation path).
2. Core values are never overridden by any persona or mood.
3. A single unrelated signal never flips the dominant persona.
4. Persona transitions are gradual (energy + hold, never a hard switch).
5. All style dimensions stay in `0.0..=1.0`; all mood axes in `-1.0..=1.0`.
6. Blend weights normalize to 1.0.
7. Mood decays toward neutral over time.

## 7. Acceptance criteria (v1 shipped)

- [x] `friday-persona` builds clean (fmt/clippy/tests) — 22 unit + 1 doc test.
- [x] Identity immutability proven by test (`identity_is_immutable_under_abuse`).
- [x] "Still truthful" invariant proven under Pookie (`still_truthful_even_under_pookie`).
- [x] No-instant-flip proven (`one_unrelated_signal_does_not_flip_dominance`).
- [x] Gradual transition proven (`transition_to_a_new_persona_is_gradual`).
- [x] Cooldown re-engagement proven (`cooldown_prevents_instant_reengagement`).
- [x] OCC → PAD + decay proven (`mood_moves_on_appraisal_and_decays_via_tick`).
- [x] Environment-as-signal proven (`environment_signal_shifts_persona`).

## 8. Wiring seam (next slice, not built here)

The conversation engine (planned in `docs/Voice.md`) feeds `Signal::conversation`
/ `Signal::emotional`; the activity/environment detector feeds
`Signal::activity`; the LLM prompt builder (`planner/src/planning/planners/llm.rs`)
prepends `render_instruction()` to the system block. The engine itself stays
deterministic and model-agnostic.
