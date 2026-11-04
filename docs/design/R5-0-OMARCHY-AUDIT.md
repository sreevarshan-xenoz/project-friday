# R5-0 OMARCHY AUDIT — the frozen architectural reference for FRIDAY OS v1

> Read-only audit of the Omarchy checkout, 2026-08-29. Scope: **R5-0A through
> R5-0E** — a survey-only archaeology of `E:\project-friday\omarchy` with the
> goal of drawing the exact boundary between the Omarchy substrate and the
> FRIDAY product we build on top of it.
>
> Method: sequential deep audits of the major architecture surfaces — repo
> map (R5-0A-intro), `shell/` (R5-0A), `bin/` + dependency graph (R5-0B),
> `install/` + `default/` + `etc/` (R5-0C), `themes/` + branding (R5-0D) — then
> a consolidated classification and ISO/packaging disposition (R5-0E).

> **The Omarchy checkout at `0b3f1b7ead00ac4bcbaae8bac16bab3f7efbe516` is a
> frozen upstream reference. R5 changes nothing inside it.**

That boundary matters more than anything else in this document. From R5-1
onward we make actual changes, and we must always be able to tell which
engineering came from upstream (Omarchy-derived machinery) versus which is
FRIDAY-owned. The Omarchy checkout stays a clean, pinned, read-only reference.

---

## Headline finding

**We are not rebuilding an OS. We are adopting Omarchy as FRIDAY's Linux
substrate and replacing the identity/packaging layer around it.**

Omarchy is a complete, battle-tested, self-hosting desktop Linux distribution:
KEEP ~all of it as the substrate. FRIDAY inserts at the plugin/IPC/state seams
and supplies the product — the runtime, the shell plugin that is the FRIDAY
desktop UX, the themes, and the identity. We inherit the boring-but-critical
OS engineering (boot, recovery, package management, hardware setup, Hyprland,
networking, audio, firewall, updates + migrations, themes, system
configuration) and spend our effort on the thing that makes FRIDAY FRIDAY.

```text
                 FRIDAY OS  (the product)
                    │
       ┌────────────┴────────────┐
       │                         │
  FRIDAY identity          FRIDAY runtime   <- FRIDAY-OWNED
       │                         │
  ├─ wordmark              ├─ Rust cognitive core
  ├─ logomark              ├─ autonomy / memory / skills
  ├─ themes                ├─ agents / daemon
  ├─ fonts                 └─ voice / vision / web
  ├─ shell plugin  (friday.shell — the desktop UX)
  └─ defaults
       │                         │
       └────────────┬────────────┘
                    │
             OMARCHY SUBSTRATE    <- KEEP (frozen 0b3f1b7)
                    │
       ├─ boot / Limine · Plymouth/SDDM machinery
       ├─ pacman / update / migrations / snapshots / channels
       ├─ hardware setup · Hyprland · systemd · networking
       ├─ firewall · recovery · theme engine · bin suite
       └─ shell substrate (IPC + plugins + bar)   <- integration seam
```

**FRIDAY is the product; Omarchy is the implementation substrate.**

---

## Do Not Do — the guardrail

These become our protection against `sed -i 's/omarchy/friday/g'` enthusiasm.
They are architectural facts, not preferences. Changing any of them during the
substrate phase is catastrophic.

```text
DO NOT:
- globally rename omarchy-* binaries
- rename OMARCHY_PATH
- move ~/.config/omarchy
- move ~/.local/state/omarchy
- rewrite the Quickshell architecture
- recreate the theme engine
- rebuild Omarchy's installer from scratch
- fork/rebuild omarchy-pkgs before its role is decided
- mix R4 qualification changes into the Omarchy tree
```

The first six are silent landmines because of the 1,255 intra-`bin/`
references plus ~310 `OMARCHY_PATH` references, the stateful migration
idempotency markers in `~/.local/state/omarchy` and `/var/lib/omarchy`, and
the runtime coupling of channel/version detection to binary + package names.

