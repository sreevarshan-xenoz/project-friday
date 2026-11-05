# friday.bar

On FRIDAY OS, FRIDAY Shell (`packaging/linux/shell`, started with
`friday shell`) draws the bar, notifications and OSD. The Omarchy shell keeps
running for what FRIDAY Shell doesn't do: the lock screen, idle handling, the
polkit agent, menus and pickers.

Selecting this plugin as the bar (`"bar": {"id": "friday.bar"}` in
`~/.config/omarchy/shell.json`) turns Omarchy's bar off without leaving a gap:
it draws nothing and reports FRIDAY Shell's bar height (36 px) so Omarchy's
panels open below it. The installer writes that file, also listing
`omarchy.notifications` and `omarchy.osd` in `disabledPlugins`.
