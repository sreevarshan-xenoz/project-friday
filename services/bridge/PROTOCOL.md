# FRIDAY ReadModelBridge — Console Protocol v1

> This document **freezes the wire protocol** between the ReadModelBridge
> (`services/bridge`, `friday-bridge`) and the Engineering Console
> (`frontend/src/console`). It is the contract both sides implement. Phase 2
> of Bridge v2 (structured errors) and Phase 1 (capability handshake) are
> locked in here; Stages 3–4 (compression, auth + binary transport) are
> **forward-declared only** — they arrive after negotiation is proven.

The bridge is **read-only by construction**: it serializes the frozen
plugin-runtime/experience read models as JSON frames; it never mutates the
runtime. Clients never import FRIDAY internals — only protocol DTOs.

- Wire protocol version: **`PROTOCOL_VERSION = 1`**
- Transport: WebSocket, one JSON text frame per protocol frame
- Bridge identity (demo): `friday-bridge-demo`; mock identity: `mock-friday`
- Listen address: `ws://127.0.0.1:18433`

---

## 1. Frame model

Every frame carries the same metadata block:

| field       | type   | meaning                                              |
|-------------|--------|------------------------------------------------------|
| `kind`      | string | discriminates the payload (below)                    |
| `sequence`  | u64    | monotonic per-connection stream position (see §5)    |
| `timestamp` | u64    | bridge clock, milliseconds since Unix epoch          |
| `source`    | string | the `RuntimeSource` name                             |
| `protocol`  | u32    | wire protocol version (currently `1`)                |
| `bridge`    | string | bridge identity                                      |

### Frame kinds

| kind            | sender   | payload summary                                                    |
|-----------------|----------|--------------------------------------------------------------------|
| `hello`         | server   | bridge identity, protocol, `bridge_stats`, **capability set** (§3) |
| `snapshot`      | server   | full read models: dashboard, timelines, metrics, ledger            |
| `delta`         | server   | additive slices that changed since the last frame                  |
| `heartbeat`     | server   | liveness; `latency_ms` (measured RTT) + `bridge_stats`             |
| `error`         | server   | a problem + the machine-readable taxonomy (§4); stream continues   |
| `capabilities`  | server   | the negotiated capability set, acking a `hello_response` (§3)      |

Clients send **control messages** (see §6), never typed frames — a client that
sends a typed frame receives a structured `invalid_frame` error and the
connection stays open.

---

## 2. Protocol state machine

```
Disconnected
   │  TCP + WebSocket connect
   ▼
TCP Connected ── hello ──► Capability Negotiation
                              │  hello_response → capabilities ack
                              │  or: timeout (300 ms) — old client
                              ▼
                          Ready ── snapshot ──► Streaming
                                                │  delta / heartbeat
                        resync (client or lag)  ▼
                                                ├─► snapshot ──► Streaming
                                                │
                                                ▼
                                            Disconnect (close / error / timeout)
```

Every frame is legal only in certain states. A frame received out of state is
rejected with a structured `invalid_frame` error — never silently.

| frame / control            | TCP Connected | Negotiation | Ready | Streaming |
|----------------------------|:-------------:|:-----------:|:-----:|:---------:|
| `hello` (server)           |       ✓       |             |       |           |
| `capabilities` ack         |               |      ✓      |       |           |
| `snapshot`                 |               |             |   ✓   |   ✓ (resync) |
| `delta`                    |               |             |       |     ✓     |
| `heartbeat`                |               |             |       |     ✓     |
| `error`                    |       ✓       |      ✓      |   ✓   |     ✓     |
| control `hello_response`   |               |      ✓      |       |  ✓ (late; re-validated, acked, no transport effect) |
| control `resync`           |               |             |       |     ✓     |
| control (typed frame)      |               |             |       |  ✗ → `invalid_frame` |

`resync` before the initial snapshot is rejected with a structured
`invalid_frame` (not recoverable); a client that falls behind the broadcast
backlog is served a fresh snapshot automatically (§7).

---

## 3. Capability handshake (Bridge v2, Phase 1)

The handshake answers: **"what can we both do?"** Future features negotiate
instead of assume — the handshake is the feature gate, not a protocol version
bump.

1. The server advertises a **`CapabilitySet`** in its `hello`:

   ```json
   {
     "protocol_version": 1,
     "required":  [ { "name": "delta", "version": 1, "required": true },
                    { "name": "heartbeat", "version": 1, "required": true },
                    { "name": "resync", "version": 1, "required": true },
                    { "name": "errors", "version": 1, "required": true } ],
     "optional": [ { "name": "metrics", "version": 1, "required": false } ]
   }
   ```

   `required` capabilities every protocol-v1 console implicitly understands;
   `optional` ones a console may enable. `gzip`/`zstd`/`binary`/`auth` are
   declared but not implemented (Stages 3–4).

