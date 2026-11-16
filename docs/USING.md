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
| Super+Ctrl+Y | your screen | the question and one screenshot. After `friday screen-text on` (Windows: the tray menu's "Read a window's text for screen asks"), the window's text instead when the app offers enough of it (it can include text you can't see, and goes to any provider you use) |

Hold Super+Y instead of tapping it to talk (`friday voice setup` first). The
speech is turned into text on your computer; only the text is sent. On niri, which can't see a key being released, press Mod+Y once to speak and again to ask.

With a selection, the box offers quick actions (Summarize, Explain, Translate,
Fix the grammar) and your own prompts, one per line in
`~/.config/friday/prompts` (on Windows `%APPDATA%\friday\prompts`). Shift+Enter sends what you typed instead (on Windows, type your own and press Enter).

## The answer pill

The answer opens in a pill under the bar and grows as it streams in.

- **Copy**, or **Type it** into the window you were in (replacing the selection).
- **Follow up** asks about that answer; **Ask again** repeats the question the
  same way; **Try another model** asks a different model to answer with more care.
- **Full answer** opens it in a terminal. Esc or a quiet minute puts it away.
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
the exact action. It stops after 8 steps. Super+Ctrl+Esc (Mod+Ctrl+Esc on niri)
stops whatever it is doing at once, as does Stop on the answer card, and
`friday actions` lists everything it did (which action, with what, whether it
worked, never what was on your screen). Questions ("how do I change the
volume?") are answered, not acted on. `FRIDAY_DESKTOP=0` in
`~/.config/friday/friday.env` turns this off.

On Windows the same works for windows and programs: "focus Notepad", "open
calc", "close the Firefox window" (asks first, with the exact action), "what
windows are open", and the volume and music: "set the volume to 30", "mute",
"pause the music", "next song", "what's playing". It uses FRIDAY's Windows
helper (the one the tray runs), so FRIDAY must be running from its tray icon.
Media controls reach whatever app is playing through Windows' media session
(browsers, Spotify, the media player); volume is the default speakers'.
Virtual desktops and brightness are FRIDAY OS only for now. Launching takes a program name that
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
- `friday private on` answers with a local model (Ollama) and sends nothing.
- `friday app-context off` stops sending the app name and window title.
- `friday forget` shows what FRIDAY keeps (counts only) and wipes it:
  `friday forget answers|sent|memory|all`.

## The desktop

FRIDAY OS has its own keymap (Super+K lists it), Super+L locks (with face unlock
after `friday face setup`), and Super+Alt+G switches game mode. `friday doctor`
checks that everything is running; `friday report` writes a redacted bug report.

## System helper

FRIDAY can update the system, install, or remove packages, but only if you
turn this on: `friday helper enable`. Even with it on, every action still asks
for your approval in a system prompt before anything changes.
Check whether it is on with `friday helper status`, or turn it off anytime with
`friday helper disable`.

## Standing tasks

Ask for something to happen again and again: `friday ask "every morning at 8 tell me the weather"`, `"every weekday at 9 summarize my unread notes"`, `"when my battery drops below 20% tell me"`. FRIDAY runs each one the way it runs your own ask, so the same rules apply: nothing is sent that a normal ask wouldn't send, and anything that deletes or sends stops and asks you first (you'll see the usual confirmation). Answers arrive as a notification and in `friday answers`.

The FRIDAY drawer lists them (up to four) with pause/resume and delete buttons. `friday tasks` lists them all; `friday tasks pause t1`, `resume t1` and `delete t1` change one. A morning task missed while the computer was off still runs if it's less than six hours late. Up to 20 tasks, at least 5 minutes apart. `FRIDAY_STANDING_TASKS=0` turns them off.

## Your phone (Pocketdesk)

FRIDAY OS installs Pocketdesk, which shows this computer on your Android phone and lets the phone act as its screen, keyboard and remote. It stays off until you turn it on: `friday phone on` asks first, starts it in your session and prints a PIN to enter in the Pocketdesk app on the phone; `friday phone off` stops it and `friday phone status` says where it stands.

Once a phone is paired and has "Answer my questions" turned on (its Devices page; it is off by default), FRIDAY's questions before it does anything that deletes or sends also go to the phone as a notification. Answer on either one: the first answer wins. No on the phone is one tap; Yes needs the phone's fingerprint or screen lock, so a phone without one can only say No. With no phone set up, nothing changes.

To ask FRIDAY from the phone, add a deck button in Pocketdesk's Settings ("Deck buttons"): a label like "Ask FRIDAY" that presses `super+y`. A tap on the phone opens FRIDAY's ask box on the computer, as if you had pressed the key.

