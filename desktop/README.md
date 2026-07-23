# desktop/

Historical placeholder for "native Windows integration."

**The live Windows host is `windows-host/`** (C# / .NET 8): Win32, UI Automation,
COM, PowerShell, clipboard, window management, and input. It speaks versioned
`BridgeFrame` messages over named pipes to the Rust bridge client in
`plugins/windows` (`friday-windows`).

| Piece | Path | Status |
| ----- | ---- | ------ |
| C# host microservice | `windows-host/` | ✅ Implemented |
| Rust bridge + capability manifest | `plugins/windows/` | ✅ Implemented |
| Design | `docs/WindowsBridge.md`, `specs/ipc.md` | Locked |
| Qualification harness | `services/windows-qual` | ✅ In use |

This `desktop/` folder remains as a layout landmark only. Prefer
`windows-host/` and `plugins/windows/` for code and docs links.

Status of the **React + Tauri** product UI is separate — see `frontend/`.
