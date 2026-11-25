# Using FRIDAY

Press a key, ask about what you're looking at, get an answer. This page is the
short tour of what FRIDAY does on Linux and where its data goes. Windows has the
same three hotkeys (Win+Y, Win+Alt+Y, Win+Ctrl+Y), an answer window with Copy,
Ask again, Follow up and Try another model, and web search ("search ..." or
"look up ..."), plus "What left this PC" and "Forget saved data" in the tray
menu. Three more work on Windows: "battery", "timer 5 min" and "wifi settings"
(also Bluetooth and sound) are answered on the PC with nothing sent, and a
question about selected text names the app and window title it came from (turn
that off in the tray menu). The pill, quick actions, voice, volume and do not
disturb, and the `friday` commands below are Linux only so far.

## The three keys

| Key | Asks about | Sent to the AI provider |
|---|---|---|
| Super+Y | whatever you type | your question |
| Super+Alt+Y | the text you selected (or copied) | the question, the text, the app name and window title |
| Super+Ctrl+Y | your screen | the question and the window's text when the app offers enough of it (faster, and less than a picture; it can include text scrolled out of view), otherwise one screenshot. `friday screen-text off` (Windows: the tray menu's "Read a window's text for screen asks") always sends the screenshot |

You can also just say it in the question: "what is on my screen" or "what am I looking at" takes the screen path
(same one-screenshot rule as Super+Ctrl+Y), while a how-to like "how do I change my screen brightness" does not.
A question that just says "this" with nothing picked ("explain this", "is this right?") makes FRIDAY ask which you
mean: the text you selected (Linux), the window in front, or just your words. On Windows a chip under the field
says so: Ctrl+Enter means the window, Enter just your words. `friday feature which-this off` (Windows: Settings →
Features) turns it off.

Hold Super+Y instead of tapping it to talk (`friday voice setup` first). The
speech is turned into text on your computer; only the text is sent. On niri, which can't see a key being released, press Mod+Y once to speak and again to ask.
On Windows, hold Win+Y the same way after Settings, Voice, "Set up talking" (whisper.cpp and its model, about 150 MB, checked against pinned checksums); a tap still opens the ask box, and `hold-to-talk` under Features switches it off.

**Voice typing:** hold Super+Shift+Y (Mod+Shift+Y on niri: press once to speak, again to type), speak, and let go:
the words are typed into the field you were in, and nothing is asked or sent. It uses the same voice setup
(`friday voice setup`) and needs `wtype`. On a FRIDAY OS install made before this key existed, run `friday keys update` once to add it (`friday doctor` says so).

What you say (typed or asked) comes out without "um" and "uh". To have names and tools spelled your way, list
them in `~/.config/friday/dictionary` (Windows: `%APPDATA%\friday\dictionary`), one per line: a word on its
own (`Kubernetes`) is one whisper should know, and `heard => written` (`cube cuddle => kubectl`) fixes what it
hears. `friday feature dictation-cleanup off` (Windows: Settings → Features) turns both off.

With a selection, the box offers quick actions (Summarize, Explain, Translate,
Fix the grammar) and your own prompts, one per line in
`~/.config/friday/prompts` (on Windows `%APPDATA%\friday\prompts`). Shift+Enter sends what you typed instead (on Windows, type your own and press Enter).

## The answer pill

The answer opens in a pill under the bar and grows as it streams in. While FRIDAY listens, thinks or speaks,
the pill shows ribbons of light that flow like a voice (slow while listening, quick while thinking).

- **Copy**, or **Type it** into the window you were in (replacing the selection).
- **Follow up** asks about that answer; **Ask again** repeats the question the
  same way; **Try another model** asks a different model to answer with more care.
- **Full answer** opens it in a terminal. Esc or a quiet minute puts it away.

