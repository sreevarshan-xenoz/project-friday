# Using FRIDAY

Press a key, ask about what you're looking at, get an answer. This page is the
short tour of what FRIDAY does on Linux and where its data goes. Windows has the
same three hotkeys (Win+Y, Win+Alt+Y, Win+Ctrl+Y), an answer window with Copy,
Ask again, Follow up and Try another model, and web search ("search ..." or
"look up ..."). The pill, quick actions, voice, local commands and the
`friday` commands below are Linux only so far.

## The three keys

| Key | Asks about | Sent to the AI provider |
|---|---|---|
| Super+Y | whatever you type | your question |
| Super+Alt+Y | the text you selected (or copied) | the question, the text, the app name and window title |
| Super+Ctrl+Y | your screen | the question and one screenshot |

Hold Super+Y instead of tapping it to talk (`friday voice setup` first). The
speech is turned into text on your computer; only the text is sent.

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
