# FRIDAY Clients

FRIDAY is a **distributed platform**. `services/mobile` is the single phone-facing
surface; everything else (Planner, Inference, Recovery, Capabilities, Windows,
Filesystem, Git, Terminal) is reached only through it. The wire contract between
clients and that service is the **Mobile Protocol** (`services/mobile/src/protocol.rs`)
— and it is now treated as a versioned **API**.

## The protocol is an API

- Tagged JSON over WebSocket (`"type"` discriminator, `snake_case` variants).
- Handshake announces versions both ways: `Hello.protocol` → `Welcome.protocol`.
- Current version: **`1.0`** (`PROTOCOL_VERSION` in `protocol.rs`).
- **Versioning policy:** bump `1.0 → 1.1 → 2.0` only with a deliberate, documented
  change. Never break a shipped version casually. Add fields with defaults so old
  clients keep working. A client speaking an unknown/unsupported version is
  rejected or negotiated by the server, not guessed at.

## Clients

| Path | Status | Notes |
| ---- | ------ | ----- |
| `android/` | Archived (2026-11-28) | Moved to its own repo, friday-mobile |
| `desktop/` | not planned | The desktop UI is FRIDAY Shell (Linux) and the tray (Windows) |
| `web/` | planned | future (dashboard/tablet companion) |
| `tablet/` | planned | future |

Every client implements the same six qualification gates (see
`specs/client-qualification.md`): Build · APK · Lint · Instrumentation · Protocol
· Manual Demo.

## Pre-voice capability: Session Transfer (M6.2.7)

Before voice (M6.3), add **Session Transfer**: a task in flight on the Desktop
continues seamlessly on the Phone and back. That makes FRIDAY feel like one system
rather than two devices. It is a protocol capability (a `session.transfer`
command/event), not client-specific logic.

## Running the daemon

```powershell
cargo run -p friday-mobile --bin friday-mobile-daemon   # 0.0.0.0:18432
```

Pairing auth is **on by default** (QR-pairing for phones). For local dev or the
web portal, open mode disables the challenge so clients connect directly:

```powershell
$env:FRIDAY_MOBILE_AUTH = "off"   # off | 0 | false | no
cargo run -p friday-mobile --bin friday-mobile-daemon
```

## Don't put FRIDAY logic in clients

Clients know only protocol DTOs — never TaskGraph, RecoveryGraph, provider
routing, or AgentKernel. Keep them thin; the platform stays on the PC.
