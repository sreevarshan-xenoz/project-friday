# R5-2-5-1 — Minimal OS Context Collector (IMPLEMENTATION FREEZE)

**Status:** FROZEN implementation of the R5-2-5-1 context-collector slice.
**Inputs:** frozen surveys `R5-2-5-0-OS-CONTEXT-INVOCATION-SURVEY.md` and `R5-2-5-1-0-COLLECTOR-OWNERSHIP-INTERFACE-SURVEY.md`; frozen `R5-2-4-CONTEXT-GATE-SPEC.md`; `dev @ cfeb4c1`.
**This freeze:** creates the dedicated `services/context-collector` crate (`friday-context-collector`) — a pure OS capability adapter. It captures per-channel context behind a testable `CommandRunner` boundary and has **no consent / provider / egress knowledge** (per frozen ownership decision).

---

## What was built

New crate `services/context-collector` (added to workspace members):

| File | Contract |
|---|---|
| `src/command.rs` | `CommandRunner` trait (`run(program, args) -> Result<CommandOutput, RunnerError>`). `RunnerError` = `Io(String) \| Timeout(String)`. `CommandOutput { stdout, stderr, status }`. `RealCommandRunner` (spawns OS processes, optional time budget via a detached thread) and `FakeCommandRunner` (deterministic scripted test double that fails **loudly** on unscripted argv so a mis-wired recipe is caught by tests). |
| `src/collector.rs` | `ContextCollector<R: CommandRunner>` with `capture(channel, &CaptureOptions) -> Result<CapturedContext, CaptureError>`. Reuses `friday_intelligence::ContextChannel` (type-only dep, per R5-2-4). |
| `src/lib.rs` | Crate root re-exporting `ContextChannel`, `ContextCollector`, `CaptureOptions`, `CapturedContext`, `CaptureError`, `CommandRunner`, and friends. No gate/provider/egress imports. |

**V1 channels (real command recipes, qualified from Omarchy substrate):**

| Channel | Recipe | Payload |
|---|---|---|
| `Screen` | `grim -g <region> -` (region) or `grim -` (fullscreen) | `CapturedContext::Image { bytes, mime, geometry }` |
| `Selection` | `omarchy-capture-text --stdout <region>` | `CapturedContext::Text { text, OcrRegion }` |
| `App` | `hyprctl -j activewindow` (JSON parsed) | `CapturedContext::AppInfo { class, title, geometry, focused }` |
| `Clipboard` | `wl-paste --no-newline` | `CapturedContext::Text { text, Clipboard }` |
| `Files`, `Mic` | — | `CaptureError::Unavailable` (deferred to a later slice) |

**Key semantics:**

- **Pure capability, no privacy decision.** `capture()` returns only OS payloads (`Image` / `Text` / `AppInfo`); there is no allow/consent/provider variant in the output vocabulary. Consent lives exclusively in `ContextGate` (R5-2-4), applied downstream by the daemon (R5-2-5-2) — never here.
- **Closed error contract.** `CaptureError` = `Command { channel, message }` \| `Unavailable { channel, reason }` \| `Timeout { channel, message }` \| `InvalidOptions { channel, message }`. `Command` absorbs nonzero exits and invalid `activewindow` JSON; `Timeout` surfaces a stale thread's missing result; `Unavailable` marks `Files`/`Mic`.
- **Determinism on Windows CI.** All logic tests run through `FakeCommandRunner`; `RealCommandRunner` only does OS I/O with no Linux-exclusive code in tests, keeping the suite green on the frozen Windows environment.

---

## Frozen invariants — pinned by tests (12 tests in `collector.rs` + 3 in `command.rs`)

1. **Every V1 channel has one unambiguous recipe; a wrong/missing recipe fails loudly.** `unexpected_command_is_loud_failure`, `fake_rejects_unscripted_calls_loudly`.
2. **Channel payloads are correct at the boundary.** `screen_region_captures_png_with_geometry`, `screen_fullscreen_needs_no_region`, `selection_ocrs_region_into_text`, `app_parses_hyprctl_activewindow_json`, `clipboard_reads_text`.
3. **Invariants enforced by types-independent options validation.** `screen_without_options_is_invalid`, `selection_without_region_is_invalid`.
4. **`Files`/`Mic` are honestly `Unavailable` in V1; nothing fabricates a capture.** `files_and_mic_are_unavailable_in_v1`.
5. **A failed/fabricated capture is reported, never silently downgraded.** `nonzero_command_exit_is_reported_not_fabricated`, `app_invalid_json_is_a_capture_error`.
6. **The collector's output vocabulary is closed — no privacy/consent/eAccess variant can silently creep in.** `captured_context_has_no_privacy_verdict` (structural).

Plus `command.rs`: `fake_returns_scripted_output_for_exact_argv`, `fake_can_assert_a_command_was_scripted`.

---

## Verification

- `cargo test -p friday-context-collector` → **15 tests** (12 collector + 3 command) pass.
- `cargo clippy -p friday-context-collector --all-targets -- -D warnings` → clean.
- `cargo fmt --check` → clean.
- Full workspace `cargo build` → clean.
- Frozen ownership honored: collector is a **dedicated crate in `services/`** (not folded into the daemon), a pure capability with no consent/provider/egress imports beyond the `ContextChannel` type re-use from `friday-intelligence`.
- Frozen interface honored: `ContextChannel → CapturedContext` via `CommandRunner`; no privacy semantics; `Files`/`Mic` → `Unavailable`.

> Note: the workspace-wide `cargo clippy --all-targets` currently reports 3 **pre-existing** doc-list-item warnings in `services/mobile/src/shell_state.rs` (unrelated, present on `dev` before this change). The collector crate itself is clippy-clean.

---

## Scope guard

This freeze touches **only**:
- `services/context-collector/Cargo.toml` (new crate: `friday-intelligence` type-dep + `serde_json`)
- `services/context-collector/src/lib.rs` (new)
- `services/context-collector/src/command.rs` (new)
- `services/context-collector/src/collector.rs` (new)
- root `Cargo.toml` (add `services/context-collector` to members)
- `Cargo.lock` (lockfile)

It deliberately does **not** touch the daemon orchestrator or shell chooser (those are subsequent slices), does not add any consent/UI/provider code, and does not create an OS UI.

Next stone: **R5-2-5-2** (daemon integration — call `ContextCollector` → `ContextGate` → `EgressGate` → provider) then **R5-2-5-3** (shell context chooser).
