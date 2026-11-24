# After v0.8.2: the 12-repo queue

Owner-approved on 2026-11-23: every idea from the 12-repo brainstorm except the "Don't build" list and the ideas the models disagreed on. Worked top to bottom on autopilot, one PR per item. Item 6 (GNOME/KDE) moved to the end on the owner's word: it is larger than first thought (the Linux ask box and screenshots need replacements there).

- Order: the top ten, then the next-in-line seven, then the rest by votes (out of 17 models), smaller first.
- Size: S about a day, M a few days, L a week or more.
- "Already planned" items are also in an older roadmap doc; doing them here closes both.
- Every new feature gets an on/off switch on the Features page.

| # | Id | Idea | Status | Size | Votes | You get |
|---|---|---|---|---|---|---|
| 1 | CTX-4 | Ask first when "this" is ambiguous | new | S | 13 | If selection, window and screen could all be meant, one short chip asks which; clear asks never prompt. |
| 2 | CTX-6 | Ephemeral one-use capture handle | partly there | S | 12 | A screen grab lives in memory for one ask chain (with its follow-ups), then is gone; "captured, not saved" shows. |
| 3 | PRV-1 | Untrusted text can't trigger actions: flag it, force a confirmation | partly there | M | 17 | Page, window and clipboard text with instructions is flagged; an action after reading it always asks. |
| 4 | PRV-16 | Private-window canary tests across every subsystem | new | S | 5 | A test plants a canary in a private window and proves it never reaches capture, extension, logs, memory, agents or providers. |
| 5 | PKG-2 | Published eval of the core ask | partly there | M | 15 | CI runs 30 fixed screen/selection asks and publishes the pass rate (fake model labelled as such). |
| 6 | CTX-2 | (moved to the end, owner 2026-11-23) | | | | |
| 7 | MEM-3 | Proposed memories you approve | new | S | 16 | After an answer a chip says "remember: you use Fish?"; nothing is stored until you accept. |
| 8 | VOI-1 | Dictation cleanup: fillers and a personal dictionary | new | S | 17 | Voice typing drops "um" and spells your names and tools correctly. |
| 9 | AGT-12 | Independent check after an agent says "done" | new | M | 9 | FRIDAY runs the project tests itself and shows red if the agent lied. |
| 10 | PRV-5 | API keys in the OS keychain | new | M | 14 | Provider keys live in libsecret / Credential Manager, with migration from the file. |
| 11 | PRV-10 | Model output can never approve an action | partly there | S | 12 | Run is accepted only from UI, key or voice; a test feeds an injected "approved: true". |
| 12 | VOI-2 | Barge-in: wake word cuts speech | partly there | S | 16 | Say "Hey Friday" over a spoken answer and it stops and listens. |
| 13 | VOI-11 | Talk-over on Windows (echo cancellation) | new | M | 4 | Interrupting spoken answers works on Windows as on Linux. |
| 14 | VOI-7 | Pause listening for meetings | new | S | 10 | One toggle, or auto when a call app holds the mic, stops the wake word; the tray shows "paused". |
| 15 | AGT-3 | A git worktree per coding-agent run | new | M | 14 | Agent edits land in a separate worktree and merge only on Accept. |
| 16 | AGT-1 | Quiet hours and budgets for standing tasks | new | S | 15 | Tasks stay silent at night and stop at a daily call/token cap. |
| 17 | MEM-4 | Hybrid keyword + local vector recall, keyword-only on low RAM | new | M | 16 | Recall finds past answers by meaning and by exact error text. |
| 18 | MEM-1 | One readable memory file you can edit | already planned | M | 17 | Everything FRIDAY remembers about you in a plain file or page: read, correct, pin, export, forget. |
| 19 | MEM-2 | Memory import from other assistants, dry run first | new | M | 17 | Bring CLAUDE.md or other notes in; a preview lists what would be added. |
| 20 | PKG-1 | Documented stable `friday ask --json` | partly there | S | 16 | Scripts get the answer as JSON with exit codes; covered by e2e. |
| 21 | ECO-1 | Paired-device list with revoke on the desktop | partly there | S | 16 | See each phone, last seen, and cut one off. |
| 22 | PRV-2 | Signed updates with rollback | already planned | M | 16 | A bad update reverts itself; releases are signed. |
| 23 | CTX-1 | One privacy filter for every capture path | partly there | S | 15 | Secret masking and the private-window list apply the same way to window text, OCR, screenshots, page text and clipboard. |
| 24 | MEM-5 | A changed fact closes the old one with a date | new | S | 15 | I moved to Pune ends the old city instead of both being true. |
| 25 | VOI-3 | Spoken morning brief and "what did I miss" | partly there | S | 15 | A keyword reads finished agents, reminders and standing-task results aloud, from local sources only. |
| 26 | LAI-1 | Usage and latency page | partly there | S | 15 | Calls, tokens, free-limit hits and seconds to first word per provider, from the local log. |
| 27 | ACT-1 | Action preview and result check | already planned | M | 15 | A risky action shows what will change; after it runs FRIDAY verifies and says done, partly or failed. |
| 28 | AGT-2 | One list of runs, tasks and approvals | already planned | M | 15 | Running and finished agents, standing tasks and pending approvals in one view. |
| 29 | PRV-3 | Deterministic risk table, visible and tested | partly there | M | 15 | Every tool is safe / watched / ask / block in one table; Settings shows it. |
| 30 | PRV-4 | Verified backup and restore | partly there | M | 15 | Back up settings, memory and history; a restore check runs before replacing anything. |
| 31 | ECO-2 | Phone shows the same thread and can approve (already approves) | already planned | M | 15 | Pick up a desktop answer or task on the phone. |
| 32 | MEM-6 | Crash-safe memory writes | partly there | S | 14 | kill -9 during a write never loses a saved fact or corrupts history. |
| 33 | LAI-2 | Different model per job (ask, screen, voice, chores) | new | S | 14 | A cheap or local model for chores, a strong one for hard asks, shown on the card. |
| 34 | UI-1 | Reactive status orb in the pill | partly there | S | 14 | Three states (listening/thinking/speaking) shown by a small orb or face; off switch. |
| 35 | CTX-3 | Text first, then OCR, then screenshot, for apps with no accessibility text | already planned | M | 14 | Electron, canvas and remote-desktop windows still answer from OCR text, with the source named. |
| 36 | AGT-4 | Agent runs that survive a restart and deliver the result | partly there | M | 14 | A run keeps going after the daemon restarts; the result comes back to the conversation. |
| 37 | AGT-7 | FRIDAY as an MCP server | already planned | M | 14 | Other AI apps use FRIDAY's context and actions behind permissions. |
| 38 | AGT-8 | Progressive tool disclosure and tool search | new | M | 14 | Only the tool schemas that matter go into the prompt, so small models stay fast. |
| 39 | AGT-9 | Grammar-constrained tool calls for small local models | new | M | 14 | A 3B model produces valid action JSON every time. |
| 40 | LAI-3 | Battery- and RAM-aware local model choice | partly there | S | 13 | On battery or low RAM FRIDAY picks a smaller local model. |
| 41 | PRV-7 | Telemetry off by default | already planned | S | 13 | No usage data leaves unless the person turns it on. |
| 42 | PKG-3 | Hardware fit in the local AI picker | partly there | S | 13 | Each preset says what will run on your RAM and VRAM. |
| 43 | CTX-5 | Route screenshots to a vision model of your choice | partly there | M | 13 | Image asks go to a model that can see (local one in private mode), text asks to a cheaper one. |
| 44 | PRV-8 | Tamper-evident action log | partly there | M | 13 | Every action and approval in a hash-chained local log that `friday verify-log` checks. |
| 45 | PRV-9 | Sandbox for coding agents (bubblewrap / restricted token) | new | L | 13 | An agent can write only inside its worktree and cannot read ~/.ssh. |
| 46 | MEM-7 | Memory size cap with a curator | new | S | 12 | Always-injected memory stays under a token budget; overflow offers a merge you approve. |
| 47 | AGT-10 | Routines limited to reversible actions | new | S | 12 | A scheduled task that tries to delete or send is blocked and logged. |
| 48 | ECO-3 | Voice notes from the phone, transcribed on the PC | new | M | 12 | Speak an ask into the phone; local whisper answers. |
| 49 | VOI-5 | Mic-is-live bar with level meter | partly there | S | 11 | A slim always-on-top bar whenever the mic is open, with level bars. |
| 50 | AGT-11 | Task recipes (one-click presets) | new | S | 11 | Daily digest, "PR watcher", "clipboard summary" in one click. |
| 51 | PRV-11 | Approve once, for this session, or always (narrow scope, expires) | partly there | S | 11 | Stop re-approving the same safe thing; "always" is per exact action and can expire. |
| 52 | PRV-12 | Hard blocklist no setting can lift | partly there | S | 11 | rm -rf /, disk wipe and key exfiltration commands are refused in every mode. |
| 53 | PRV-13 | Offline proof | partly there | S | 11 | `friday doctor --offline` shows private mode made zero outbound connections. |
| 54 | ECO-5 | Push when an agent or task finishes | partly there | M | 11 | The phone buzzes with a title only, no content. |
| 55 | ECO-6 | Phone camera as ask context (on request) | new | M | 11 | Photo of a paper or device goes with the question after a prompt on the phone. |
| 56 | ACT-2 | Background UI actions that keep your focus | partly there | L | 11 | FRIDAY clicks in another window through invoke patterns while you keep typing. |
| 57 | CTX-7 | Long answers and big outputs as files with a short card | new | S | 10 | A 5k-line output or table opens as a file link; the model sees head, tail and a summary. |
| 58 | PRV-14 | URL fetch guard (no loopback, link-local, LAN) | partly there | S | 10 | Fetch tools refuse internal addresses; tested. |
| 59 | PKG-4 | Scoop and Nix packages next to winget | partly there | S | 10 | `scoop install friday`, `nix run`. |
| 60 | ACT-4 | Undo for real actions | already planned | M | 10 | Window moves, launches, file writes and agent edits can be undone. |
| 61 | MEM-9 | Daily summary per project | partly there | S | 9 | A short local summary of the day, deletable. |
| 62 | CTX-8 | Follow-up thread compaction for small-context free models | new | M | 9 | Long follow-up chains are summarised so a 30-turn chat still fits a free model. |
| 63 | AGT-13 | Event-triggered routines (local events only) | partly there | M | 9 | A routine fires when a download finishes, a file changes or an app opens; no inbound webhooks. |
| 64 | AGT-14 | Offer a routine after repeated work (propose only) | already planned | M | 9 | After the same steps a few times FRIDAY drafts a routine you review. |
| 65 | CTX-9 | Capture receipt on the answer card | already planned | S | 8 | Before the first word the card says app, window, "text 2.1k chars" or "screenshot", and what was masked or skipped. |
| 66 | MEM-10 | Search past conversations as a view | partly there | S | 8 | What did you tell me about X last week with cited dates. |
| 67 | VOI-8 | Double-tap to latch dictation | new | S | 8 | Tap twice to keep recording without holding the key. |
| 68 | CTX-10 | Structured answer cards (tables, steps, code with Copy/Run) | new | M | 8 | Answers render tables and steps in both UIs; code blocks get Copy. |
| 69 | VOI-9 | Cheaper offline wake engine (sherpa-onnx / openWakeWord) | new | M | 8 | Idle CPU under a few percent, same engine on both OSes, benchmark in CI. |
| 70 | CTX-11 | Blank password fields and card inputs in screenshots | new | S | 7 | Password and card fields seen in the accessibility tree are painted over before a screenshot leaves. |
| 71 | MEM-11 | Secrets never reach memory or history | partly there | S | 7 | Keys on screen are masked in answers log, memory, lessons and recall index; a test plants one. |
| 72 | PKG-5 | Doctor repairs common breakage (`friday doctor --fix`) | partly there | S | 7 | Named failures (missing key bind, stopped service, bad config) repair themselves. |
| 73 | PKG-6 | First-run ends with a real success | already planned | S | 7 | Doctor and tour run a real ask on your own window. |
| 74 | PKG-7 | Config import from other assistants with dry run | new | S | 7 | Move provider keys and model choices over, preview first. |
| 75 | AGT-15 | Scheduled self-paced watch ("tell me when the build ends") | partly there | M | 6 | Cheap polling of a window or command with a done condition. |
| 76 | LAI-4 | Provider circuit breaker and key pools | partly there | S | 5 | A rate-limited key or provider is skipped for a while; several keys rotate. |
| 77 | PKG-8 | Migrations with a rollback point | new | S | 5 | Updates migrate settings and memory and restore them if interrupted. |
| 78 | CTX-12 | What changed on screen diff between two consented captures | already planned | M | 5 | Ask what changed since the last capture, as a short text diff. |
| 79 | MEM-12 | Export everything and import on a new computer | already planned | M | 5 | Plain files that rebuild workspaces, threads and preferences. |
| 80 | AGT-16 | Two agents, two models, one task, compared | new | L | 5 | A side-by-side run of two agents on one brief. |
| 81 | CTX-13 | Per-app answer profiles and quick chips | already planned | S | 4 | The ask box offers 2-3 actions for what is in front and answers in the style set for that app. |
| 82 | PRV-17 | See the exact payload the first time a provider gets a request | new | S | 4 | A one-time preview of the real request before the first send to a new provider. |
| 83 | CTX-14 | Clipboard as context, with expiry and password-manager skip | already planned | S | 3 | Explain what I copied works; old or password-manager clips are ignored. |
| 84 | CTX-15 | Stage timings per ask (key to first word) | partly there | S | 3 | `friday doctor` shows p50/p95 for capture, provider and render; CI fails on regressions. |
| 85 | MEM-13 | Retention rules by kind | partly there | S | 3 | Temporary context expires on its own; stable preferences stay. |
| 86 | VOI-12 | Dictation history | new | S | 3 | The last 20 dictations stay locally so a wrong-window dictation can be recovered. |
| 87 | VOI-13 | Spoken answer length: brief, normal, detailed | new | S | 3 | Voice answers can be shorter than the text card. |
| 88 | UI-5 | Themes and skins for the pill | partly there | S | 3 | A few built-in looks, light and dark, on both hosts. |
| 89 | CTX-16 | Strip noise from the accessibility tree before sending | new | M | 3 | Hidden controls and layout wrappers are dropped so prompts are smaller and faster on free models. |
| 90 | CTX-17 | Point at one control and ask about it | already planned | M | 3 | Pick an element or region; only its role, text and neighbours go to the model. |
| 91 | CTX-18 | Terminal scrollback as context | already planned | M | 3 | Why did this fail reads the last lines of the focused terminal, masked first. |
| 92 | PRV-18 | Privacy zones per app, site and folder | already planned | M | 3 | Local only, never read, never indexed. |
| 93 | PKG-9 | Per-feature health page | partly there | M | 3 | Shows voice, memory, local model, providers and screen access with their switch. |
| 94 | CTX-19 | Quote the screen line an answer relied on | new | S | 2 | The answer shows the exact phrase it used so you can check it. |
| 95 | ACT-5 | Frozen context at the keypress | already planned | S | 2 | Switching windows while FRIDAY thinks cannot change what it acts on. |
| 96 | MEM-14 | Forget really forgets, everywhere | partly there | S | 2 | Deleting a fact also removes derived summaries and the recall index; a test proves it. |
| 97 | VOI-14 | Local voice picker with a Play button | partly there | S | 2 | Hear each installed voice before choosing. |
| 98 | VOI-15 | Transcript preview before a voice note or ask is used | new | S | 2 | Edit or drop what whisper heard before FRIDAY acts on it. |
| 99 | UI-6 | Soft state sounds | new | S | 2 | Three quiet cues for listening, done and failed; off switch. |
| 100 | CTX-20 | Ask about a named window or two windows | new | M | 2 | Explain the terminal window or "compare these two windows" reads exactly those windows, with consent. |
| 101 | CTX-21 | Drop a file or image on the ask box | already planned | M | 2 | Drag a PDF, image or text file onto the pill to ask about it. |
| 102 | ACT-6 | Show me where (ring on the real control) | already planned | M | 2 | Where is Save? draws a ring on the control, also in Chromium pages. |
| 103 | AGT-17 | MCP client with a per-server switch | new | M | 2 | Add tools by pointing at an MCP server; each one switchable. |
| 104 | AGT-18 | Codex and Cursor as supervised agents (and ACP) | new | M | 2 | More coding agents work under the same follow-and-resume flow. |
| 105 | CTX-22 | Look again button when the model needs more | new | S | 1 | The card offers a re-capture button; the model can never capture by itself. |
| 106 | CTX-23 | Say plainly why a screen ask failed | already planned | S | 1 | The card says: no accessibility text, capture denied, or app unsupported, and what still works. |
| 107 | MEM-15 | Show which memories shaped an answer, with Forget | already planned | S | 1 | Used 2 memories on the card; one click removes a wrong one. |
| 108 | LAI-5 | Model picker eval for screen questions | new | S | 1 | A local benchmark ranks the free models on screen asks. |
| 109 | UI-7 | What's new card after an update | new | S | 1 | One card lists changes and new switches, once per version. |
| 110 | PRV-19 | Panic switch | new | S | 1 | One key stops agents, voice, capture and routines at once. |
| 111 | PKG-10 | Hotkey conflict check at setup | new | S | 1 | Doctor tells you when another app already owns the key. |
| 112 | PKG-11 | Release checksums, signature and SBOM page | partly there | S | 1 | Users can verify a download. |
| 113 | ECO-8 | Send a link or text from the phone share sheet | new | S | 1 | Phone share sends a URL to the PC to ask about. |
| 114 | MEM-16 | Meetings become project memory | already planned | M | 1 | A caption transcript turns into decisions and actions in the project thread. |
| 115 | VOI-16 | Voice activity gating before whisper | partly there | M | 1 | No audio reaches the model when nobody speaks; lower idle load. |
| 116 | UI-8 | FRIDAY's own accessibility | new | M | 1 | Screen-reader labels, keyboard use and high contrast for the pill and Settings. |
| 117 | ACT-7 | Safe recovery when a UI step fails | new | L | 1 | A failed click proposes one safe alternative instead of retrying blindly. |
| 118 | CTX-2 | GNOME and KDE support | new | L | 15 | The keys, the ask box and screen asks work on GNOME and KDE (their own shortcuts or the GlobalShortcuts portal, an ask box and screenshots that work there; grim and fuzzel don't). Needs a GNOME/KDE test VM. |
| 119 | MEM-17 | Meeting notes from what the computer plays | owner-approved (from Cicada, Apache-2.0) | M | owner | Opt-in, off by default: `friday meeting start` listens to system audio (the PipeWire monitor on Linux, WASAPI loopback on Windows), transcribes it with the local whisper, and keeps the transcript and short notes (decisions, to-dos) on this computer. "What did we decide in the 3pm call?" and `friday answers search` find them. Audio never leaves; text goes to a model only for the notes. Exit test: a recorded clip played through the speakers becomes a transcript and notes with nothing sent but text, and stops when told. Later and separate: live subtitles, voice interpreting. |
| 120 | LAI-6 | Public free models, opt-in, with a red warning | owner-approved | S | owner | Free tiers with a named operator and published terms (OpenRouter free models, Groq, Cerebras, Gemini, NVIDIA NIM), and last apisfreellm.com (no key, no terms, unknown operator), each added only when the person turns it on. A red line says the provider may keep or train on what is sent, so it should be treated as public. Typed questions only: screen, selection, clipboard, private-window and private-mode asks never go there. Never a default or an automatic fallback. Exit test: with one turned on, a typed ask reaches it and a screen ask and a private-mode ask do not. Not built: tools that pose as another app, pool accounts or get around rate limits. |

## The owner's ISO list (2026-11-24), worked before row 101

From the owner's live-ISO test, brainstormed with nine models (all nine agreed on the order below, give or take one place). Worked top to bottom before the rest of the table.

| # | Id | Item | Exit test |
|---|---|---|---|
| I1 | UI-9 | Super+I opens FRIDAY Settings; a Commands page lists every `friday` command; a key sheet built from the real Hyprland binds | Super+I opens Settings; a CI check fails when a command or bind is missing from the pages. |
| I2 | LAI-7 | Local AI step in the installer: what was found (GPU, VRAM, RAM, disk), a recommended model; Ollama shows Ollama-library models only, llama.cpp a GGUF list with an LFM2 rung for low-end machines; the list ships on the ISO, downloads run at first login and resume | In the VM, the step works offline, recommends a model that fits, and the model downloads after login. |
| I3 | PKG-12 | App bundles at first login (developer, gaming, productivity, creative): repo packages with pacman, AUR built as the user, one failure doesn't stop the rest, a report at the end | In the VM, two bundles install with one forced failure reported and retryable. |
| I4 | UI-10 | FRIDAY OS's own look (from the owner's own setup: Nord colours, small gaps, thin border, no blur or shadow, dimmed inactive windows, quick slide animations), own wallpaper and accent; Omarchy kept only for lock, idle and polkit | A screenshot of the default desktop has no Omarchy look; the look can be switched back. |
| I5 | UI-11 | Face unlock in the tour (only when a camera exists) and in Settings, with a short guide; the password always still works; Windows opens Windows Hello settings | In the VM with a fake camera the step shows; without one it doesn't. |
| I6 | UI-12 | Motion: one shared motion file for the pill, orb and transitions with a reduce-motion switch; installer ASCII art that plays only while it waits, skippable | Reduce-motion turns every animation off; the installer is unchanged on a slow console. |

