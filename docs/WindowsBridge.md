# Windows Bridge (M4.5.2) — Design

> **Status:** Design locked. No implementation code exists yet. This document is
> the contract; the code in `plugins/windows/` and `windows-host/` must conform
> to it. It is intentionally frozen like `Architecture.md` / `Execution.md`.

## 1. Principle

The Windows Bridge is a **device driver for Windows**, not a plugin with
business logic. One responsibility only: translate *intent* into Windows API
calls and normalize the result.

The Rust runtime must remain **completely platform-agnostic**. It never sees a
Win32 handle, an `HWND`, an `HRESULT`, or a Win32 error code. It only knows it
is issuing a *capability request* over a transport. Swapping in a Linux or macOS
bridge later requires **zero** changes to the planner, runtime, or AI layers —
only a different transport/host behind the same contract.

```
┌──────────────────────────────────────┐
│         FRIDAY Runtime (Rust)         │   platform-agnostic
└──────────────────────────────────────┘
                │
         Capability Request  (ActionRequest: tool="windows")
                │
      Windows Adapter (Rust)  ── plugins/windows/{bridge,ipc,actions}
                │
══════════════ IPC (Named Pipes, versioned) ══════════════
                │
      Windows Host Service (C#)  ── windows-host/
                │
 ┌──────────────┼───────────────┐
 │ Win32 API    │ UI Automation │
 │ COM          │ PowerShell    │
 └──────────────┴───────────────┘
                │
            Windows
```

## 2. Repository layout

### Rust side (lives in the workspace, platform-agnostic)

```
plugins/windows/
    Cargo.toml
    src/
        lib.rs            # registers WindowsAdapter with the runtime
        bridge.rs         # WindowsAdapter: implements ActionAdapter
        ipc.rs            # IpcTransport: Named Pipes (Win) / mock (tests)
        protocol.rs       # BridgeFrame envelope + protocol version
        actions.rs        # intent → normalized ActionRequest mapping
        manifest.rs       # Capability Manifest (static + discovery)
        error.rs          # BridgeError normalized set
        model.rs          # Window / Bounds / WindowState abstraction
        tests/            # unit + mock-host integration
```

`plugins/windows` is a workspace member. It depends only on `friday-core`,
`friday-actions`, and `tokio`/`serde`. It must **not** depend on any
Windows-only crate (no `windows`, no `windows-sys`). The only Windows-specific
code is the Named Pipe transport, selected at runtime by target OS; on
non-Windows builds it falls back to an in-process mock transport so the adapter
is testable everywhere.

### C# side (standalone microservice — separate tree, no Rust deps)

```
windows-host/
    Program.cs
    IPC/            # Named pipe server, frame codec
    Actions/        # one handler per intent (window.*, input.*, clipboard.*)
    Win32/          # P/Invoke to user32/gdi32
    UIAutomation/   # Inspect.exe-style element discovery
    COM/            # shell / COM interop
    PowerShell/     # privileged or composite operations
    Security/       # message validation, privilege drop, allow-list
```

The host contains **no planner, no memory, no AI, no permission logic**. It
receives an intent, performs the Windows call, and returns a normalized result
or error.

## 3. Wire protocol

Reuse `friday_core::protocol::{ProtocolMessage, Transport, ModuleId}` as the
correlated request/response envelope. The bridge adds exactly one field for
versioning — a `BridgeFrame` wrapper — so the frozen `ProtocolMessage` is
untouched:

```rust
#[derive(Serialize, Deserialize)]
pub struct BridgeFrame {
    pub protocol_version: u8,        // currently 1
    pub message: ProtocolMessage,    // core's correlated envelope
}
```

- **Transport:** Named Pipes on Windows
  (`\\.\pipe\friday-windows-<session>`). Rationale: native, fast, no ports, no
  firewall prompts, easy auth, excellent latency. No HTTP/WebSocket for local
  IPC.
- **Framing:** 4-byte little-endian length prefix + UTF-8 JSON `BridgeFrame`.
  Length-prefixing avoids JSON streaming ambiguity and enables auth handshake
  frames.
- **Versioning:** client sends `protocol_version`; if the host requires a
  different major version it rejects with `ProtocolError`. Adding fields to
  `payload` is a minor-version, backward-compatible change.
- **Non-Windows builds / tests:** `IpcTransport` uses an in-process channel or
  Unix socket to a Rust **mock host** that implements the same frames, so the
  adapter is exercised in CI without the C# service.

### Lifecycle mapping (async, intent → result)

Every action is **asynchronous** and maps onto the kernel lifecycle:

```
ActionRequest (windows.<intent>)
   → bridge accepts      -> LifecycleState::Running
   → host processes      -> (optional progress frames: "running")
   → host replies ok     -> LifecycleState::Completed  + ActionResult{success:true}
   → host replies error  -> LifecycleState::Completed(failed)
                              + ActionResult{success:false, error: BridgeError}
```

