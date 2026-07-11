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
   Windows plugin.

## Models

- A dedicated **Vision Model** specialist (multimodal) is part of the model
  registry. Local-first on the RTX 3060, cloud fallback for heavy tasks.
- Embeddings of seen screens can be stored in the `Vector` memory layer for
  "find the window I had open earlier" recall.

## Integration

Vision produces `vision.result` events that the Planner and Windows plugin
consume to decide *how* to act. Reasoning (what to do) stays with the LLM;
execution (how to click/type) stays with the Windows plugin — keeping fragile
automation logic out of the model.

## Status

Planned (Roadmap M5). Event kinds reserved.
