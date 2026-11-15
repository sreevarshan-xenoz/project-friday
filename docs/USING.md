# Using FRIDAY

Press a key, ask about what you're looking at, get an answer. This page is the
short tour of what FRIDAY does on Linux and where its data goes. Windows has the
same three hotkeys (Win+Y, Win+Alt+Y, Win+Ctrl+Y), an answer window with Copy,
Ask again, Follow up and Try another model, and web search ("search ..." or
"look up ..."), plus "What left this PC" and "Forget saved data" in the tray
menu. The pill, quick actions, voice, local commands and the `friday` commands
below are Linux only so far.

## The three keys

| Key | Asks about | Sent to the AI provider |
|---|---|---|
| Super+Y | whatever you type | your question |
| Super+Alt+Y | the text you selected (or copied) | the question, the text, the app name and window title |
| Super+Ctrl+Y | your screen | the question and one screenshot |

Hold Super+Y instead of tapping it to talk (`friday voice setup` first). The
speech is turned into text on your computer; only the text is sent. On niri, which can't see a key being released, press Mod+Y once to speak and again to ask.

With a selection, the box offers quick actions (Summarize, Explain, Translate,
Fix the grammar) and your own prompts, one per line in
`~/.config/friday/prompts`. Shift+Enter sends what you typed instead.

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