---

## Execution record

| Id | Surface | Result | Summary |
|---|---|---|---|
| R5-0A-intro | Repo architecture map | 🟢 | 1,705 files across 15 top-level dirs |
| R5-0A | `shell/` | 🟢 | Integration-seam precedent (IPC + plugins + file-state) |
| R5-0B | `bin/` + dependency graph | 🟢 | Router mechanics, 1,255-edge dependency map |
| R5-0C | `install/` + `default/` + `etc/` | 🟢 | Packaging + bring-up + hardening + update pipeline |
| R5-0D | `themes/` + branding | 🟢 | Theme engine value + wordmark/logomark split |
| R5-0E | Consolidated architecture + ISO | 🟢 | KEEP/ADAPT/FRIDAY/REBRAND/REPLACE + `omarchy-pkgs` open |

**All audit surfaces are complete and the classification is frozen as of this
document.**

---

## R5-0A — `shell/`

The shell is a single long-running Quickshell `ShellRoot` instance that already
exposes a **clean interface to an external FRIDAY runtime** via three seams:

1. **IPC transport** — `qs ipc -p $OMARCHY_PATH/shell call <target> <method> <args>` (wrapper `bin/omarchy-shell:59`).
2. **Plugin system** — drop-in QML at `~/.config/omarchy/plugins/<id>/`, discovered by bash scan, validated against path-traversal, hot-reloaded via inotifywait.
3. **File-backed state** — daemon writes JSON, plugin watches via FileView/inotify.

The **`agents` plugin** (`shell/plugins/agents/`) is the working precedent:
external CLI writes JSON to `~/.local/state/omarchy/agents/usage/`, bar-widget
plugin renders it and registers its own IPC target `omarchy.agents`. This is
exactly the pattern the FRIDAY runtime will follow.

Key mechanics: property injection not singletons (`shell.qml:14-20`),
`ensureService`/`_syncServices` (`shell.qml:283-346`), `shell` IPC handler
(`shell.qml:872-1030`), `PluginRegistry.qml` (validate/scan/load/hot-reload),
shell-notifications owns an in-process FreeDesktop D-Bus NotificationServer
(`Service.qml:931-943`), OSD (`omarchy.osd` with own `osd` IPC target), menu
via `omarchy-menu` → `shell toggle omarchy.menu`, one config file
`~/.config/omarchy/shell.json`, theming via `Color`/`Style` prologue singletons
+ `omarchy-theme-set` → IPC `shell applyTheme` → `Style.scheduleRefresh`.

---

## R5-0B — `bin/`

The router `bin/omarchy` (1,092 lines) is **metadata-driven, single-dispatch**:
discovery by filename glob, routes from `# omarchy:group=`/`# omarchy:name=`
header metadata (fallback: filename stem), `exec`s the real binary. Metadata
keys: group/name/summary/args/examples/alias/requires-sudo/hidden.

Dependency map (1,255 intra-bin reference lines across 440 binaries):

- Fan-in leaders: `omarchy-notification-send` (45 callers), `omarchy-shell` (38), `omarchy-pkg-add` (37), `omarchy-cmd-present` (35), `omarchy-pkg-drop` (22).
- 162 pure leaf binaries.
- Cross-surface blast radius: `test/` 1,551, `default/` 430, `shell/` 209, `migrations/` 179, `install/` 113, `etc/` 8, `applications/` 13.

Four reference kinds:

1. **Binary-name invocations** — break on rename.
2. **Router-route invocations** (`omarchy <group> <name>`) — decoupled via metadata.
3. **Stateful path markers** — `~/.config/omarchy` (75 refs), `~/.local/state/omarchy` (48), `/var/lib/omarchy` (12), `/usr/share/omarchy` (36), `OMARCHY_PATH` (310 refs). The silent landmines, especially migration idempotency markers.
4. **Branding strings**.

