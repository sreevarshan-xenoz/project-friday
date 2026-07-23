# API

FRIDAY is a **Cargo workspace**. The foundational public API is `friday-core`;
higher subsystems are separate crates that depend on it.

| Crate | Path | Role |
| ----- | ---- | ---- |
| `friday-core` | `core/` | Event bus, plugins, memory, protocol, runtime |
| `friday-planner` | `planner/` | Intent / planning / execution / verification engines |
| `friday-actions` | `services/action-runtime/` | AgentKernel + adapters |
| `friday-capabilities` | `services/capabilities/` | Registry, ranking, orchestrator, cost |
| `friday-simulation` | `services/simulation/` | Plan simulation |
| `friday-context` | `services/context/` | Context Builder |
| `friday-intelligence` | `services/intelligence/` | LLM providers / router |
| `friday-recovery` | `services/recovery/` | Recovery graph |
| `friday-benchmark` | `services/benchmark/` | Resilience harness |
| `friday-mobile` | `services/mobile/` | Mobile protocol daemon |
| `friday-mcp` | `mcp/` | MCP client / transport / discovery |
| `friday-windows` | `plugins/windows/` | Windows bridge client |

Import core with:

```toml
friday-core = { path = "../core" }
```

Subsystem contracts and event vocabulary: **`specs/`** (especially
`specs/events.md`, `specs/kernel.md`). This document covers the core surface.

## Runtime

```rust
use friday_core::Friday;

let mut friday = Friday::new();
friday.register_plugin(Box::new(my_plugin)).await?;
friday.register_memory_layer(Box::new(my_layer));
friday.run().await?;              // runs until the bus closes
friday.run_with_timeout(d).await?; // runs until timeout (tests/demos)
```

## Event bus

```rust
use friday_core::event::{Event, EventBus, Priority};

let bus: EventBus = friday.bus().clone();
let mut rx = bus.subscribe();
bus.publish(
    Event::new("voice.transcript", "mic", serde_json::json!({ "text": "open notepad" }))
        .with_priority(Priority::High),
).await?;
let event = rx.recv().await?;
```

- `Event { id, kind, source, payload, timestamp, trace_id, priority, hops }`
- `EventBus::subscribe()` — full-stream receiver.
- `EventBus::publish(event)` — broadcast to all subscribers.
- `MAX_HOPS` guards against event loops.

## Plugin SDK

```rust
use friday_core::plugin::{Plugin, PluginContext, PluginMetadata};
use async_trait::async_trait;

#[async_trait]
impl Plugin for MyPlugin {
    fn metadata(&self) -> &PluginMetadata { &self.meta }
    async fn init(&mut self, ctx: &PluginContext) -> friday_core::Result<()> { Ok(()) }
    async fn handle_event(&self, event: &Event) -> friday_core::Result<Vec<Event>> { Ok(vec![]) }
}
```

## Memory

```rust
use friday_core::memory::{MemoryEngine, MemoryEntry, MemoryLayerKind};

let entry = MemoryEntry::new(MemoryLayerKind::Session, "last_cmd", serde_json::json!({"cmd":"ls"}));
friday.memory().store(&entry).await?;
let found = friday.memory().retrieve(MemoryLayerKind::Session, "last_cmd").await?;
```

Implement `MemoryLayer` to add a backend; register with
`Friday::register_memory_layer`.

## Protocol (request/response)

```rust
use friday_core::protocol::{ModuleId, ProtocolMessage, Transport};

let req = ProtocolMessage::request(
    ModuleId::new("planner"), ModuleId::new("coder"),
    "code.generate", serde_json::json!({ "task": "..." }),
);
let resp = req.reply(serde_json::json!({ "diff": "..." }));
assert!(resp.is_response());
```

Implement `Transport` to bridge languages (in-process channel, IPC, network).