The bridge never blocks the runtime; it `await`s the IPC reply (with the
context's deadline/cancellation from M4.4.5) and lets the kernel drive
lifecycle. This matches the `LifecycleManager` from M4.4.5 exactly.

## 4. Window model

No raw handles cross the boundary. The host maps `HWND` → stable `Window.id`
internally; Rust only ever sees the abstraction:

```rust
pub struct Window {
    pub id: String,            // host-generated stable id (not HWND)
    pub title: String,
    pub process: String,       // process name / exe
    pub process_id: u32,
    pub bounds: Bounds,
    pub state: WindowState,    // Normal | Minimized | Maximized | Hidden
    pub monitor: u32,
}

pub struct Bounds { pub x: i32, pub y: i32, pub width: u32, pub height: u32 }
pub enum WindowState { Normal, Minimized, Maximized, Hidden }
```

## 5. Intent catalog (NOT APIs)

The bridge exposes **intent**, never Win32 names. Capability namespace
`windows.<intent>`.

| Intent | Maps to (host-internal only) |
| --- | --- |
| `windows.open` | launch app by path/name → returns `Window` |
| `windows.close` | close window / terminate process |
| `windows.focus` | bring to foreground |
| `windows.move` | set `Bounds.x/y` |
| `windows.resize` | set `Bounds.width/height` |
| `windows.capture` | screenshot → base64 PNG bytes |
| `windows.list` | enumerate visible windows → `Vec<Window>` |
| `windows.foreground` | get current foreground `Window` |
| `windows.click` | `Click(window_id, point)` (vision-ready) |
| `windows.type` | `Type(window_id, text)` |
| `windows.scroll` | `Scroll(window_id, delta)` |
| `windows.clipboard.read` | read clipboard text |
| `windows.clipboard.write` | write clipboard text |

`click`/`type`/`scroll` already carry a `Window.id` + coordinates so the future
Vision module can supply the target without the bridge changing. Operations
that would be "ShutdownComputer" etc. are executed verbatim — **the bridge does
not decide permission**; the decision was already made upstream by the
`PermissionEngine`.

## 6. Error model (normalized)

`BridgeError` is the only error type that crosses back to Rust. HRESULTs,
`GetLastError()` codes, COM `HRESULT`, and .NET exceptions are **never** leaked.

```rust
pub enum BridgeError {
    WindowNotFound,
    AccessDenied,
    ProcessNotRunning,
    Timeout,
    UIElementMissing,
    InvalidHandle,
    HostUnavailable,     // pipe down / no handshake
    ProtocolError,       // version/framing mismatch
    Unknown(String),
}
```

Mapping at the host boundary is the host's job (e.g. `ERROR_INVALID_WINDOW_HANDLE`
→ `InvalidHandle`; file-in-use / ACL → `AccessDenied`). The Rust side converts
`BridgeError` into an `ActionResult::error(...)`.

## 7. Capability Manifest

The host serves a manifest; the bridge ships a static copy and can refresh it at
connect time. This feeds M4.6 (Capability Registry + Negotiation) — the runtime
discovers windows capabilities dynamically instead of assuming them.

```json
{
  "bridge": "windows",
  "version": "1.0",
  "protocol_version": 1,
  "capabilities": [
    "windows.open", "windows.close", "windows.focus",
    "windows.move", "windows.resize", "windows.capture",
    "windows.list", "windows.foreground",
    "windows.click", "windows.type", "windows.scroll",
    "windows.clipboard.read", "windows.clipboard.write"
  ]
}
```

If the host reports a capability missing (e.g. `Vision: ✗ camera unavailable`),
the planner routes around it — no mid-plan failure.

## 8. Security boundary

- The bridge **never** makes allow/deny decisions. It executes whatever intent
  it is given. Permission was already enforced by `PermissionEngine` upstream.
- The C# host's `Security/` module validates frame shape, enforces the protocol
  version, and (later) may run with reduced privileges / an allow-list of
  executable paths. That is host-internal hardening, not runtime policy.

## 9. Testing strategy

1. **Unit** (cross-platform): `BridgeFrame` round-trip serde; `Window` model;
   `BridgeError` mapping; manifest parse/validate; intent→`ActionRequest`
   mapping.
2. **Mock-host integration** (cross-platform, CI): a Rust `MockHost` implements
   the Named-Pipe-equivalent frames and simulates an app lifecycle:
   `open → running → capture → close → exited`, asserting the bridge returns the
   correct `ActionResult` and drives `LifecycleManager`.
3. **Real Notepad harness** (Windows + C# host only, skipped otherwise): launch
   Notepad → verify window exists → focus → type text → capture contents →
   close → verify process exited. This is the baseline acceptance suite for the
   real bridge.

Tests that require the C# host skip silently when it is not present (same
pattern as the Git adapter), so CI stays green without Windows+C#.

## 10. Definition of Done (maps to the milestone's 8 requirements)

- ✅ Rust runtime remains completely platform-agnostic (no Win32 types in Rust).
- ✅ IPC uses Named Pipes with versioned (`BridgeFrame.protocol_version`) message contracts.
- ✅ C# host is responsible only for Windows API interaction.
- ✅ No Win32 types cross the IPC boundary (only `Window` / `ActionResult` / `BridgeError`).
- ✅ All operations are asynchronous and lifecycle-aware (kernel `LifecycleManager`).
- ✅ Errors normalized into `BridgeError` (no HRESULT / Win32 codes leak).
- ✅ Integration tests automate a real application (Notepad harness).
- ✅ A capability manifest is exposed for runtime discovery (feeds M4.6).

## 11. Out of scope (explicitly deferred)

- Vision-driven element discovery (the `click`/`type`/`scroll` intents are
  already shaped for it; the Vision module supplies coordinates later).
- Permission/policy inside the host (upstream only).
- Linux/macOS bridges (same contract, different host — enabled by this design,
  not built now).
