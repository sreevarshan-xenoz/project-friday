# Plugin SDK

This is the contract every FRIDAY **bus plugin** implements. If you are adding a
capability, start here. The reference implementation is
`core/examples/echo_plugin.rs`.

> **Also see:** executable tools usually implement **`ActionAdapter`** in
> `friday-actions` (filesystem, terminal, git, MCP, …) and are dispatched by the
> AgentKernel after permission + audit — not only the `Plugin` trait. Capability
> advertising uses manifests / `CapabilityRegistry` (`friday-capabilities`).
> Specs: `specs/services.md`, `specs/kernel.md`, `specs/capabilities.md`.

## The trait

```rust
#[async_trait]
pub trait Plugin: Send + Sync {
    fn metadata(&self) -> &PluginMetadata;
    async fn init(&mut self, ctx: &PluginContext) -> Result<()> { Ok(()) }
    async fn handle_event(&self, event: &Event) -> Result<Vec<Event>>;
    async fn shutdown(&mut self) -> Result<()> { Ok(()) }
}
```

## Metadata (declares interests)

```rust
let meta = PluginMetadata::builder("echo", "Echo Plugin")
    .description("Replies to pings with pongs")
    .subscribes(&["demo.ping"])   // kinds the router forwards here
    .emits(&["demo.pong"])        // kinds this plugin may publish
    .dependencies(&[])            // plugin ids that must start first
    .build();
```

The router only delivers events whose `kind` matches one of `subscribes`. You
never receive events you did not ask for.

## Lifecycle

1. **Construct** the plugin with its `PluginMetadata`.
2. **Register** it: `friday.register_plugin(Box::new(plugin)).await?`. This calls
   `init(&ctx)` once, giving you the `EventBus` and shared `config`.
3. **Handle** matched events via `handle_event`. Return the events you want
   published back onto the bus to drive the next pipeline stage. Returning an
   empty `Vec` means "handled, nothing to do."
4. **Shutdown** is called on teardown.

## Emitting follow-up events

```rust
async fn handle_event(&self, event: &Event) -> Result<Vec<Event>> {
    let out = Event::new("demo.pong", &self.meta.id, event.payload.clone())
        .with_trace(event.trace_id);   // keep the pipeline correlated
    Ok(vec![out])
}
```

Always carry `trace_id` forward so the whole interaction can be traced.

## A complete example

See `core/examples/echo_plugin.rs`. Build and run it with:

```powershell
cargo run -p friday-core --example echo_plugin
```

## Cross-language plugins

Plugins may be written in any language. Non-Rust plugins communicate with the
core over the **protocol transport** (`core/src/protocol`) using the same JSON
event shape. A C# Windows plugin, for instance, connects via an IPC transport
and speaks `Event` JSON on the bus.
