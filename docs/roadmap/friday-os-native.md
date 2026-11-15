# FRIDAY OS: native control

The goal: on FRIDAY OS, FRIDAY feels like part of the system. You tell it what
you want ("put Firefox on workspace 2 and play my focus playlist", "update
the system and tell me when it's done") and it does it, across apps, without a
click per step. On Windows and the `friday-git` package on Linux it does what
the platform allows; FRIDAY OS gets everything.

## What "kernel level" means here

No kernel modules or drivers (`AGENTS.md`: only if nothing else can do it, and
nothing here needs one). The same feel comes from a trusted system layer:

- an always-on daemon in the user session, already there;
- compositor IPC (Hyprland), D-Bus (MPRIS, NetworkManager, notifications, power),
  systemd, and AT-SPI for the interface of apps that expose it;
- a small root helper behind polkit for system work (packages, services, network),
  so FRIDAY never runs as root itself.

## Consent, kept narrow

FRIDAY acts only on an explicit request, and what it may do without asking
again depends on the action's class:

| Class | Examples | Rule |
|---|---|---|
| Read | list windows, read the focused app's title, now playing | always allowed during a request |
| Reversible | launch an app, focus or move a window, switch workspace, volume, media, DND, brightness | allowed for the task once you say "let it run" (or standing permission per class, off by default) |
| Destructive or outward | close a window with unsaved work, delete, install or remove packages, power off, send anything, change network or accounts | confirmed every time, with the exact action shown |

Always: a visible "FRIDAY is working" indicator with Stop, a kill key, a step
limit per task, and an audit log (`friday actions`) of every action taken.

## Plan, in order

1. **Desktop adapter (Linux).** Typed verbs the planner can call: windows
   (list, focus, move to workspace, close, fullscreen), workspaces, launching
   apps by desktop entry, media (MPRIS), volume and brightness, notifications
   and DND, power profile, Wi-Fi and Bluetooth. Hyprland IPC and D-Bus, no
   screen scraping. Risk class on every verb. Tested against stubbed
   `hyprctl` and D-Bus, then in the VM.
   *Built:* the adapter, and the loop as a tool the planner routes to (a goal that
   is an instruction about windows, workspaces, volume, media or the power
   profile). Checked on a real Hyprland in the VM. The Stop button
   on the answer card, the kill key (Super+Ctrl+Esc) and the audit log
   (`friday actions`) are in too.
2. **The control loop.** For a goal that needs the desktop, the model picks
   the next typed action, FRIDAY runs it, observes (window list, optionally a
   screenshot with consent), and repeats until done or the step limit. The
   consent classes above, the indicator and Stop, the audit log.
3. **Inside apps.** AT-SPI (read the interface tree, press buttons, type) for
   GTK, Qt and Electron apps that expose it; the browser organ (CDP) already
   exists; per-app D-Bus where an app offers it.
   *Built (adapter):* `ui` verbs `apps`, `tree`, `find`, `read`, `click`,
   `type_text` over AT-SPI. It reads real GTK4 and GTK3 apps (found deep under
   GTK4's unnamed wrapper panels) and presses elements that offer an action
   (GTK3, Qt, Electron: checked on pavucontrol, a tab pressed and selected).
   GTK4 buttons expose no press action at all, which the adapter says plainly;
   pressing those needs keyboard input, not built yet. Password fields are
   never read or typed into; a button that could lose or send something
   (Delete, Send, Pay, Install, Allow...) is refused unless the caller has
   confirmed it. Wired into the control loop as `ui_*` actions: a click is resolved first,
   so the consent decision uses the button's own label and the confirmation names it.
   A second engine, [agent-desktop](https://github.com/vercel-labs/agent-desktop) (Apache-2.0,
   accessibility trees on Linux, Windows and macOS), sits behind the same seam: used when a
   supported 1.x binary is found, with our AT-SPI adapter as the fallback. Compact snapshots
   (about 6 KB for Nautilus) instead of long paths; it presses what the app lets it and fails
   closed otherwise (GTK4 rows still can't be pressed on Hyprland). Not packaged yet.
4. **The system helper.** `friday-os-helper`: root, a D-Bus API, polkit prompt
   per sensitive call. Install and remove packages, enable services, change
   network settings, update the system ("update everything and tell me").
   *Built (service, not yet enabled anywhere):* `friday-os-helper`, a root service
   on the system bus (`org.friday.Helper1`) with fixed methods only: update the
   system, install a package, remove a package, read a job. Each call is checked
   three ways: the caller must be FRIDAY's daemon (by the program behind the bus
   connection), polkit must approve (the prompt names the package, and only an
   active local session can be asked), and the arguments must pass (valid name,
   in the repos, never a protected package). It runs `/usr/bin/pacman` with a
   fixed argument list, one job at a time, with timeouts. The files ship
   inert under `packaging/linux/helper/`; nothing starts it until an explicit
   opt-in, which is not built and not enabled in the ISO yet.
5. **Standing tasks.** "Every morning...", "when the battery drops below 20%...":
   list, pause and delete them from the FRIDAY drawer; each runs under the
   same consent classes.
6. **Pocketdesk on FRIDAY OS.** [Pocketdesk](https://github.com/sreevarshan-xenoz/Pocketdesk)
   (the phone as a screen, keyboard and remote for this computer, KDE Connect
   features, no account) ships with FRIDAY OS: packaged into the ISO and the
   pacman repo, paired in the first-login tour. Then it joins FRIDAY:
   confirmations for gated actions can be answered on the phone
   (`pocketdesk ask`), and a phone deck button can ask FRIDAY.
7. **Checks for all of it.** Scripted scenarios in the VM per verb, an e2e where
   the fake model drives the loop, and an owner pass on real hardware.

Windows and plain Linux get the same adapter interface with what each
platform exposes (UI Automation on Windows; no helper, no standing tasks on
Linux without FRIDAY OS's pieces) as those steps land.
