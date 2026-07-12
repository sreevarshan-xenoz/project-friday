# Windows Qualification — Sprint 001

**Status:** 🟡 Under Qualification (Sprint 001)
**Owner:** Reliability Engineering
**Crate under test:** `windows-host` (C# service behind the `windows` bridge)
**Rule:** Evidence-based. No feature expansion, no new architecture. We *prove* the
existing Windows subsystem until its dashboard turns 🟡 → 🟢.

This spec is the qualification plan for a subsystem that already exists. It does
not change core contracts, the event vocabulary, or the Recovery Graph. It adds a
**corpus**, a **failure-injection catalog**, a **Desktop State Model** used by the
qualification harness (and later by recovery), and **Windows-specific metrics**.

## Why Windows first

Windows is the **keystone** subsystem: Browser, Vision, Voice, and Skills all funnel
through it. If Windows is not dependable, every downstream capability inherits its
failures and debugging becomes ambiguous (which layer failed?). Ordering:

```
Windows → Terminal → Filesystem → Git → Inference → Browser → Skills → Vision/Voice
```

Browser is deliberately **last** — its failures are `Browser + Windows`, and we
refuse to qualify it before Windows is excellent.

## Scope — real surface (from `windows-host/Manifest.json`)

Implemented intents: `windows.open`, `windows.close`, `windows.focus`,
`windows.move`, `windows.resize`, `windows.list`, `windows.foreground`,
`windows.click`, `windows.type`, `windows.scroll`, `windows.capture`,
`windows.clipboard.read`, `windows.clipboard.write`.

Handlers live in `windows-host/Actions/` (`WindowHandlers.cs`, `InputHandlers.cs`,
`ClipboardHandlers.cs`), discovery in `UIAutomation/ElementFinder.cs` +
`Win32/WindowRegistry.cs`, execution in `PowerShell/Runner.cs`. Rust never sees a
raw HWND — `WindowRegistry` maps a stable `win-N` id ↔ live handle.

**Gap already visible:** `minimize` / `restore` are **not** in the manifest or
handlers, so the window-lifecycle corpus items below are partially uncovered until
those intents land (a qualification finding, not a redesign).

## Windows Qualification Corpus

### Window lifecycle
`Launch → Wait Ready → Focus → Resize → Move → Minimize → Restore → Close`
Mapped to: `OpenApplicationHandler` (polls `MainWindowHandle`, 20×100 ms, falls back
to enum-by-PID), `FocusWindowHandler` (`ShowWindow` + `SetForegroundWindow`),
`MoveWindowHandler`, `ResizeWindowHandler`, `CloseApplicationHandler`. **Minimize /
Restore** currently have no intent — corpus item, not yet implemented.

### Input
`Keyboard → Mouse → Clipboard → Drag → Scroll → Shortcut Keys`
Mapped to: `InputHandlers.cs` (`click`, `type`, `scroll`), `ClipboardHandlers.cs`
(`clipboard.read/write`). Drag / shortcut-keys are corpus items to confirm.

### Window discovery
`Find by title → Find by PID → Find by executable → Find active window → Find child controls`
Mapped to: `ListWindowsHandler` + `ForegroundWindowHandler` (title/active),
`WindowRegistry.ProcessName` (PID/executable), `UIAutomation.ElementFinder`
(child controls).

## Failure injection catalog (the real value)

Injected against the corpus; each must be detected and (where a recovery exists)
resolved or escalated honestly:

| # | Injection                      | Where it bites in `windows-host`                                          |
|---|--------------------------------|---------------------------------------------------------------------------|
| 1 | Window disappears              | External close invalidates `WindowRegistry` id→hwnd; no staleness check.  |
| 2 | Process crashes                | `OpenApplicationHandler` returns a window that then dies.                 |
| 3 | Focus stolen                   | Another app calls `SetForegroundWindow`; our call silently loses.         |
| 4 | Window minimized unexpectedly   | State drifts from `Normal`; no intent to observe/restore.                 |
| 5 | Multiple windows, same title   | `list` returns several; focus-by-id is correct, but discovery must disambiguate. |
| 6 | Elevated window               | `SetForegroundWindow` fails across integrity levels / UAC.                |
| 7 | Hung application              | No timeout on `Wait Ready`; handler blocks.                               |
| 8 | UAC dialog                    | Blocks input; must detect and escalate, not retry blindly.               |
| 9 | Slow startup                  | 20×100 ms poll may time out before UI is ready.                          |
|10 | Explorer restart              | All enumerated HWNDs change; registered ids point at dead handles.       |

These are the bugs users actually hit. Findings feed `git` issues, never silence.

## Desktop State Model

A model the harness (and later recovery) uses to *understand* desktop state instead
of issuing blind commands. Fields:

```
Active Window      win-N | none
Open Windows       [win-N, ...]
Focused Process    name | pid
Clipboard          text | image | empty
Displays           count, geometry
Virtual Desktop    index
Foreground Lock    bool        (another process owns foreground)
Input Blocked      bool        (UAC / modal dialog)
Elevation          bool        (current op requires admin)
```

This is a *qualification/recovery aid*, not a core abstraction — it does not change
the `windows` bridge contract. It makes failure injection observable and recovery
decidable.

## Windows-specific qualification metrics

Distinct from the global four; precise so they cannot be reinterpreted:

- **Launch Success** = launched-and-ready / launch attempts.
- **Focus Accuracy** = focus landed on intended window / focus attempts.
- **Input Accuracy** = input delivered to intended target / input attempts.
- **Window Detection** = correct window found by discovery query / queries.
- **Clipboard Reliability** = read-back matches written / clipboard ops.
- **Recovery Success** = injected failures resolved or safely escalated / injections.
- **Recovery Cost** = mean `cost_ms()` of resolved recoveries (ms).
- **Mean Interaction Time** = mean ms from intent issued to observed effect.

## Qualification bar (🟡 → 🟢)

The Windows Qualification progress is a composite of the eight metrics above,
normalized to `[0,1]`, each required to clear its individual bar (Launch/Focus/
Input/Detection/Clipboard ≥ 0.99, Recovery Success ≥ 0.85, Interaction Time within
SLA). Overall progress = mean of the eight normalized scores; **🟢 Qualified** when
overall ≥ 0.95 **and** every individual bar is met, sustained over the trailing
window.

The Resilience Dashboard shows the current sprint on one line:

```
Current Qualification Sprint: Windows  ████████░░ 78%
```

(Proxy baseline today: Windows recovery coverage from the simulated benchmark, to
be replaced by the composite above once the corpus is instrumented.)

## Trust criterion (definition of done)

When Windows is 🟢, you should genuinely trust FRIDAY to do, unsupervised:

> "Open VS Code, switch to the project, run `cargo test`, and bring the terminal to the front."

## Acceptance criteria (Sprint 001 exit → 🟢)

- [ ] Windows Qualification Corpus implemented and runnable on this Win32 host.
- [ ] All 10 failure injections exercise the corpus and produce an issue (none ignored).
- [ ] Desktop State Model implemented; failures become observable via it.
- [ ] The eight Windows metrics computed and reported; dashboard shows the sprint line.
- [ ] Overall Windows Qualification ≥ 0.95 with every individual bar met, sustained.
- [ ] At least the `minimize`/`restore` gap and the stale-HWND (explorer restart)
      finding have issues filed; fixes land or are explicitly deferred with reason.

## Relationship to other specs

- Does **not** modify `recovery.md` (Recovery Graph is frozen). New Windows
  recovery, if needed, follows the state-level path in `recovery_policy.md`.
- Follows `benchmark.md` (honest measurement; benchmarks describe reality).
- Progress is tracked per `engineering/product_principles.md` (trust, not intelligence).
