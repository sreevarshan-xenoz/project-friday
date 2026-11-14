# FRIDAY

A standalone desktop assistant for Windows and Linux: one Rust core, a thin host per platform, installable on an existing machine. FRIDAY OS (the Arch ISO) is the reference Linux setup, not the only way in.

Core use case (before everything else): press a key, ask about what is on screen or selected, get a useful answer or action, reliably, on both platforms.

System integration, not kernel code:
- Windows: UI Automation, ETW, Win32
- Linux: AT-SPI, portals, compositor IPC, eBPF, D-Bus, systemd

No kernel drivers or modules unless nothing else can do it.

| Part | Description |
|---|---|
| Core and daemon | Rust workspace (`core`, `planner`, `services/*`). `friday-linux-daemon` (`services/mobile`; shipped as `friday-daemon.exe` on Windows) runs goals and publishes `state.json`. |
| Linux | `friday` CLI (`packaging/linux/friday`), FRIDAY Shell (`packaging/linux/shell`, Quickshell), Hyprland and niri desktop hooks, all in the `friday-git` Arch package (`packaging/arch`). |
| Windows | `friday-tray.exe` (`windows-tray/`, .NET 8): per-user tray application, ask dialog, supervises the daemon and `windows-host/` (C#: UI Automation, COM, Win32). Per-user MSI in `installer/windows/`. |
| FRIDAY OS | `iso/friday/`: Reference Arch ISO. Default desktop runs FRIDAY Shell on Omarchy's pinned Hyprland configuration or niri (`friday-os/plugins/friday.bar`). |

## Hotkeys

Global hotkeys trigger the assistant on each platform:

### Linux (Hyprland and niri)

- `Super+Y`: Ask prompt (`friday ask`)
- `Super+Alt+Y`: Ask about selected text (`friday ask --selection`)
- `Super+Ctrl+Y`: Ask about what is on screen (`friday ask --screen`)

Keybinding configurations are provided in `packaging/linux/hyprland/friday.lua` and `packaging/linux/niri/friday.kdl`.

### Windows

- `Win+Y` (fallback: `Ctrl+Alt+Y`): Ask prompt
- `Win+Alt+Y` (fallback: `Ctrl+Alt+Shift+Y`): Ask about selected text
- `Win+Ctrl+Y` (fallback: `Ctrl+Alt+Shift+S`): Ask about what is on screen

Hotkeys are managed by `friday-tray.exe`. If the primary Windows key combination is unavailable, the tray registers the fallback shortcut.

## AI Provider and Privacy

- **Default AI:** free models work out of the box, with no key or sign-in: OpenCode Zen first, then Kilo's free gateway as the fallback. A key of your own takes precedence (`friday setup-ai` on Linux, the tray's AI provider window on Windows).
- **Your own account:** on Linux, `friday connect claude|codex|agy|cline` answers with Claude Code, Codex, Antigravity or Cline, through that tool's own app and sign-in (FRIDAY never sees the credentials, and runs it with no tools). Signing in needs a browser, so it happens after the first login, not in the ISO installer.
- **Free models and your data:** free models may keep what you send them (questions, selected text, screenshots) and may use it for training. For anything private, use your own key or account.
- **Per-use screen consent:** A screenshot is sent to the model only for a screen ask, and pressing the screen hotkey is that ask's consent.
- **Action confirmation:** The daemon acts only on explicit request; actions ask before execution.
- **Narrow contracts and secrets:** Interfaces are limited to narrow event bus and IPC contracts (`specs/ipc.md`, `specs/events.md`) and `state.json`. Secrets are never logged or printed.

## Installation

### Arch Linux

Build and install the `friday-git` package from `packaging/arch`:

```bash
cd packaging/arch && makepkg -si
```

The package installs `friday-linux-daemon`, `friday-ctl`, the `friday` CLI wrapper, systemd user services, compositor keybindings, and FRIDAY Shell.

Start the daemon service:

```bash
systemctl --user enable --now friday-linux-daemon
```

### Windows

Install using the per-user MSI package (`FRIDAY-<version>-x64.msi`):

```powershell
msiexec /i FRIDAY-<version>-x64.msi
```

