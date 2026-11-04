# AGENTS.md — guide for agents and contributors

## What FRIDAY is (today)

An Arch Linux + Hyprland desktop with a built-in assistant:

- `friday-linux-daemon` (Rust, `services/mobile`) runs goals through an LLM
  provider and publishes its state to `~/.local/state/friday/state.json`.
- `friday` CLI (`iso/friday/airootfs/usr/bin/friday`): `ask`, `goal`,
  `doctor`, `setup-ai`, `answers`; Omarchy commands pass through.
- Desktop: Dots-Hyprland "ii" Quickshell shell (default) or Omarchy themes,
  with a FRIDAY bar indicator and Super+Y / Super+Alt+Y.
- Installer: `iso/friday/airootfs/root/{configurator,installer}`.

The core use case, which comes before anything else: **press a key, ask about
what's on screen or selected, get a useful answer or action, reliably.**

## Current plan

Work top to bottom. Don't start a later phase while an earlier one has known
failures.

1. **First hour works.** Fresh clone → `build-iso.sh` → install in a VM →
   first login → Super+Y answers. Run the VM checklist in
   `docs/HANDOFF-2026-11-04.md` and fix everything it finds.
2. **Automate it.** Every check that found a bug by hand becomes a test in
   `iso/friday/test/` and runs in CI (the `desktop` job).
3. **Make the core use case excellent.** Selection/screen context, answer
   quality, the actions FRIDAY can take reliably, latency, clear failures.
4. **Package FRIDAY separately.** Daemon + CLI + bar widget + keybinds as an
   Arch package usable on any Arch/Hyprland machine; the ISO becomes the
   showcase, not the only way in.
5. **Only then** pull items from `docs/roadmap/ecosystem-backlog.md`.

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
- **Don't commit build output** (ISOs, `target/`, `iso/out/`).

## Commands

| What | Command |
|---|---|
| Build the ISO | `CLEAN_WORK=true bash iso/friday/build-iso.sh` (Arch host with `archiso`) |
| Desktop/installer checks | `bash iso/friday/test/run-checks.sh` (Arch; see script header for deps) |
| Test VM | `bash iso/friday/test/run-qemu.sh --headless --daemon --reset-disk` (falls back to TCG without KVM) |
| Drive the VM | `python3 iso/friday/test/vm-control.py status \| exec "<cmd>" \| screenshot --output x.png` |
| Health on a running system | `friday doctor` |

Gotcha: `vm-control.py wait-ready` reports SSH up before the guest is; poll a
real `ssh … uptime`.
