# M16: First Real Observation

Split into four independently shippable milestones.

---

## M16.1: Event Infrastructure

**Scope:** Only event system changes.

- `WorkspaceEvent` redesign (strongly typed `Topic`, `EngineId`)
- New `publish()` API with auto-generated metadata
- `EventRecorder` (append-only JSONL)
- Replay CLI tool
- Update all 12 construction sites

**Definition of Done:**
- All tests pass.
- JSONL file is written.
- `cargo run --bin friday-replay data/events/today.jsonl` prints events.
- Zero perception code changed.

---

## M16.2: First Real Sensor

**Scope:** Only sensor + perception changes.

- `friday-sensors` crate
- `windows_focus` module (Win32 polling, normalized events)
- `friday-perception` refactored to use real sensor

**Definition of Done:**
```
Alt+Tab
    ↓
sensor.window.focus_changed
    ↓
Workspace
    ↓
JSONL
```

---

## M16.3: Passive Memory

**Scope:** Memory becomes event-driven.

- Memory subscribes to workspace instead of accepting direct calls
- Delete all `memory.store(...)` outside the memory engine

**Definition of Done:**
- Memory observes workspace events.
- No direct memory calls from other engines.

---

## M16.4: First End-to-End Pipeline

**Scope:** One integration test proving the full pipeline.

```
User changes focused window
    ↓
Sensor
    ↓
Workspace
    ↓
Recorder
    ↓
Memory
    ↓
Evaluation
```

**Definition of Done:**
- One integration test: `window_focus_creates_memory_episode`.
- That single test proves the architecture works.