- Installs to `%LOCALAPPDATA%\Programs\FRIDAY` without requiring administrator privileges.
- Adds `friday-tray.exe` to user startup (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`).
- Adds executables to the user `PATH` and Start menu.
- Runs entirely in the user desktop session to interact with UI Automation, capture, and hotkeys.

Silent install for automation:

```powershell
msiexec /i FRIDAY-<version>-x64.msi /qn
```

### FRIDAY OS

FRIDAY OS is the reference Linux setup provided as an Arch Linux ISO image. Build the ISO with:

```bash
CLEAN_WORK=true bash iso/friday/build-iso.sh
```

The builder compiles `friday-git` and installs it onto the live media image.

## Plan

Project roadmap and delivery phases are defined sequentially in [AGENTS.md](AGENTS.md):

1. First hour works (Linux VM and Windows clean install verification)
2. Automate it (CI coverage for checks, packages, MSI, end-to-end tests)
3. Make the core use case excellent (context, answer quality, actions, latency)
4. Standalone packages (`friday-git` and MSI releases)
5. Deeper integration (system interfaces behind consent)
6. Ecosystem backlog ([docs/roadmap/ecosystem-backlog.md](docs/roadmap/ecosystem-backlog.md))

Later phases do not begin while earlier phases have known failures.

## Repository Layout

```
FRIDAY/
├── core/             Rust core workspace (friday-core: event bus, memory, protocol, runtime)
├── planner/          Planning and reasoning engines (friday-planner)
├── services/         Background capability services (daemon in services/mobile, action runtime, skills)
├── packaging/        Linux packaging and shell
│   ├── arch/         Arch Linux friday-git PKGBUILD, install scripts, and smoke checks
│   └── linux/        friday CLI, systemd user service, compositor binds, and FRIDAY Shell
├── installer/        Windows installer
│   └── windows/      WiX v5 MSI definitions, build script, and winget manifests
├── windows-tray/     Per-user Windows tray application (.NET 8, hotkeys, process supervisor)
├── windows-host/     Windows desktop integration host (C#: UI Automation, Win32, COM, capture)
├── friday-os/        FRIDAY OS bar and desktop integration plugins
├── iso/              Reference Linux system builder
│   └── friday/       Arch ISO profile and build scripts (build-iso.sh)
├── tests/            Integration and end-to-end tests
│   └── e2e/          End-to-end ask and hotkey test suites for Linux and Windows
├── specs/            Source-of-truth subsystem specifications, IPC protocols, and ADRs
└── docs/             Architecture documentation, handoff checklists, and guides
```

## Build and Verification

Primary build and test commands:

| Target | Command |
|---|---|
| Rust unit and workspace tests | `cargo test -p <crate>` |
| End-to-end ask test (Linux) | `bash tests/e2e/ask.sh` |
| End-to-end hotkeys test (Linux) | `bash tests/e2e/hotkeys.sh` |
| Windows tray tests | `dotnet test windows-tray/FridayTray.Tests` |
| Windows MSI build | `pwsh installer/windows/build.ps1` |
| Windows hotkeys test | `pwsh tests/e2e/hotkeys.ps1` |
| Desktop and installer checks | `bash iso/friday/test/run-checks.sh` |
| Arch package build and smoke test | `cd packaging/arch && makepkg -si` |
| FRIDAY OS ISO build | `CLEAN_WORK=true bash iso/friday/build-iso.sh` |
| Health check (Linux) | `friday doctor` |

See the Commands table in [AGENTS.md](AGENTS.md) for full options and VM test procedures.

## Documentation

- [AGENTS.md](AGENTS.md): Guide for agents and contributors, rules, and commands
- [docs/HANDOFF-2026-11-04.md](docs/HANDOFF-2026-11-04.md): Linux first-hour verification checklist
- [docs/Architecture.md](docs/Architecture.md): Architecture overview and event-driven model
- [docs/Security.md](docs/Security.md): Security model, secrets, and permission boundaries
- [docs/roadmap/ecosystem-backlog.md](docs/roadmap/ecosystem-backlog.md): Ecosystem backlog
- [specs/README.md](specs/README.md): Subsystem specifications index
- [specs/ipc.md](specs/ipc.md): Daemon IPC protocol specification
- [specs/events.md](specs/events.md): Event bus vocabulary and schemas
- [installer/windows/README.md](installer/windows/README.md): Windows installer architecture and verification
- [tests/README.md](tests/README.md): Integration test suite organization

## License

Dual-licensed under the MIT License ([LICENSE-MIT](LICENSE-MIT)) or the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)).