**Recommendation (carried forward):** rename the router + user-facing routes
first if ever needed; do NOT rename the 1,255-edge binary network in one shot;
keep stateful paths stable.

---

## R5-0C — `install/` + `default/` + `etc/`

### The pivotal finding: this repo does not build an ISO by itself

Per `docs/file-layout.md:8-9`, two Arch packages are built from this one repo,
but the **PKGBUILDs live in a separate, non-vendored `omarchy-pkgs`
repository** (under `pkgbuilds/`), and the **ISO builder / archiso / pacstrap
materialization is also external**. This repo is the *content* — commands,
configs, package lists, migrations, docs — that a separate packaging + ISO
layer turns into an installable OS.

The two packages built from this repo, split by `useradd -m`:

- **`omarchy-settings`** — everything before user creation: `/etc/skel/**`, `/etc` drop-ins, fonts, plymouth, sddm theme, branding, limine/mkinitcpio/snapper configs. Installed first.
- **`omarchy`** — runtime: `bin/`, `install/`, `migrations/`, `themes/`, `shell/`, `version`.
- Standalone: `omarchy-keyring` (pacman GPG), `omarchy-nvim`.

**Three-layer `$HOME` population:**
1. **Seed** — `/etc/skel` copied by `useradd -m`.
2. **Finalize** — `omarchy-provision-user` (routed `omarchy finalize user`) once per user.
3. **Resync** — `omarchy-reinstall-configs`, explicit destructive clobber.

`OMARCHY_PATH = /usr/share/omarchy` on production; `default/bash/env-bootstrap`
is the single source of truth (plus dev-link via `/etc/omarchy.conf`).
`etc-overrides` ships Arch-owned files via `post_install` `cp -f`.

**Install orchestration** (`install/config/all.sh`, `install/hardware/all.sh`
via `run_logged` → `/var/log/omarchy-install.log`): config (theme-system,
browser-policy, lockout, lockscreen-PAM, ssh, docker, snapper, locate,
service-enable, firewall), hardware (vendor/device quirks: asus, framework16,
intel, apple/T2, lenovo, surface, bluetooth, nvidia→vulkan, regdom, speaker),
login (sddm), post-install. First-run (`omarchy-provision-first-run`) enables
6 session units + welcome + wifi toasts. Deferred provisioning via
`omarchy-provision-owner.service` on tty1.

**Update/migration pipeline** (`omarchy-update`): free-space gate → prime →
snapshot → stay-awake → keyring/system-pkgs → **migrate** → `hook
post-update` → analyze → restart. Update-lock per-user flock; channels
stable/rc/edge/dev; pacman-guard PreTransaction hook aborts bare `pacman
-Syu` forcing `omarchy update`; per-user migration markers in
`~/.local/state/omarchy/migrations/`, login-triggered, never background.

**`etc/` hardening posture:** UFW default-deny + LocalSend 53317 + Docker-DNS;
narrowly-scoped NOPASSWD sudo (deliberately no blanket passwordless);
faillock deny=10; oomd; `HandlePowerKey=ignore`; 15s inhibit delay; zswap off
(zram double-compress focus); cups-browsed unprivileged with
`ProtectSystem=strict`; Docker not sudoless unless opted in.

---

## R5-0D — `themes/` + branding

### The theme engine is the single cleanest reuse seam in the entire codebase

22 themes, each shipping: `colors.toml` (semantic palette), optional per-app
files (`neovim.lua`, `vscode.json`, `icons.theme`, `btop.theme`,
`chromium.theme`, `helix.toml`, `keyboard.rgb`), `preview*.png`, and
`backgrounds/*.webp`.

