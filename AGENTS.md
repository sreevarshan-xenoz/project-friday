# AGENTS.md — guide for agents and contributors

## What FRIDAY is

**Goal:** a standalone assistant for **Windows and Linux** with deep
system integration. A shared Rust core plus a thin host per platform;
installable on an existing machine. FRIDAY OS (the Arch ISO) is the
showcase and reference Linux setup, not the only way in.

**Today:**

- Core: Rust workspace (`core`, `planner`, `services/*`).
  `friday-linux-daemon` (`services/mobile`) runs goals through an LLM
  provider and publishes `~/.local/state/friday/state.json`. Default
  provider: OpenCode Zen's free models (no key); a user-configured key
  (`friday setup-ai`) takes precedence.
- Windows: `windows-host/` (C#: UI Automation, COM, Win32, PowerShell),
  `plugins/windows`, Windows sensors behind `cfg(windows)`.
- Linux: Hyprland host and sensors, the `friday` CLI
  (`packaging/linux/friday`: `ask`, `goal`, `doctor`,
  `setup-ai`, `answers`, `shell`), Super+Y / Super+Alt+Y, and
  FRIDAY Shell (`packaging/linux/shell`): FRIDAY's own Quickshell desktop
  (bar, drawers, notifications, OSD), shipped in `friday-git`.
- FRIDAY OS: installer in `iso/friday/airootfs/root/`. Default desktop is
  FRIDAY Shell on Omarchy's pinned Hyprland config; Omarchy's shell keeps
  lock/idle/polkit/menus with its bar, notifications and OSD switched off
  (`friday-os/plugins/friday.bar`). end-4's Dots-Hyprland is a vendored,
  non-default option: don't copy its look or code into FRIDAY Shell.

**Core use case (comes before everything else):** press a key, ask about
what's on screen or selected, get a useful answer or action, reliably, on
both platforms.

**System integration, not kernel code.** Go deep through the sanctioned
interfaces: UI Automation / ETW / Win32 / services on Windows; AT-SPI /
Wayland portals / compositor IPC / eBPF / D-Bus / systemd on Linux. No
kernel drivers or modules (signing, crashes, kernel churn, user trust)
unless a concrete need can't be met any other way.

## Current plan

Work top to bottom. Don't start a later phase while an earlier one has known
failures.

1. **First hour works.** Linux: fresh clone → `build-iso.sh` → VM install →
   first login → Super+Y answers (VM checklist in
   `docs/HANDOFF-2026-11-04.md`). Windows: install on a clean machine →
   hotkey → answer. Fix everything found.
2. **Automate it.** Every bug found by hand becomes a test: Linux desktop in
   `iso/friday/test/` (CI `desktop` job, done); Windows host and hotkey flow
   on the `windows-latest` runner.
3. **Make the core use case excellent** on both platforms: selection/screen
   context, answer quality, reliable actions, latency, clear failures.
4. **Ship as standalone packages.** One core, per-platform packaging: an
   Arch package (daemon, CLI, desktop integration) and a Windows installer
   (service + tray app + hotkey, MSI/winget). Same CLI and `state.json`
   contract everywhere.
5. **Deeper integration** through the interfaces above (e.g. eBPF/ETW for
   system awareness), each behind explicit user consent.
6. **Only then** pull items from `docs/roadmap/ecosystem-backlog.md`.

## Rules

- **Verify before saying done.** Rust: `cargo test -p <crate>`. Desktop/ISO:
  `bash iso/friday/test/run-checks.sh`, and the VM for anything the checks
  can't see. Say what wasn't verified.
- **One change per commit**, conventional messages (`feat(...)`, `fix(...)`).
- **Contracts stay narrow.** Subsystems talk through the event bus / IPC and
  `state.json` (`specs/`). The daemon acts only on an explicit request;
  screen capture needs per-use consent. Never print or log secrets.
- **ISO changes** go under `iso/friday/airootfs/`; new packages go in
  `packages.x86_64` (live ISO) and/or the installer's pacstrap list, and must
  exist in the Arch repos (the checks verify this).
- **Upstreams are pinned, not followed.** Omarchy is fetched at the commit in
  `iso/friday/fetch-omarchy.sh`; vendored dots/themes are copied as files. Bump
  a pin on purpose, in its own commit.
- **FRIDAY ships as the `friday-git` package** (`packaging/`), on FRIDAY OS
  too: its files never go in `airootfs/` (pacman would conflict).
- **Don't commit build output** (ISOs, `target/`, `iso/out/`, packages and
  the local repo in `airootfs/opt/friday/repo/`).

## Commands

| What | Command |
|---|---|
| Build the ISO | `CLEAN_WORK=true bash iso/friday/build-iso.sh` (Arch host with `archiso`, `base-devel`, `rust`; run as a normal user: it builds the `friday-git` package, then sudo mkarchiso) |
| Arch package | `cd packaging/arch && makepkg -si` (builds committed HEAD); smoke test: `bash packaging/arch/check-package.sh <pkg>` |
| Desktop/installer checks | `bash iso/friday/test/run-checks.sh` (Arch; see script header for deps) |
| Test VM | `bash iso/friday/test/run-qemu.sh --headless --daemon --reset-disk` (falls back to TCG without KVM) |
| Drive the VM | `python3 iso/friday/test/vm-control.py status \| exec "<cmd>" \| screenshot --output x.png` |
| Health on a running system | `friday doctor` |

Gotcha: `vm-control.py wait-ready` reports SSH up before the guest is; poll a
real `ssh … uptime`.
