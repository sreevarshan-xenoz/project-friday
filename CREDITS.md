# Credits

FRIDAY builds on other people's open-source work. Code ported into FRIDAY
keeps its upstream copyright header, and each port is listed here: the
project, its license, the files taken and the upstream commit.

## Ported into FRIDAY

Ideas (look and behaviour, written fresh for FRIDAY Shell):

| Source | License | What FRIDAY took |
|---|---|---|
| [caelestia-dots/shell](https://github.com/caelestia-dots/shell), [snowarch/iNiR](https://github.com/snowarch/iNiR), [end-4/dots-hyprland](https://github.com/end-4/dots-hyprland) | GPL-3.0 | the rail bar style: a slim column down the left edge, drawers beside it (`bar/RailBar.qml`) |
| [end-4/dots-hyprland](https://github.com/end-4/dots-hyprland) | GPL-3.0 | the bar's small tools: snip, colour picker, microphone (an idea, `bar/ToolButtons.qml`) |
| [Gakuseei/Ricelin](https://github.com/Gakuseei/Ricelin) | MIT | the keystone bar style: everything in one notch at the top's middle (`bar/Bar.qml`) |
| [end-4/dots-hyprland](https://github.com/end-4/dots-hyprland) | GPL-3.0 | CPU and memory readouts in the bar, warning lines at 90% and 95% (`bar/ResourcesChip.qml`, switch `bar-resources`) |
| [end-4/dots-hyprland](https://github.com/end-4/dots-hyprland) | GPL-3.0 | app icons in the workspaces (`bar/Workspaces.qml`, Look > Workspace icons) |
| [end-4/dots-hyprland](https://github.com/end-4/dots-hyprland) | GPL-3.0 | the dock: pinned and running apps, up when the pointer reaches the bottom edge (`desktop/Dock.qml`) |
| [Deepender25/Edge-Drop](https://github.com/Deepender25/Edge-Drop) | Apache-2.0 | the shelf on the screen edge: drop files or text to keep them (`desktop/Shelf.qml`) |

## Shipped alongside FRIDAY (unchanged, under their own licenses)

| Project | License | Where |
|---|---|---|
| [end-4/dots-hyprland](https://github.com/end-4/dots-hyprland) | GPL-3.0 | FRIDAY OS installer option, `iso/friday/airootfs/usr/share/friday/dots-hyprland` |
| [Omarchy](https://github.com/basecamp/omarchy) | MIT | FRIDAY OS's Hyprland base, pinned in `iso/friday/fetch-omarchy.sh` |
| [agent-desktop](https://github.com/vercel-labs/agent-desktop) | Apache-2.0 | `friday-git` and the MSI (`agent-desktop-LICENSE` beside it) |
| [obscura](https://github.com/h4ckf0r0day/obscura) | Apache-2.0 | downloaded on request (`friday obscura install`), a pinned release in `packaging/tools/tools.json` |
| [anydoc](https://github.com/firecrawl/anydoc) | MIT | linked into friday-ctl and the daemon: documents read as text (`@file`, chosen folders) |
| [llmfit](https://github.com/AlexsJones/llmfit) | MIT | downloaded on request (`friday llmfit install`), a pinned release in `packaging/tools/tools.json`; behind `friday local-ai fit` |
| [tiny-voca](https://github.com/n4ze3m/tiny-voca), [dizione](https://github.com/n4ze3m/dizione) | Apache-2.0 | voice (`services/mobile/src/tinyvoca`, see its NOTICE) |
