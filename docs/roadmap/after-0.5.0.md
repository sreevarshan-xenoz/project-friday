# After v0.5.0: what comes next, in order

Chosen by the owner on 2026-11-20. One item at a time, each on its own branch
and PR, each with tests and a live check (VM for Linux). An item marked M or L
gets a short design the owner sees before it is built; S items are built
directly. Every item has to pass the rule in `AGENTS.md` ("A subsystem earns
its place"), and new CLI logic goes in `friday-ctl` (Rust), not the bash
script.

Size: S = about a day, M = a few days, L = a week or more.

Status (2026-11-20): items 1 to 14 are built, each in the PR shown beside it
(14 may still be in review). What is left: 15 to 17, and 18 needs a real PC.
Items 1, 2, 3, 4, 6, 7 and 8 went into the bash script, not `friday-ctl`; item
16 moves them over. Items 13 and 14 are smaller than the table says (see the
notes below).

## 1. Daily use

| # | Item | What a person sees | Size |
|---|---|---|---|
| 1 | Explain the last error (#233) | In a terminal, Super+Y picks up the failed command and its output by itself | S |
| 2 | Rewrite in place (#234) | Select text anywhere, press a key, pick "fix grammar / shorter / formal / translate"; the selection is replaced | M |
| 3 | Paste as… (#235) | A paste hotkey that transforms the clipboard first (bullets, JSON, translated, cleaned up); shares item 2's code | S |
| 4 | App-aware chips (#236) | The answer pill offers actions that fit the app (browser: summarise page; editor: explain selection) | S |

## 2. Trust and clarity

| # | Item | What a person sees | Size |
|---|---|---|---|
| 5 | Free-tier status (#237) | Settings shows each provider's state ("Zen: limit hit, Kilo: OK") and who answered last | S |
| 6 | Measure, don't guess (#238) | First run times the local model; Auto uses the measured speed, not only "above 4B" | S |
| 7 | Privacy by window (#239) | In a password manager, a banking site or a private browser window, FRIDAY uses the local model or refuses; no screenshot leaves | M |
| 8 | Undo (#240) | `friday undo` reverses what FRIDAY last did on the desktop | M |

## 3. Bigger features

| # | Item | What a person sees | Size |
|---|---|---|---|
| 9 | Offline knowledge pack (#241) | A small local model answers system questions from the Arch wiki, man pages and tldr, indexed on the computer | M |
| 10 | Watch this (#242) | Drag a region or name a command; FRIDAY says when it changes (build done, download finished) | M |
| 11 | Show me (#244) | "Where's the setting for X?" draws a ring on the real button | L |
| 12 | "What was that?" (#246) | Opt-in rolling memory of on-screen text for the last few hours, local only, no screenshots | L |
| 13 | Teach by showing (#247) | Do a task once; FRIDAY saves it as a skill to replay by name | L |
| 14 | Wake word and live captions (#248) | A local "Friday" wake word; live captions and notes for any audio | L |

## 4. Platform and project

| # | Item | What it is | Size |
|---|---|---|---|
| 15 | Windows parity | Local AI and frosted glass on Windows | M |
| 16 | Bash to Rust | Port `friday local-ai` and `setup-ai` into `friday-ctl`; the bash script keeps only thin wrappers | M |
| 17 | Demo | A 60-second recording for the README | S |
| 18 | Real-hardware pass | Install on a real PC with a GPU (needs the owner's machine) | owner |

## Why this order

- 1 to 4 are small and used every day, and 2 builds what 3 needs.
- 5 to 8 make FRIDAY easier to trust before it does more.
- 9 goes first of the bigger ones: it makes the local AI from v0.5.0 worth having.
- 11 to 14 are the largest and the least certain, so they come last.
- 16 can move earlier if the bash script causes another bug.

## Notes on what was built

- **13 Teach by showing** saves what *FRIDAY* did (its own `actions.jsonl`),
  not what the person did by hand: the accessibility bus reports focus and text
  changes, not clicks. Typed text and destructive steps are never saved.
- **14 Wake word and live captions**: captions come from the sound the computer
  plays (or the microphone with `--mic`), a few seconds behind. The wake word
  is only "hey/ok Friday", opt-in, and runs through the same local whisper.cpp
  model as `friday voice`.
- **11 Show me** works with GTK and Qt apps. Chromium shows only its window
  frame on the accessibility bus here, so web pages fall back to a worded answer.

## Owed live checks

Everything above passed its unit and CLI checks and a run in the FRIDAY OS test
VM where noted in its PR. These still need a real session (the VM pass, then the
real-hardware pass in item 18):

- Keys sent through Hyprland (`send_key_state`): Ctrl+V for Rewrite in place and
  Paste as, Ctrl+Z for Undo.
- kitty on a real session: Explain the last error and "Tell me when this finishes".
- The `friday-recall` and `friday-wake` user services under systemd, and the bar
  marks they show.
- Private windows and recall answered by a real local model.
- Captions and the wake word with a real microphone and real speech.
- niri: none of the new features were checked there.