Questions asked within 15 minutes of each other are one conversation: FRIDAY
sees your last few questions and its answers, so "and the second one?" works,
even after FRIDAY restarts. After 15 quiet minutes the next question starts a
new one; `friday ask --new` starts one at once. `friday forget memory` wipes
the conversation along with what FRIDAY remembers. When a question says something
about you ("I use fish, how do I …", "my editor is Helix"), the answer offers
"Remember: you use fish" (a notification button on Linux, a button on the
Windows answer card); nothing is kept unless you click it. `friday feature
propose-memories off` (Windows: Settings → Features) stops the offers. What FRIDAY remembers about you lives in one plain file you can read and edit,
`~/.config/friday/memory.md` (Windows: `%APPDATA%\friday\memory.md`, or Settings → Privacy, "What FRIDAY
remembers about you"), one `- ` line each; every ask carries it, and the line under an answer says so.
`friday memory` lists it, `friday memory edit` opens it, `friday memory forget <words>` takes a line out, and
`friday forget memory` clears it with the rest. Only "remember that …" adds a line ("remember to buy milk" doesn't), anything shaped like
a key is masked before it goes out, and `friday feature memory-file off` stops asks carrying it.
- Past answers are searchable in the FRIDAY drawer.

## Without a model

Some things FRIDAY does itself and sends nowhere: volume, mute, do not disturb,
battery, timers, opening Wi-Fi settings. The answer says
"On this computer · nothing was sent".

## Control the desktop (FRIDAY OS)

On FRIDAY OS, tell FRIDAY what to do with your windows and apps and it does it:
"put Firefox on workspace 2", "switch to workspace 3", "set the volume to 30",
"pause the music", "open Firefox and move it to workspace 4", "what's playing".
It looks at your windows, takes the steps one at a time and tells you what it did.

Looking and reversible changes (focus, move, launch, volume, media, brightness)
just happen, because you asked. Closing a window always asks first and shows
the exact action. When the ask carries text from outside your own words (selected
text, the clipboard, a file, a web page), or follows one in the same conversation,
every action asks first: opening a page, reading a file, moving a window, starting
a coding agent or a standing task. That text might hold instructions, and only you
decide what runs. It stops after 8 steps. Super+Ctrl+Esc (Mod+Ctrl+Esc on niri)
stops whatever it is doing at once, as does Stop on the answer card, and
The FRIDAY drawer shows the last few things it did (and any that didn't work), and `friday actions` lists everything it did (which action, with what, whether it
worked, never what was on your screen). Questions ("how do I change the
volume?") are answered, not acted on. `FRIDAY_DESKTOP=0` in
`~/.config/friday/friday.env` turns this off.

On Windows the same works for windows and programs: "focus Notepad", "open
calc", "close the Firefox window" (asks first, with the exact action), "what
windows are open", and the volume and music: "set the volume to 30", "mute",
"pause the music", "next song", "what's playing", "set the brightness to 40" (a laptop's own screen; external monitors don't offer it to Windows), "how many desktops do I have" and "switch to desktop 2" (or next, previous). It uses FRIDAY's Windows
helper (the one the tray runs), so FRIDAY must be running from its tray icon.
Media controls reach whatever app is playing through Windows' media session
(browsers, Spotify, the media player); volume is the default speakers'.
Moving a window to another desktop and the power profile are FRIDAY OS only (Windows has no stable way to do either). Launching takes a program name that
Windows can find (`notepad`, `calc`, `mspaint`), not a path or a command line.
`FRIDAY_DESKTOP=0` in `%APPDATA%\friday\friday.env` turns it off. Looking
inside apps (below) uses the agent-desktop the installer bundles.

It can also look inside apps and press their controls (with [agent-desktop](https://github.com/vercel-labs/agent-desktop) when it is installed, else FRIDAY's own reader; `FRIDAY_UI_ENGINE=atspi|agent-desktop|auto` in `~/.config/friday/friday.env` chooses): "click the Save button
in gedit", "what tabs does pavucontrol have". This works with apps that offer
accessibility actions (GTK3, Qt, Electron). GTK4 apps such as Nautilus can be
read but their buttons can't be pressed from here, and FRIDAY says so. A button
that could delete or send something (Delete, Send, Pay, Install, Allow...) asks
first and shows the button's name. Passwords are never read or typed into. What
it reads inside an app goes to the AI model answering you; in private mode that
model is on your computer.

## Talking to FRIDAY

When something you asked by voice needs your OK, FRIDAY stops talking, plays a short sound and listens once (about six seconds, only while the Run/Cancel prompt is on screen): say "run" or "cancel" ("yes", "no" and the like work too, but only as the whole answer). Something that can't be undone, like deleting files or closing a window, can be cancelled by voice but needs a click (or your phone) to run. A question you typed never opens the microphone. `friday feature voice-confirm off` (Windows: Features, voice-confirm) turns it off.

With spoken answers on, an answer to something you asked by voice is followed by the same sound once it has been read to the end: ask a follow-up right away, no key needed ("thanks" or "stop" ends it; stopping the reading or closing the answer skips it). `friday feature voice-followup off` turns that off.

With "Hey Friday" on, saying it while FRIDAY is reading an answer stops the reading and listens. While a call
or meeting app (Zoom, Teams, Slack, Discord, a browser and the like) is using the microphone, "Hey Friday"
rests, with a notification when it pauses and when it listens again. `friday feature call-pause off`
(Windows: Settings → Features) keeps it listening during calls.

To stop FRIDAY talking, press the talk key or close the answer (Esc). On Linux, `friday voice talk-over on` lets you simply talk over a spoken answer: PipeWire's echo cancellation gives FRIDAY its own microphone and speaker (your default devices stay as they are), so it hears you and not itself; it stops and asks what you said. `friday voice talk-over off` removes it.

Asking something new while an answer is still on its way replaces it: the newest question wins (on Windows the old one is stopped; on Linux its answer is still kept in `friday answers`, just not shown or read out).

## Spoken answers (FRIDAY OS)

FRIDAY can read its answers out loud in a small voice that is made on your computer: nothing is sent
anywhere to speak, and it needs no GPU.

```
friday voice speak setup     download the voice once (about 50 MB, checked against a pinned checksum)
friday voice speak on        read every answer aloud (off: friday voice speak off)
friday voice speak voices    the installed voice's names (Female, Male, ...); pick one: friday voice speak voice 1
friday say "Hello"           say something now;  friday say --stop  stops it
```

The installer can talk you through the install too: after the network check it asks (default No) and downloads the
voice into memory for that session only; nothing is kept on the new system.

It speaks a sentence at a time, so you hear the start while the rest is still being made, and skips code
blocks. A new answer replaces the one being read. The default is **tiny voca** (Apache-2.0, about 50 MB): a
4.5-million-parameter voice (Female or Male) that runs inside FRIDAY itself, several times faster than real time
on a laptop CPU, so speech starts at once. It is clear and natural. FRIDAY's own Rust port of its phonemizer
(dizione, Apache-2.0) reads numbers, dates, money, units and web addresses aloud, and the tests check it against
the original Python code. For something more expressive:

- `friday voice speak setup kokoro` installs **Kokoro** (Apache-2.0, about 150 MB), with Bella as the default
  voice and seven more to choose from (Heart, Nicole, Sarah, Sky, Nova, Emma, Isabella). It is the most
  expressive of the voices that run on a laptop, and also the heaviest: it makes speech at about real time on a
  typical CPU, so FRIDAY works on three sentences at once (about 1 GB of memory while it speaks, and a few seconds
  before the first words).
- `friday voice speak setup kitten` installs Kitten TTS nano (Apache-2.0), and `friday voice speak setup supertonic`
  installs Supertonic 2 (weights under the [OpenRAIL-M licence](https://huggingface.co/Supertone/supertonic-2)).

## How is my computer doing?

Ask "how is my computer doing", "is my system healthy" or "why is my laptop so slow". FRIDAY looks (read-only) at
failed services, this boot's errors, disk, memory, load and battery (on Windows: disks, memory, uptime, battery,
stopped automatic services and this boot's critical errors), says in plain words what is wrong and the one safest
fix, and never runs a fix unless you ask. `friday doctor` checks FRIDAY itself.

## The first-login tour

FRIDAY OS walks you through the keys the first time you log in: each key is shown as a keycap, you try it, and it
moves on. The steps offer buttons for turning on a voice and pairing a phone. Take it again any time with
`friday tour`. On Windows the first run opens a welcome window with the same idea (reopen it from the tray menu's
"Welcome…").

After an update, the first login shows one card with what changed since the version you last saw and any new
feature switches. It shows once per version; `friday whats-new` shows it again, and `friday feature whats-new off`
turns the card off (Linux; Windows to follow).

## Other ways to ask

```
friday ask --region      drag out an area of the screen and ask about it
friday ask --web "..."   search the web and answer with sources
                         ("search ..." and "look up ..." do the same)
friday ask --follow-up   ask about the last answer
```

Web search happens only when you ask for it, and never in private mode.

## Privacy

- `friday sent` lists what left this computer: which provider, text or
  screenshot, how many secrets were masked first. Keys, passwords and tokens in
  what you send are masked before sending.
- `friday private on` answers with a local model (Ollama) and sends nothing; it also starts no coding agent, since an agent sends the folder's code to its own AI.
- `friday app-context off` stops sending the app name and window title.
- From a mail or chat app (Thunderbird, Outlook, Signal, Discord, Slack…), the ask box says the text goes to your AI provider first; with a local model it stays on this computer. Your own list: `~/.config/friday/sensitive-windows`; turn off with `friday feature sensitive-warn off`.
- `friday forget` shows what FRIDAY keeps (counts only) and wipes it:
  `friday forget answers|sent|memory|all`.

On Windows, **Settings…** in the tray menu opens a normal window for all of this: start at
sign-in, starting with administrator rights (so FRIDAY can control apps that run as
administrator; Windows asks once), naming the app, reading a window's text, the AI provider
and accounts, desktop control, coding agents, and the keys FRIDAY answers to.

Setup asks what FRIDAY may do (FRIDAY OS: the "What may FRIDAY do?" step; Windows:
the installer's Customize page and the tray menu): control windows, apps, volume
and music; run scheduled tasks; name the app a selection came from; send a
window's text instead of a screenshot. All are on by default, and each
can be changed later (`FRIDAY_DESKTOP=0`, `FRIDAY_STANDING_TASKS=0`,
`friday app-context off`, `friday screen-text off` or the Windows tray menu).

API keys you add are kept out of the plain settings file where the system has a keychain. On Windows
they go to Credential Manager (a key already in `friday.env` moves there when the tray starts; the file
keeps `NAME=keyring:` in its place). On Linux, `friday secrets keychain` moves them to your desktop
keyring (GNOME Keyring or KWallet, through `secret-tool`) and `friday secrets file` moves them back; a
keyring that is still locked when FRIDAY starts leaves those providers out until you unlock it and
restart FRIDAY (`systemctl --user restart friday-linux-daemon`).

## Accessibility

FRIDAY Shell works with a screen reader (Orca) and the keyboard: every button, switch, slider and Settings page says what it is and whether it's on, Tab moves between them, and Enter or Space presses them. For high contrast, `friday shell palette contrast` (or Super+N → Look) gives black, white text and edges, a yellow accent and no see-through glass; window borders follow it. `friday shell palette core` goes back.

## The launcher

Click the grid in the bar. Type to find an app, one of FRIDAY's quick commands
(mute, do not disturb, timer, battery, Wi-Fi and Bluetooth, settings, the tour) or a file
by name (needs `fd`; files are looked up on this computer). Arrows move, Enter opens.
The last row, "Ask FRIDAY", is the only one that asks the AI: typing alone sends nothing.

## The desktop

FRIDAY OS has its own keymap (Super+K lists it), Super+L locks (with face unlock
after `friday face setup`, below), and Super+Alt+G switches game mode. `friday doctor`
checks that everything is running; `friday report` writes a redacted bug report.
`friday feature motion off` makes FRIDAY Shell change at once, with no sliding,
springing or pulsing; `friday.nomotion` on the installer's boot line keeps its
waiting screen still too.

## App bundles

`friday bundles` installs apps for a kind of work in one go: developer
(Neovim, VS Code's open-source build, Zed, Claude Code, git tools), gaming
(Steam, Lutris, Wine, GameMode), productivity (LibreOffice, Obsidian,
Thunderbird) or creative (GIMP, Inkscape, Krita, Blender, Kdenlive, OBS).
Apps come from the Arch repos where they're there, and from the AUR (built as
you, never as root) where they're not. One that fails never stops the rest;
the end says what didn't install and how to try again, and a notification says
it's done. Gaming turns on Arch's 32-bit `multilib` repo, which Steam needs.
`friday bundles list` shows what's in each. The first-login tour offers them;
`friday feature app-bundles off` takes that step out.

## Face unlock

The lock screen can know your face. The first-login tour offers it when the
computer has a camera; any time later, run `friday face setup` in a terminal.

1. It says what it will do and asks before starting.
2. If Gaze isn't installed yet, it installs it (`gaze-bin`, built from the AUR
   on this computer), which asks for your password.
3. Gaze learns your face: look at the camera and follow its prompts.
4. It turns face unlock on for the lock screen. Then it asks whether to use
   your face for `sudo` and password prompts too; say no if you're unsure.
5. Press Super+L and look at the camera to try it. Typing your password always
   works too, so a failed look never locks you out.

Your face is checked on this computer; no picture is sent anywhere. An
ordinary webcam can be fooled by a photo more easily than a laptop's infrared
camera, so keep it to the lock screen on a shared or mobile machine.
`friday face status` says what's on; `friday face remove` turns it all off
(Gaze stays installed: `gaze uninstall` removes it).

On Windows, face unlock is Windows Hello: Settings → Accounts → Sign-in
options.

## System helper

FRIDAY can update the system, install, or remove packages, but only if you
turn this on: `friday helper enable`. Even with it on, every action still asks
for your approval in a system prompt before anything changes.
Check whether it is on with `friday helper status`, or turn it off anytime with
`friday helper disable`.

## Standing tasks

Ask for something to happen again and again: `friday ask "every morning at 8 tell me the weather"`, `"every weekday at 9 summarize my unread notes"`, `"when my battery drops below 20% tell me"`. FRIDAY runs each one the way it runs your own ask, so the same rules apply: nothing is sent that a normal ask wouldn't send, and anything that deletes or sends stops and asks you first (you'll see the usual confirmation). Answers arrive as a notification and in `friday answers`. To keep them quiet at night, `friday tasks quiet 22-7` (Windows: Settings → Tasks, "Quiet at night"): tasks that repeat wait until the quiet hours end and catch up then, while reminders and battery alerts still come. At most 50 task runs happen in a day (`friday tasks limit <n>` changes that), so a task gone wrong can't use up your AI provider's day.

The FRIDAY drawer lists them (up to four) with pause/resume and delete buttons. `friday tasks` lists them all; `friday tasks pause t1`, `resume t1` and `delete t1` change one. A morning task missed while the computer was off still runs if it's less than six hours late. Up to 20 tasks, at least 5 minutes apart. `FRIDAY_STANDING_TASKS=0` turns them off.

On Windows, ask the same way with Win+Y. When a task is due, its answer opens like any other answer, while FRIDAY is running. Settings → Tasks lists them with Pause/Resume and Delete.

## Coding agents

FRIDAY can hand work to a coding agent and follow it for you. Ask with the agent's name and a folder, for example `friday ask "have opencode add a README to ~/code/site"` or `"ask agy to fix the failing tests in ~/code/app"`. FRIDAY writes the task for the agent, starts it in that folder, and returns straight away; you get a notification when it's done. Without a folder, it uses the last one that agent worked in, and the ask box says which before you press Enter: on Windows a chip under the field ("opencode works in ~\code\site"; click it to pick another folder an agent has worked in), on Linux the box's hint names the newest run's folder. To add to what it just did, say so: "tell opencode to also add tests" or "tell the agent to use tabs". A follow-up goes on in the same agent session, so the agent remembers what it did, when the run before ended in the last 15 minutes (otherwise it starts a new session in the same folder, and says so). Sent while the agent is still working, it waits for that run to finish. If the agent no longer has the session, the run starts fresh and its summary says "Started fresh". `friday feature agent-resume off` makes every follow-up a new session. To have FRIDAY check an agent's work, turn on `friday agents check on` (Windows: Settings → Agents, "Check an agent's work"): when an agent says it's done, FRIDAY runs the project's own tests (`cargo test`, `npm test`, `go test`, `pytest` or `make test`, whichever the folder has) at low priority for up to ten minutes, without your keys in its environment, and the summary and notification say what they showed; failing tests make the run a failure, whatever the agent said. It's off by default because the tests run code the agent just wrote. To keep an agent's changes apart until you've seen them, turn on `friday agents worktree on` (Windows: Settings → Agents, "Keep an agent's changes apart"): in a git folder each run then works on its own branch, in a copy in FRIDAY's state folder (a follow-up goes on in the same copy), and your folder stays as it was. `friday agents accept <id>` brings the changes in, staged for you to look over and commit; `friday agents discard <id>` drops them (Windows: the buttons under the run in Settings → Agents). It works with opencode, agy (Antigravity), Claude Code and Codex, whichever is installed and signed in (Codex through its ACP adapter: `npm i -g @agentclientprotocol/codex-acp`; FRIDAY puts each Codex session in its Read-only mode, so every edit and every command beyond plain reads asks, and stops it if it leaves that mode; since plain reads don't ask, Codex runs only in FRIDAY's sandbox, which hides your keys and the other agents' sign-ins: `friday agents sandbox on`, Linux for now); for Claude Code say "Claude Code", or name a folder ("ask claude to fix the build in ~/code/app"), since "claude" alone may just mean the model. A Claude Code run counts against your Claude plan, and permissions you've already allowed in your own Claude Code settings still apply without asking FRIDAY.

While the agent works, FRIDAY answers its approvals for you, by fixed rules rather than by an AI's judgement. Text the agent reads (a README, a web page) can't talk FRIDAY into anything. FRIDAY approves:

- reading and searching inside the folder, except files that look like secrets;
- editing files inside the folder, except `.git`, files that look like secrets (`.env`, keys, certificates), files that decide what a build or test runs (`package.json`, `Makefile`, `build.rs`, `Cargo.toml`, `.cargo/`, `conftest.py`, `pyproject.toml`, CI workflows, git hooks), files git ignores (`node_modules`, `target`) and `.gitignore`, `.gitattributes` or `.gitmodules`, which decide what git shows as changed;
- read-only commands, such as `git status`, `git diff` and `ls`;
- builds and tests, such as `cargo test` and `npm test`, while git shows no uncommitted change in the folder.

A build or test runs the project's own code, so once the folder has changes nobody has committed (the agent's edits, or yours), each build or test comes to you first: a tricked agent could otherwise write harmful code into a test and run it. A folder that isn't a git repository always asks.

Anything else waits for you with a notification: deleting or moving files, `git commit` or `git push`, installing packages, fetching from the web, anything outside the folder (links that point out of it included), and any command it doesn't recognise. Answer with `friday agents allow <id>` or `friday agents deny <id>`. A step nobody answers in 30 minutes is refused. FRIDAY only ever allows a step once; it never tells an agent to "always allow". When the agent asks a question your request already answers, FRIDAY answers it, up to three times a run. Any other question comes to you as a notification; answer with `friday agents reply <id> "your answer"`, and the agent carries on.

FRIDAY does the same in agent sessions you start yourself, with the session's folder as the project. Sessions in your home folder, or near the top of the disk, get nothing approved.

- **Claude Code:** before it shows you a permission prompt, it asks FRIDAY, which approves the step if the rules allow it. Anything else shows in your terminal as usual, and FRIDAY sends a notification. It never sees sessions that don't ask for permission (bypass mode).
- **agy:** before each step, it asks FRIDAY. A step the rules don't allow waits for you, even when agy is set to go ahead on its own: answer it with `friday agents allow <id>` or `deny <id>` (on Windows, the tray's prompt). No answer in 5 minutes means no. Unattended runs (`agy -p`) aren't held, since nobody is there to answer; agy decides those as it would without FRIDAY.
- **opencode:** FRIDAY follows opencode's background service and answers the routine permissions it asks for; the rest wait in opencode's own prompt, with a notification.

FRIDAY adds itself to Claude Code's settings (`~/.claude/settings.json`) and agy's hooks (`~/.gemini/config/hooks.json`) the first time it runs, leaving everything else in those files as it was. `friday agents hooks off` takes it out of all three, and `friday agents hooks on` puts it back.

On Windows, a step or question that waits for you opens a small FRIDAY window with Allow and Deny (or a box for your reply), and a finished run shows a notification from the tray. Settings → Agents lists recent runs and has the switches for agent runs and for approvals in your own Claude Code sessions.

**Your own instructions for an agent:** write them in `~/.config/friday/agents/<agent>.md` (`opencode.md`, `agy.md` or `claude.md`; on Windows `%APPDATA%\friday\agents\`), for example "use tabs, never touch vendor/". FRIDAY puts them in front of the task whenever that agent starts a new session, never writes the file itself, and never lets it change what it approves. The file sits outside any project folder, so an agent that tries to edit it has to ask you first. No file, no instructions.

`friday agents` lists the runs and `friday agents show <id>` reports one: what you asked, what the agent said, and every step approved by FRIDAY, approved by you, or refused. `friday agents off` stops FRIDAY from starting agents (`FRIDAY_AGENTS=0`).

## Questions about FRIDAY

Ask FRIDAY about itself, for example "how do I ask FRIDAY about my screen?" or "which hotkey asks about selected text?". A question that names FRIDAY, a hotkey or a shortcut gets the matching part of this guide, the copy installed with your version, and the answer says so ("FRIDAY guide: The three keys"). Nothing extra is sent beyond that excerpt. `friday feature self-docs off` (Windows: Settings → General → Features) turns it off.

## Troubleshooting

Tell FRIDAY what's wrong, for example `friday ask "my wifi keeps dropping"`, `"why is my disk full"` or `"troubleshoot my sound"`. FRIDAY looks before it answers: it runs diagnostic commands that only read, such as `nmcli device status`, `journalctl -b -p err`, `df -h` and `wpctl status` (on Windows: `ipconfig`, `Get-NetAdapter`, `Test-NetConnection`). It reads what they print and answers with what it found, in up to four rounds.

A fixed list decides which commands may run, not the AI. Each command may only take the options on its list, so ones that print secrets (saved Wi-Fi passwords, a process's environment) or read a file they're given are refused too. Nothing that changes your system runs here: no restarts, no installs, no `sudo`, no reading your files or keys. When the fix needs such a command, FRIDAY tells you what it is so you can run it yourself, or say `friday ask "run <command>"`, which asks you first. Command output has secrets masked before it goes to your AI provider. `FRIDAY_DIAGNOSE=0` turns troubleshooting off.

## Your phone (FRIDAY Mobile)

The installer offers pairing at first login unless you untick it (the "What may FRIDAY do?" step). FRIDAY OS installs [FRIDAY Mobile](https://github.com/sreevarshan-xenoz/friday-mobile), FRIDAY's phone app, which shows this computer on your Android phone and lets the phone act as its screen, keyboard and remote. It stays off until you turn it on: `friday phone on` asks first, starts it in your session and prints a PIN to enter in the FRIDAY Mobile app on the phone (`friday-mobile-…-android-arm64.apk` from its releases); `friday phone off` stops it and `friday phone status` says where it stands.

Once a phone is paired and has "Answer my questions" turned on (its Devices page; it is off by default), FRIDAY's questions before it does anything that deletes or sends also go to the phone as a notification. Answer on either one: the first answer wins. No on the phone is one tap; Yes needs the phone's fingerprint or screen lock, so a phone without one can only say No. With no phone set up, nothing changes.

To ask FRIDAY from the phone, add a deck button in FRIDAY Mobile's Settings ("Deck buttons"): a label like "Ask FRIDAY" that presses `super+y`. A tap on the phone opens FRIDAY's ask box on the computer, as if you had pressed the key.

