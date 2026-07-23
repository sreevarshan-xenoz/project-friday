# Vision System

The vision subsystem lets FRIDAY *see* the desktop — reading the screen the way a
human glances at a window. Like everything else, it is a plugin set, not core
functionality.

## Event kinds

- `vision.frame` — a captured screen/image frame (payload: encoded image or
  path + region).
- `vision.query` — a question about the current frame.
- `vision.result` — structured answer (bounding boxes, OCR text, UI element
  tree).

## Components

1. **Screen Capture** — efficient DXGI/GDI desktop duplication on Windows.
2. **OCR** — extract text and layout from frames.
3. **Scene Understanding** — a vision model identifies windows, buttons, fields,
   and their state.
4. **UI Element Tree** — maps what is seen to actionable controls for the
   Windows service.

## Models

- A dedicated **Vision Model** specialist (multimodal) is part of the model
  registry direction under `services/intelligence`. Local-first on the RTX 3060,
  cloud fallback for heavy tasks.
- Embeddings of seen screens can be stored in the `Vector` memory layer for
  "find the window I had open earlier" recall.

## Integration

Vision produces `vision.result` events that the Planner and Windows bridge
consume to decide *how* to act. Reasoning (what to do) stays with the LLM /
intelligence layer; execution (how to click/type) stays with the Windows
service — keeping fragile automation logic out of the model.

The Windows bridge already exposes vision-ready intents
(`windows.click` / `type` / `scroll` / `capture`) so vision can land as a thin
perception layer on top of an existing execution path.

## Status

⏳ **Planned** — Roadmap **M7** (Desktop World Model / Vision).  
Event kinds reserved. Not required for Alpha Qualification of existing services.
