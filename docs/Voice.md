# Voice

Voice is the most natural front door to FRIDAY, but per the roadmap it is a
**plugin set**, not a core feature. The core only knows `voice.*` / `audio.*`
event kinds.

## Pipeline (as events)

```
audio.capture
   → audio.wake            (wake word detected)
   → audio.speech          (raw audio clip)
   → voice.transcript      (STT result)
   → conv.turn             (handed to conversation engine)
   → goal.raw              (intent extracted)
   → plan.request          (planner)
   ... pipeline ...
   → voice.speak           (TTS request)
   → audio.play            (audio output)
```

## Components

- **Wake Word Engine** — always-on, tiny model. Emits `audio.wake`.
- **Speech Recognition** — streaming STT. Emits `voice.transcript`.
- **Conversation Engine** — maintains tone and context, emits `goal.raw`.
- **Speech Model** — specialist model for natural TTS prosody.
- **Voice Reply** — TTS plugin consuming `voice.speak`.

## Design notes

- Local-first: wake word and STT run on-device using the RTX 3060.
- Cloud fallback only when local confidence is low.
- All voice I/O is logged (per the logging requirement) but transcripts are
  treated as sensitive and excluded from non-local memory tiers unless approved.
- The **Android console** and **mobile daemon** already provide a non-voice remote
  front door; voice is an additional modality, not a prerequisite for control.
- **Semantic Turn Detection (design note, pending execution-model ADR):** VAD is
  responsible only for speech activity + interruption; it has no opinion on
  sentence completeness. After streaming STT, a **Semantic Turn Detector**
  decides whether the user has finished expressing the thought before
  `voice.transcript` → `conv.turn` is emitted. This prevents false triggers on
  mid-utterance pauses ("Can you… …open VS Code"). `conv.turn` remains the
  canonical completed-utterance boundary — **no new event kind** is introduced.
- **Barge-in (design note, pending execution-model ADR):** interrupting TTS is
  one instance of a **generic cancellation contract**, not a voice-specific
  mechanism. Every long-running action is a `Task { id, owner, priority, state,
  cancellation_token }`; `voice.speak` / `audio.play`, `browser.navigate`,
  `terminal.execute`, `vision.scan`, `download.file` are all cancellable the
  same way. Barge-in flow: VAD detects speech → cancel active `voice.speak`
  task → re-arm capture → Semantic Turn Detection → `conv.turn`. A bespoke
  `audio.stop` is intentionally not defined here — it would duplicate the
  generic mechanism other organs will also need.

## Status

⏳ **Planned** — Roadmap **M8** (Voice).  
Event kinds reserved so earlier milestones can emit compatible events.
Semantic Turn Detection and barge-in are captured as design notes; interruption
ships via the generic task-cancellation execution model, not a voice-only path.