Not built: xnulinux (arm64 only, Apple's licence, taints the kernel), atomic-engine (preview, Apple Silicon only), linger (licence). Possible later: a small-GPU llama.cpp preset that keeps a MoE model's busy experts on the GPU (`--n-cpu-moe`).

## Left out on purpose

Not built, because the models split on them or they need a decision first:

VOI-6 Per-stage voice choice, local by default, hosted only if you opt in; ACT-3 Visible browser tasks with take-over (existing browser, no bundled one); AGT-5 Standing goal checked by a judge, with a hard cap; PRV-15 Suggest allow-rules from what you always approve; PRV-6 Credential vault that logs you in without the model seeing the password; ECO-4 Owner-only Telegram bridge (one channel, opt-in); AGT-6 Skill folders in the agentskills.io format (import, review, off switch); ECO-7 Signed skill catalog for reviewed skills only; MEM-8 Encrypted sync of threads and workspaces between your machines; UI-4 Interface translation; UI-3 Break nudge from window titles (opt-in); UI-2 Pet that shows agent progress; VOI-4 Choose your own wake name; VOI-10 Wake name inside a sentence.

Also never built: avatar and persona engines, voice cloning, voiceprint login, always-on screen or mic, skill marketplaces, many chat channels, phone calls, a bundled browser, kernel hooks, a second memory engine, telemetry or cloud voice by default.
