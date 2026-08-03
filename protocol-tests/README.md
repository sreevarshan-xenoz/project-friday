# Protocol Compatibility Suite

Golden, byte-frozen samples of the FRIDAY ReadModelBridge wire protocol
(**v1**), locked by two independent guards so the contract stays stable for
years — every future bridge implementation must successfully parse every
historical frame.

```
protocol-tests/
  v1/
    hello.json                   server → client: identity + counters + capability set
    capabilities.json            server → client: negotiation ack
    snapshot.json                server → client: full read models (5-decision demo, seed 42)
    delta.json                   server → client: additive change (executive phase advance)
    heartbeat.json               server → client: liveness + measured round-trip + counters
    error.json                   server → client: machine-readable error taxonomy
    control-hello-response.json  client → server: capability negotiation
    control-resync.json          client → server: fresh snapshot request
  check.mjs                      framework-agnostic conformance checker (plain Node)
```

## What the fixtures lock

- **Frame grammar** — `kind`/`sequence`/`timestamp`/`source`/`protocol`/`bridge`
  on every frame; per-kind payloads (dashboard/timelines/metrics/ledger,
  `bridge_stats`, capability set, error taxonomy).
- **Negotiation contract** — the required (`delta`, `heartbeat`, `resync`,
  `errors`) and optional (`metrics`) capability sets and the client
  `hello_response` shape that answers them.
- **Error taxonomy** — `code`/`severity`/`recoverable`/`recommended_action`
  on `error` frames, per `services/bridge/PROTOCOL.md` §4.
- **Broadcast semantics** — `delta.json` carries `sequence: 0` because the
  driver publishes deltas without a sequence; the connection loop stamps them
  at write time (PROTOCOL.md §5), so a client never sees a real `0` on the
  wire.

## The two guards

1. **Rust (byte-lock)** — `services/bridge/tests/protocol_compat.rs` parses
   every frame fixture losslessly and asserts that re-serializing reproduces
   the golden bytes exactly. A silent wire-format drift fails the build.
   Control fixtures are validated structurally.

2. **Node (conformance)** — `check.mjs` asserts the fixture shapes against the
   frozen contract with zero dependencies, so a bridge implemented in another
   language can vendor it as its own parse gate:

   ```bash
   node protocol-tests/check.mjs
   ```

## Regenerating

Fixtures come from `services/bridge/examples/dump_protocol.rs`, which builds
every frame through the real constructors (`SnapshotPublisher`,
`DeltaPublisher`, `Heartbeat`, `FrameSerializer`) with a fixed timestamp, so
output is byte-reproducible. Regenerate only after an **intentional** wire
change — and commit the new bytes together with that change, since the
byte-lock test will fail otherwise:

```bash
cargo run -p friday-bridge --example dump_protocol
node protocol-tests/check.mjs
cargo test -p friday-bridge --test protocol_compat
```
