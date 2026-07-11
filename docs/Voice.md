# Voice

Voice is the most natural front door to FRIDAY, but per the roadmap it is a
**plugin**, not a core feature. The core only knows `voice.*` event kinds.

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

## Status

Planned (Roadmap M5). The event kinds above are reserved so earlier milestones
can emit compatible events.
