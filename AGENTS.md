# AGENTS.md — guide for agents and contributors

## What FRIDAY is

A standalone desktop assistant for **Windows and Linux**: one Rust core, a
thin host per platform, installable on an existing machine. FRIDAY OS (the
Arch ISO) is the reference Linux setup, not the only way in.

**Core use case (before everything else):** press a key, ask about what's on
screen or selected, get a useful answer or action, reliably, on both platforms.

**System integration, not kernel code:** UI Automation / ETW / Win32 on
Windows; AT-SPI / portals / compositor IPC / eBPF / D-Bus / systemd on Linux.
No kernel drivers or modules unless nothing else can do it.

| Part | Where |
|---|---|
| Core + daemon | Rust workspace (`core`, `planner`, `services/*`). `friday-linux-daemon` (`services/mobile`; shipped as `friday-daemon.exe` on Windows) runs goals and publishes `state.json` (`~/.local/state/friday`, `%LOCALAPPDATA%\friday`). Default AI: OpenCode Zen's free models; a user key takes precedence. |
| Linux | `friday` CLI (`packaging/linux/friday`), FRIDAY Shell (`packaging/linux/shell`, Quickshell), Hyprland binds Super+Y / Super+Alt+Y / Super+Ctrl+Y, all in the `friday-git` Arch package (`packaging/arch`). |
| FRIDAY OS | `iso/friday/`. Default desktop: FRIDAY Shell on Omarchy's pinned Hyprland config; Omarchy's shell keeps lock/idle/polkit with its bar, notifications and OSD off (`friday-os/plugins/friday.bar`). end-4's Dots-Hyprland is a vendored, non-default option: don't copy its look or code. |
| Windows | `friday-tray.exe` (`windows-tray/`, .NET 8): per-user tray app, hotkeys Win+Y / Win+Alt+Y / Win+Ctrl+Y, ask box, supervises the daemon and `windows-host/` (C#: UIA, COM, Win32). Per-user MSI in `installer/windows/` (no service: session 0 can't see the desktop). |

## Plan

Top to bottom; don't start a later phase while an earlier one has known failures.

1. **First hour works.** Linux: `build-iso.sh` → VM install → login → Super+Y
   answers (checklist: `docs/HANDOFF-2026-11-04.md`). Windows: MSI on a clean
   machine → Win+Y → answer. Both still need a real-hardware pass.
2. **Automate it.** Every bug found by hand becomes a test. CI covers the
   desktop checks, the Arch package, the MSI (build, install, smoke,
   uninstall), an ask answered end to end (`tests/e2e/ask.sh`: real daemon,
   fake model) and the three hotkeys from keymap to answer (`tests/e2e/hotkeys.*`)
   on Linux and Windows; a `release.yml` dry run on packaging changes.
3. **Make the core use case excellent:** context, answer quality, actions,
   latency, clear failures.
4. **Standalone packages:** `friday-git` and the per-user MSI exist; a `vX.Y.Z`
   tag publishes them and the ISO as a GitHub release (`release.yml`), with winget
   manifests ready to submit.
5. **Deeper integration** through the interfaces above, each behind consent.
6. **Only then** `docs/roadmap/ecosystem-backlog.md`.

Phases 3 to 5 follow `docs/roadmap/vnext.md` ("your work, remembered":
workspaces with session resurrection, thought threads, project
understanding, modes, sovereignty), each milestone with its exit test. The
owner-approved queue of twenty improvements from the v0.7.3 brainstorm is
`docs/roadmap/after-0.7.3.md`, worked top to bottom; the second round,
`docs/roadmap/after-0.8.0.md`, follows it; the third, from the 12-repo
brainstorm, is `docs/roadmap/after-0.8.2.md`.

## Rules

- **Verify before saying done** (commands below), and say what wasn't verified.
  Anything the checks can't see goes through the VM (Linux) or the Windows CI job.
- **One change per commit**, conventional messages (`feat(...)`, `fix(...)`).
- **Contracts stay narrow:** the event bus / IPC and `state.json` (`specs/`).
  The daemon acts only on an explicit request; screen capture needs per-use
  consent. Never print or log secrets.
- **FRIDAY ships as packages** (`friday-git`, the MSI), on FRIDAY OS too: its
  files never go in `airootfs/`. ISO-only changes go under `airootfs/`; new
  packages go in `packages.x86_64` and/or the installer's pacstrap list and
  must exist in the Arch repos.
- **Upstreams are pinned:** Omarchy at the commit in `fetch-omarchy.sh`; bump
  a pin on purpose, in its own commit. A fix the pin needs before then is a
  patch in `iso/friday/omarchy-patches/` (applied on top, in order).
- **A subsystem earns its place:** a new crate or module needs one clear job,
  a call from the running daemon or a host, behaviour a person would notice,
  tests that prove it, and a reason it belongs in FRIDAY rather than an
  external service. Interesting isn't a reason (`docs/design/MODULE-AUDIT-2026-11.md`).
- **No build output in git:** ISOs, `target/`, `bin/`, `obj/`, packages, MSIs,
  `airootfs/opt/friday/repo/`.

## Commands

| What | Command |
|---|---|
| Rust | `cargo test -p <crate>`; CI also runs fmt and clippy `-D warnings` |
| End-to-end ask | `bash tests/e2e/ask.sh` (after `cargo build -p friday-mobile --bin friday-linux-daemon --bin friday-ctl`) |
| Core-ask eval | `bash tests/e2e/core-eval.sh` (same build; Linux): 30 fixed selection, page, screen and private asks, pass rate in CI's job summary |
| End-to-end hotkeys | `bash tests/e2e/hotkeys.sh` (same build; needs lua, jq); Windows: `pwsh tests/e2e/hotkeys.ps1` on an installed MSI |
| Desktop/installer checks | `bash iso/friday/test/run-checks.sh` (Arch; deps in the script header) |
| Arch package | `cd packaging/arch && makepkg -si`; smoke: `bash packaging/arch/check-package.sh <pkg>` |
| ISO | `CLEAN_WORK=true bash iso/friday/build-iso.sh` (Arch, normal user; builds `friday-git`, then sudo mkarchiso) |
| Test VM | `bash iso/friday/test/run-qemu.sh --headless --daemon --reset-disk`; drive with `python3 iso/friday/test/vm-control.py` |
| Windows tray tests | `dotnet test windows-tray/FridayTray.Tests` (any OS) |
| Windows MSI | `pwsh installer/windows/build.ps1` (Windows); smoke: `friday-tray.exe --smoke` |
| Release | push a tag `vX.Y.Z` (≥ 0.2.0; `vX.Y.Z-alpha` for a pre-release): `release.yml` builds, tests and publishes the package, MSI and ISO |
| Health | `friday doctor` (Linux) |

Gotcha: `vm-control.py wait-ready` reports SSH up before the guest is; poll a
real `ssh … uptime`.
