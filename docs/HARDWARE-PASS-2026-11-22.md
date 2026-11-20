# Real-hardware pass (2026-11-22)

What to check on a real PC, in order. Each line says what to do and what
should happen. Mark `[x]` when it did, or write what happened instead; a
failure here becomes a test (AGENTS.md, phase 2).

Use the newest release (`gh release list`; the ISO and the MSI are on the
release page). `friday doctor` first on Linux; it should be all green.

## FRIDAY OS install (Linux, GPU PC)

- [ ] Boot the ISO, install with the defaults. The installer finds the GPU and
      offers local AI with a preset that fits its VRAM ("text only" when it
      doesn't fit a model).
- [ ] First login: FRIDAY Shell comes up, the orb says ready.
- [ ] `friday local-ai detect` and `friday-ctl local-ai detect` print the same
      GPU, VRAM and RAM (the bash and the Rust copy of the same sizing);
      `friday local-ai status` shows what's installed.
- [ ] Volume, brightness, battery, Bluetooth and Wi-Fi in the shell work.
- [ ] Animations feel smooth (the VM was too slow to judge).

## The core use case

- [ ] Super+Y, ask "what's the capital of Australia": an answer in a few
      seconds, streaming as it comes.
- [ ] Select text in a browser, Super+Alt+Y "explain this": the answer is
      about the selection.
- [ ] Super+Ctrl+Y "what's on my screen": it asks first, then answers about
      the window in front.
- [ ] Settings → providers shows who answered last.

## New since v0.6.0

- [ ] **Open a page**: Super+Y "open archlinux.org": the page opens in your
      own browser (not hidden).
- [ ] **Read a page**: Super+Y "summarize https://archlinux.org/news/": the
      answer is about that page and ends with it as the source. In
      `friday sent` (the egress log) the read shows as `page`.
- [ ] **Private mode** (`friday private on`), same question: it says it
      didn't read the page. `friday private off` after.
- [ ] **Lessons**: ask something that fails (e.g. "restart the service
      nosuch.service"), then ask it again: the second answer mentions the
      earlier failure. `friday forget memory` removes `lessons.jsonl`.

## vNext N1: your work, remembered (new tonight)

- [ ] **Workspaces**: open a terminal in a git repo with `nvim` (or `hx`) on
      a file, a browser on another workspace, one floating window. Super+Y
      "save this as my friday workspace". Close everything. Super+Y "restore
      my friday workspace": the windows come back on their workspaces, the
      terminal in the repo with the editor on the file, the floating one in
      its place; the answer lists what came back and what didn't. Run it
      again: "already in place", no duplicates.
- [ ] **Browser tabs**: with Chromium on a workspace and a few tabs open,
      save a workspace, close Chromium, restore: it comes back on its
      workspace with the same tabs, none twice.
- [ ] **Project facts**: in a git repo with an uncommitted change, Super+Y
      "what's left on <project>": the answer names the branch, the
      uncommitted file and your last commit (from git, not guessed).
- [ ] **Continue building**: after a reboot, Super+Y "Friday, let's continue
      building FRIDAY": the workspace comes back, and the answer says where
      you stopped (from your earlier asks and actions) and your open
      thoughts. Time it (the goal is under five seconds to the windows).
- [ ] **Timeline**: `friday-ctl timeline yesterday` lists what you asked and
      did; Super+Y "what did I do yesterday?" answers from it.
- [ ] **Thoughts**: in a terminal in the repo, Super+Y "keep this idea:
      workspaces should remember tab groups"; "we decided to ship workspaces
      first"; "open question: sync threads between computers?". Then
      `friday-ctl thought list friday` shows all three under the project,
      with your capitals, and "what was I thinking about FRIDAY?" quotes them
      with dates.
- [ ] **Autosave** (opt-in): `friday workspace autosave on`; after ten
      minutes `friday workspace list` shows "last session"; "restore my last
      session" brings it back. `friday workspace autosave off`.
- [ ] **Windows local AI**: tray Settings → Local AI shows the GPU, VRAM and
      RAM and a recommended model; "Set up local AI" installs Ollama (winget),
      downloads it with progress, and Win+Y then answers with it on.

## Owed from items 1-14 (real session only)

- [ ] kitty: run a failing command, Super+Y: it offers to explain the error.
      "Tell me when this finishes" on a `sleep 20`: a notification at the end.
- [ ] `friday recall on`: the bar shows the recall mark; after a minute on a
      page, "what was that about …" finds it. `friday recall off` clears it.
- [ ] `friday wake on`: the bar shows the mic mark; say "Hey Friday, what
      time is it in Tokyo?": an answer. `friday wake off`.
- [ ] `friday captions on` with a video playing: captions appear;
      `friday captions on --mic` with your voice; notes saved (mode 600).
- [ ] A private window (password manager, bank site, private browsing):
      Super+Y uses the local model or refuses; no screenshot leaves.
- [ ] With local AI installed: private-window asks and recall are answered by
      the local model.
- [ ] Rewrite in place: select text in a text field, Super+Alt+Y, pick
      "Rewrite: make it shorter": the selection is replaced. Paste as…
      (Super+Alt+V). `friday undo` (or Ctrl+Z) puts the text back.
- [ ] Show me: "where's the setting for dark mode" in a settings app draws a
      ring on the control.
- [ ] Routines: do two steps, "save that as my morning", then "run my
      morning".
- [ ] niri: log into the niri session and repeat Super+Y and the selection
      ask.

## Windows (real machine)

- [ ] Install the MSI from the release; Win+Y answers; the tray starts the
      daemon.
- [ ] Win+Alt+Y on selected text, Win+Ctrl+Y on the screen.
- [ ] "open example.com" opens it in your default browser; "summarize
      https://example.com" reads it.
- [ ] `friday-ctl local-ai detect` (from the install folder) names the real
      GPU and its VRAM (the Windows branch was never run on a GPU).
- [ ] Hotkeys and streaming under normal load (a browser and an editor open).

## Report back

For each failure: what you did, what happened, and the log
(`~/.local/state/friday/` or `%LOCALAPPDATA%\friday\logs`, with personal data
removed). I turn each into a test.
