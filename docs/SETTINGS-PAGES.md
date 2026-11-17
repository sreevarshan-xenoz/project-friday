# FRIDAY Settings: the pages

One list for both apps, so a person who never opens a terminal can do everything
the `friday` commands do. The Linux window lives in `packaging/linux/shell/settings/`
(`friday app`); the Windows window is `windows-tray/FridayTray/SettingsForm.cs`
(Start menu, or the tray menu's Settings). Labels are plain words; each setting
says what it does in a line underneath.

| Page | What it does | Linux (command underneath) | Windows |
|---|---|---|---|
| Home | status, ask, "how is my computer doing?", tour or welcome | `ask`, `tour` | Home |
| AI | choose the AI and key (adds to saved keys), free models only (asks first), connect an account, private mode | `setup-ai`, `connect`, `private` | AI |
| Voice | read answers aloud, download the voice, hear it, stop, pick a voice | `voice speak`, `say` | Voice |
| Privacy | name the app, read a window's text, what left this computer, forget my data | `app-context`, `screen-text`, `sent`, `forget` | Privacy |
| Health | health look, check FRIDAY, restart, save a report | `ask`, `doctor`, `daemon restart`, `report` | Health & updates |
| Desktop control | let FRIDAY control windows and apps | (Windows only for now) | Desktop control |
| Shortcuts | the keys and what they do | (shown on Home) | Shortcuts |

Not in an app yet (still terminal-only): phone pairing (`phone`), face unlock (`face`),
scheduled tasks (`tasks`), shell look (`shell style|palette`, in the control drawer),
voice typing (`voice`). Add each to a page, then to the list in
`iso/friday/test/checks/static.sh` ("FRIDAY Settings covers the commands").

Rules for a page: a switch reads its state from the command's own `status` (never a
second copy); nothing that deletes runs without a confirm; a pasted key goes to the
command on stdin, never on a command line; saving a key keeps the others.
