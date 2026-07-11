# IPC Specification — Windows Bridge

The Windows Bridge is **just IPC**: a device driver for Windows. Rust never sees an
`HWND` or `HRESULT`. Platform-agnostic Rust → Named Pipes → standalone C# host →
Win32 / UI Automation / COM / PowerShell.

## Rust side — `plugins/windows` (`friday-windows`)

- `WindowsAdapter` implements `ActionAdapter` (`tool = "windows"`).
- `BridgeFrame` wraps `friday_core::protocol::ProtocolMessage` and carries a
  `protocol_version` for forward/backward compatibility.
- Transport: Named Pipes on Windows; a `MockTransport` is used in cross-platform
  tests (no Windows host required to build/test).
- Errors normalized into a fixed `BridgeError` set (never raw Win32 codes).
- Exposes **intent**, not Win32 APIs: `windows.open/close/focus/move/resize/
  capture/list/foreground`, vision-ready `windows.click/type/scroll`, and
  `windows.clipboard.*`.
- Ships a `CAPABILITY_MANIFEST` (feeds Capability Intelligence, M4.6).

## C# side — `windows-host/`

A standalone microservice (not compiled in this Rust environment; `dotnet` absent):

```
Program.cs            — host entry / pipe server
IPC/BridgeFrame.cs     — frame (de)serialization, protocol_version check
Actions/*.cs           — per-intent handlers
Win32/Native.cs        — P/Invoke
UIAutomation/          — element discovery + control
COM/                   — Office/Shell COM
PowerShell/            — script execution
Security/Validator.cs  — permission + sandbox checks before any Win32 call
Manifest.json          — capability manifest (mirrors Rust side)
```

## Invariants

- Every action is **async + lifecycle-aware** (maps onto `LifecycleManager`).
- The host validates permissions (`Security/Validator.cs`) before invoking Win32.
- A real-Notepad harness exists but is **skipped in CI** (opt-in via
  `FRIDAY_WINDOWS_HOST_TEST=1`); CI relies on `MockTransport` tests.

## See also

- `docs/WindowsBridge.md` (locked design).
- `ipc.md` in `docs/Architecture.md` addendum.
