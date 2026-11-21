# After v0.8.0: the second brainstorm queue

On 2026-11-22, after the twenty items of `after-0.7.3.md` had their first
pass, a second round of outside opinions brainstormed again: ChatGPT (inside
the project and once outside it, so it would think differently), Gemini,
Claude Fable, Sonnet and Haiku, and the five OpenCode free models that
weren't rate-limited (big-pickle, hy4-flash, iceman, flint-40b,
nemotron-lightning). Their ideas were merged and the code gaps they named
checked against `dev`. The owner asked for this list to be worked on
autopilot. Item 11 is the owner's own.

Same rules as before: one item at a time, each with its own branch, tests
and a fresh review; hardware-pass failures jump the queue. Bugs and privacy
holes go first, then the core hotkey's speed and reliability, then new
things. "Votes" is how many of the eleven suggested it.

| # | Item | Votes | What done looks like |
|---|---|---|---|
| 1 | **The daemon survives a client leaving** | 1 | `friday-linux-daemon.rs` stops its command loop when a result is sent with no client connected (`broadcast::send` fails with zero receivers), so after a cancelled `--wait` every later ask goes unanswered while `doctor` says healthy. Results to nobody are dropped instead; an e2e test drops the socket mid-goal and then asks again. |
| 2 | **Screen text from the focused window only** | 3 | Screen text snapshots the whole app (`agent_desktop.rs` `window_text`), so a second window, even a private one, can go with the ask. It reads the focused window, and refuses (no screenshot either) when that window is private. |
| 3 | **Private windows are checked by the daemon** | 3 | The private-window check lives in the capture gate (`context_read.rs`), not only in the shell, so `friday-ctl context screen` and every host get it; it works on niri as well as Hyprland; secrets in window titles are masked. |
| 4 | **Text read from the screen can't steer FRIDAY** | 3 | Every injected block (follow-up, recall, workspace, folder search, page, window text) uses one marker that `goal_segment` stops at, so a page that says "search the web for…" can't pick tools; the prompt tells the model that block is untrusted data. |
| 5 | **Answers don't keep what was asked about** | 1 | `last-answer.md`/`answers.md` stop storing the raw selection; an answer made locally (private window, private mode) is marked, and its follow-up stays local too. |
| 6 | **URLs and paths aren't "a password"** | 1 | The token guard (`friday` selection and paste-as, `Asking.cs`) stops refusing URLs, paths and dotted names, with one table of cases tested on both platforms. |
| 7 | **Screen asks fail over, and say what they sent** | 2 | A busy provider on the text route falls back to the screenshot (consent already given) instead of ending the ask; the answer says text or screenshot, and "first N characters only" when the window or selection was cut; the ask-box hints stop promising a screenshot. |
| 8 | **The hotkey is fast, and CI proves it** | 4 | The ask box opens before the project lookup and pill probe (today up to 5 s of timeouts first); `tests/e2e` gets a hermetic screen ask (text route, screenshot fallback, refused consent) and keypress-to-box and keypress-to-first-words budgets. |
| 9 | **Windows screen asks send text first** | 2 | Win+Ctrl+Y sends the window's UI Automation text when there's enough of it, through the same masking and egress check, and a screenshot only when there isn't. |
| 10 | **Windows: "That was wrong", and selection without Ctrl+C** | 2 | The answer window gets "That was wrong" (`friday-ctl wrong`); Win+Alt+Y reads the selection through UIA before falling back to Ctrl+C, so a terminal inside VS Code isn't interrupted. |
| 11 | **FRIDAY upgrades itself, and the system if you let it** | owner | FRIDAY updates itself on both platforms; on FRIDAY OS a switch (off by default) upgrades the whole system too: a snapshot first (Btrfs/snapper where available, otherwise a package-cache rollback list), backs up FRIDAY's own state, upgrades without restarting what's running (services restart at the next login or idle), checks that FRIDAY and the desktop still start, and reverts on its own when they don't. |
| 12 | **The Windows update is checked before it runs** | 1 | The MSI is signed in `release.yml` and the tray checks the signature (not only a checksum from the same release) before msiexec, from a fresh folder. |
| 13 | **"This" means what was in front when you pressed the key** | 2 | The context (window, selection, focused element) is frozen at the keypress, so switching windows while FRIDAY thinks can't change what it acts on; screen asks join the 15-minute conversation, so "how do I fix it?" knows the error just asked about. |
| 14 | **Actions prove they worked** | 2 | An action reports done only after checking the result (the file exists, the window moved, the setting changed), and says "partly done" or "failed" otherwise; no false "Done". |
| 15 | **FRIDAY knows what it can't do right now** | 2 | When the network, a provider, the extension or screen text is down, the answer says which and what still works, from one live check shared with `friday doctor`. |
| 16 | **Instant answers without a model** | 1 | Arithmetic, unit and time-zone conversions, and colour codes answer locally at once, before any provider is asked. |
| 17 | **Point and ask** | 2 | Pick a region or an element on screen and ask about just that; text first, a cropped screenshot only when needed. |
| 18 | **Per-app instructions** | 1 | "In my terminal, answer with commands only": instructions kept per app, shown in "what FRIDAY used". |
| 19 | **Easy and private asks go local** | 2 | With a local model set up, short asks and anything from a private window go to it first; harder ones go to the cloud, and the answer says which. |
| 20 | **Recall that behaves** | 3 | Recall pauses while the screen is locked or idle, matches private titles by whole words (no more "Embankment"), keeps the full 7 days it promises (segments instead of one 20 MB file rewritten every 30 s), and reads the focused window. |
| 21 | **One project identity everywhere** | 3 | `thoughts::named` still matches any word of a folder's name, so "FRIDAY" matches both `project-friday` and `friday-mobile`; the project picked once for the ask is the only one thoughts and the timeline use. |
| 22 | **Long use stays fast** | 1 | `answers.md` rotates and the timeline reads its tail; the undo journal gets a lock (the shell and the daemon both rewrite it today). |
| 23 | **The partial items, finished** | 6 | From `after-0.7.3.md`: undo for launches, file writes and agent runs (and on Windows), Windows Terminal help, the "2:30" strip, local OCR for apps without accessibility text, Windows workspaces/thoughts/timeline. |

Also: `vnext.md`'s "Still open" list is out of date (lessons are scoped,
browser redirects are intercepted, the resume grammar is shared); it is
refreshed in the same PR as this list.

## Ideas kept for later

Task capsules and a workflow compiler (overlap with workspaces, threads and
teach-a-routine), cross-app handoff, a live "notify me when…" watcher (it
would be ambient capture), smart form fill, an attention budget for
notifications, a semantic search across recall, meetings and threads.