`colors.toml` schema: `mode` (dark/light) + semantic accents
(`accent/selection/muted`) + background ramp (`background/dark_background/
darker_background/lighter_background`) + foreground ramp + 8 named ANSI colors
+ bright variants (`omarchy-theme-color:176-281`). Note: **no `urgent` key**
(the shell's urgent role = `red`); missing shades auto-derived by mixing.

`default/themed/*.tpl` (17 templates, incl. `shell.toml.tpl` — the master for
all shell surfaces: `[bar]/[hyprland]/[controls]/[spacing]/[font]/[popups]/
[tooltip]/[notifications]/[launcher]/[menu]/[polkit]/[lock]/[image-picker]`).
Placeholders: `{{ key }}`, `{{ key_strip }}`, `{{ key_rgb }}`, plus
`{{ mix a b % }}`, `{{ hypr_gradient }}`, `{{ shell_gradient }}`.
`shell.<section>.toml` overrides one generated section only. Existing files in
output never overwritten (hand-written theme wins).

**Engine flow** (`omarchy theme set <name>`): resolve → flock → stage into
`~/.local/state/omarchy/current/next-theme` → **denylist** drops run-code
files from git-cloned themes (`alacritty.toml foot.ini ghostty.conf kitty.conf
vscode.json` + any `*.lua`) → render templates → swap to current → **base64
IPC push** (`shell applyTheme <colorsB64> <shellB64>` →
`Color.loadColors`/`loadShell` → `Style.scheduleRefresh`, `shell.qml:879-888`)
→ background transition IPCs → `omarchy-bar-text-color` WCAG contrast →
post-theme hooks in parallel (foot OSC, tmux, gnome, vscode, obsidian,
browser, keyboard RGB) → `omarchy-hook theme-set` → cache warm.

**Fonts are system-wide, NOT theme-owned.** `[font] base-size` scales the
whole type tree; family = fontconfig `monospace` alias. Branded icon font
`default/fonts/omarchy/omarchy.ttf` carries the logo as private-use glyph
`U+E900` (+ Pi/OpenCode/omp/Grok/Codex/LM Studio/Ollama/T3 code/Ori
U+E901-909), installed to `/usr/share/fonts/omarchy/omarchy.ttf`.

### Two identities — wordmark vs logomark

* **Wordmark** — `logo.svg`/`logo.txt`/`logo.png` (800×188 banner), "OMARCHY"
  in Delta Corps Priest 1 block/FIGlet style, monochrome. Used at boot, login,
  screensaver, limine.
* **Logomark** — `icon.png`/`icon.txt` (300×300 square ring). About default,
  app icon, menu glyph `U+E900`.

Boot (Plymouth) and login (SDDM) share the same logo + colors and are
re-synced together by `omarchy-plymouth-set` (change one, the other follows).
About = fastfetch TUI with green logo + `"Omarchy $version"` hardcoded OS line
(`etc/fastfetch/config.jsonc:74`); screensaver = `ttfx` animated ASCII. Both
window-classed `org.omarchy.*`.

**Coupling classification:**

| Class | Surfaces |
|---|---|
| 🟢 swappable skin (static assets) | root `logo.*`/`icon.*`, window PNGs, `omarchy-ascii`, OSD, menu "About"/"Style" items, Discord app icon |
| 🔵 coupled but separable | `omarchy.ttf` + glyph `U+E900`, About/Screensaver content + `org.omarchy.*` classes, `omarchy-show-logo`, limine `interface_branding` |
| 🔴 hardcoded wordmark strings | welcome text, fastfetch `"Omarchy $version"`, `update.omarchy`/`learn.omarchy` menu labels, wayland-session name, Plymouth/SDDM metadata, channel/version (binary + package names + `omarchy.org` URLs) |

FRIDAY identity → wordmark replaces wordmark wherever the user sees the OS
identity; logomark replaces the ring/icon; icon-font mechanism carried forward
with replaced glyph assets; Plymouth+SDDM are a FRIDAY skin over existing
machinery.

---

## R5-0E — Consolidated classification (FROZEN)

### 1. 🟢 KEEP — inherit untouched (the substrate)

| Surface | Evidence |
|---|---|
| Router + bin command suite (`bin/omarchy` + ~440 binaries) | R5-0B |
| Shell substrate (Quickshell `ShellRoot`, IPC, PluginRegistry, bar/panel/clock/polkit, Common/Ui, OSD) | R5-0A |
| Theme engine (colors.toml → base64 IPC → hot-reload + templates + post-hooks) | R5-0D |
| Install/bring-up (`install/config/hardware/login/post-install/all.sh`) | R5-0C |
| System hardening (`etc/`: firewall, faillock, oomd, logind, zram/sysctl) | R5-0C |
| Update/migration/snapshot machinery | R5-0C |
| Hyprland/session/dotfiles seed (`default/`) | R5-0C |
| Agent skills, `manual/`, `docs/` (as substrate reference) | repo map |

Internal names/paths stay Omarchy during the substrate phase: `omarchy-*`
binaries, `OMARCHY_PATH`, `~/.config/omarchy`, `~/.local/state/omarchy`,
`/usr/share/omarchy`, `omarchy.*` IPC namespace.

### 2. 🔵 ADAPT — integration points

* **FRIDAY shell plugin** (`friday.shell` plugin set) — new drop-in QML under
  `~/.config/omarchy/plugins/`, own `friday.*` IPC namespace. **This is where
  the FRIDAY desktop experience is actually built.**
* Package lists (`omarchy-base.packages` → add FRIDAY runtime deps) + new
  FRIDAY session systemd unit.
* First-run hooks → FRIDAY onboarding.
* Themed templates stay (17 `.tpl`); FRIDAY themes flow through the engine.
* Firewall/sudo/udev rules → add FRIDAY-specific rules.

### 3. 🟣 FRIDAY-OWNED — what `project-friday` supplies

* **FRIDAY runtime** — the existing Rust cognitive core, memory, autonomy,
  skills, agents, voice/vision/web (packaged alongside the substrate).
* **FRIDAY shell plugin** (`friday.shell`) + IPC/state contract.
* **FRIDAY daemon** + FRIDAY session units.
* **FRIDAY themes** — first-class themes via the engine.
* **FRIDAY identity** — wordmark, logomark, icon font glyphs.
* **FRIDAY onboarding** — new first-run provisioning.
* **FRIDAY keyring** (if we diverge from omarchy-keyring).

### 4. 🔴 REBRAND — what the user sees as FRIDAY

Wordmark, logomark, Plymouth+SDDM skin, About `"FRIDAY $version"` + ASCII,
screensaver, welcome text, menu labels, wayland-session name, icon font
glyphs. The **verbal wordmark strings** (limine, fastfetch OS line, welcome,
menu labels, session name, Plymouth/SDDM metadata) are a distinct REBRAND
surface — hardcoded text, not asset-derivable.

### 5. 🔴 REPLACE LATER — only on a real technical reason

* Internal binary/route names (`omarchy-*`) — the 1,255-edge blast radius.
* Stateful paths (`~/.config/omarchy`, `~/.local/state/omarchy`,
  `/usr/share/omarchy`, migration markers) — silent landmines.
* `OMARCHY_PATH` — only via deliberate migration.
* Channel/version detection — packaging-boundary coupling.

---

## R5-0E — the six sub-questions

### A. Exact Omarchy dependency boundary

One artifact frozen at `0b3f1b7`. Internal seams stay stable. FRIDAY touches
the substrate at exactly **three contract points**, all proven by the `agents`
plugin precedent:
1. **IPC** — `qs ipc -p $OMARCHY_PATH/shell call <target> <method> <args>` (`bin/omarchy-shell:59`).
2. **Plugin drop-in** — QML dir scanned/validated/hot-reloaded by PluginRegistry.
3. **File-backed state** — daemon writes JSON, plugin watches via FileView/inotify.

### B. Exact FRIDAY integration seams

FRIDAY inserts at the **plugins layer**, not inside the substrate. FRIDAY
runtime (Rust) ↔ shell via the same three contract points.

### C. Package architecture

Two content packages (`omarchy`, `omarchy-settings`) + `omarchy-keyring` +
`omarchy-nvim`, built in the external `omarchy-pkgs` repo. FRIDAY needs: the
content tree (wholesale) + a FRIDAY runtime package + FRIDAY identity assets.
The split sits exactly on the REBRAND layer.

### D. Standalone ISO architecture

The repo does not build an ISO. It's content the external packaging+ISO layer
turns into an installable OS. Standalone-reproducible FRIDAY ISO = FRIDAY-owned
packaging + ISO layer (see G).

### E. Update/migration strategy

Wholesale inherit the pacman-guard → snapshot → migrate → hook pipeline.
FRIDAY migrations ride the same `migrations/*.sh` marker system, **adding**
FRIDAY-specific migrations alongside — never rewriting the marker format.

### F. First-boot strategy

Three-layer `$HOME` population (Seed `/etc/skel` → Finalize
`omarchy-provision-user` → Resync) + first-run graphical hooks. FRIDAY
onboarding is a **new first-run hook not a replacement**.

### G. `omarchy-pkgs` decision — EXPLICITLY OPEN

Not decided. Three options, deliberately held:
- **(i) Fork `omarchy-pkgs`** — rebrand packages, keep the proven build/offline-mirror/materialize layer. Low risk, keeps the upstream dependency.
- **(ii) Own fresh packaging** consuming this content tree + FRIDAY runtime — full independence, but rebuilds the BS of ISO materialization.
- **(iii) Hybrid** — fork now for substrate continuity, plan a FRIDAY-native packaging layer later (matches the REBRAND-later rule).

**No fork/rebuild happens before this role is decided.**

---

## The standalone-ISO answer

The "minimum set of Omarchy components needed for a reproducible standalone
FRIDAY ISO" has two parts:

1. **The content we already have, frozen-and-reusable:** `bin/` + `install/` +
   `default/` + `etc/` + `migrations/` + `themes/` + `shell/` + `config/` +
   `applications/` — the entire OS substrate, a single artifact you already own
   at `0b3f1b7`. FRIDAY inherits ~all of it.
2. **What is NOT in this repo and must be handled:** `omarchy-pkgs` (the four
   PKGBUILDs + the ISO/build/materialize + offline-mirror), the base Arch
   layer, and the mirror. The content side is fully reusable; **the packaging
   side is the real new-work frontier.**

---

## Frozen decision record

- **FRIDAY = product; Omarchy = substrate.**
- Internal Omarchy names remain untouched for now.
- FRIDAY integrates through the existing **IPC + plugin + file-state** seams.
- Branding is separated from implementation identity.
- `omarchy-pkgs` remains an explicit open decision, not prematurely chosen.
- No ISO work starts until this audit is the frozen architectural reference.
- The Omarchy checkout stays pinned, clean, read-only.

---

## Next sequence

```text
R5-0A  shell audit                 ✅
R5-0B  bin audit                   ✅
R5-0C  install/default/etc         ✅
R5-0D  themes/branding             ✅
R5-0E  consolidated architecture   ✅
        │
        ▼
R5-0F  write R5-0-OMARCHY-AUDIT.md ✅  (this document)
        │
        ▼
        FREEZE
        │
        ▼
R5-1  FRIDAY OS v1 construction
        Phase 1: Omarchy + FRIDAY runtime + FRIDAY daemon
                 + friday.shell plugin + IPC/state contract
```

R5-1 begins with **Phase 1 integration, not the ISO**: Omarchy + FRIDAY
runtime + FRIDAY daemon + `friday.shell` plugin + the IPC/state contract.
Once that actually runs on the Omarchy environment, **then** the packaging/ISO
layer is tackled.