2. The console answers with a control message selecting the subset it wants:

   ```json
   { "type": "hello_response", "capabilities": { "metrics": true } }
   ```

3. The server resolves the selection (`CapabilitySet::resolve`): **only names
   the server advertised can ever be enabled**. On success it acks with a
   `capabilities` frame carrying the negotiated set (before the snapshot). An
   unknown name yields a structured `unsupported_capability` error (warning,
   recoverable) and the connection stays open.

4. A console that never answers is served with only the `required`
   capabilities after the negotiation window (`300 ms` default) — **old
   clients keep working unchanged**.

Flow: `Hello → Capabilities → client picks → server acks → snapshot → stream`.
Compression/auth will extend this same negotiation (Stage 3/4) rather than add
new transports.

---

## 4. Structured error taxonomy (Bridge v2, Phase 2)

`error` frames carry `message` **plus** a machine-readable classification so
clients react programmatically instead of parsing strings:

| field                | type             | meaning                                     |
|----------------------|------------------|---------------------------------------------|
| `message`            | string           | human-readable explanation                  |
| `code`               | ErrorCode        | snake_case classifier (below)               |
| `severity`           | `info`/`warning`/`error`/`critical` | how loudly to surface it    |
| `recoverable`        | boolean          | can the client simply retry?                |
| `recommended_action` | string           | what the client should do next, in words    |

### `ErrorCode`

| code                            | severity  | recoverable | recommended_action                                    |
|---------------------------------|-----------|-------------|-------------------------------------------------------|
| `unsupported_version`           | error     | false       | upgrade the client to a supported protocol version    |
| `unsupported_capability`        | warning   | true        | drop the capability from `hello_response` and retry   |
| `authentication_failed`         | error     | true        | provide a valid token and reconnect (Stage 4)         |
| `compression_negotiation_failed`| warning   | true        | fall back to an agreed codec or plain text (Stage 3)  |
| `invalid_frame`                 | warning   | true        | fix the frame; clients only send control messages     |
| `invalid_sequence`              | warning   | true        | resync with a fresh snapshot                          |
| `resync_required`               | warning   | true        | send `resync` for a fresh snapshot                    |
| `rate_limited`                  | warning   | true        | back off and retry after the rate window              |
| `internal_bridge_error`         | error     | false       | retry the connection                                  |

`auth`/`compression` codes are **forward-declared**: the taxonomy is frozen
now, the features arrive with later stages. An `error` frame never drops the
stream; a `critical`/unrecoverable error is followed by a close.

---

## 5. Sequencing guarantee

`sequence` is **per-connection, contiguous, and stamped at write time**: the
driver broadcasts deltas without a sequence and each connection loop assigns
the next number the moment it writes a frame. Broadcast frames, heartbeats,
negotiation acks, errors, and resync snapshots therefore arrive in a strictly
contiguous `N, N+1, N+2, …` stream, in the order they hit the wire. A client
sees a gap only when the bridge actually dropped a frame (a slot in the
broadcast backlog), which the console counts as `dropped`. The bridge keeps
**one** shared monotonic counter; the connection loop is the only task that
consumes it, so no cross-task reservation race can reorder a stream.

---

## 6. Client control messages

| message                                | purpose                                            |
|----------------------------------------|----------------------------------------------------|
| `{"type":"hello_response","capabilities":{name: bool}}` | capability negotiation (§3)          |
| `{"type":"resync"}`                    | request a fresh snapshot mid-stream                 |

A client that falls behind the bounded broadcast backlog (`256` slots default)
is resynced automatically with a fresh snapshot instead of a backlog of stale
frames.

---

## 7. Backpressure

The broadcast channel per connection is bounded (`with_broadcast_capacity`,
default `256`). When a slow console's backlog fills, the oldest frames are
dropped (`broadcast::RecvError::Lagged`) and the connection receives a fresh
snapshot — memory stays bounded per connection and a slow client only ever
catches up via resync.

---

## 8. Forward declarations (Stages 3–4, not yet implemented)

- **Compression** (Stage 3): `gzip` first, `zstd` optional — a new optional
  capability negotiated via the §3 handshake, applied at the
  `FrameSerializer` boundary (transparent to the wire DTOs). Only after the
  negotiation window exists: `Hello → Capabilities → client picks gzip →
  server acks → compressed frames`.
- **Authentication + binary transport** (Stage 4): token in hello/subprotocol
  (`auth` capability, `authentication_failed` code) and a binary frame
  encoding (`binary` capability). Local engineering console today, but the
  seam is frozen now.
- **Connection limits + idle timeout**: max concurrent connections; drop peers
  that never pong within the heartbeat window (already enforced today).
- **Version compatibility**: the server rejects incompatible clients with a
  structured `unsupported_version` error, never a silent failure.
