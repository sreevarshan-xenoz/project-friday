-- Extra autostart processes.
-- o.launch_on_start("my-service")

-- FRIDAY Shell: the bar, drawers, notifications and OSD. Omarchy's shell
-- keeps running for the lock screen, idle, polkit, menus and pickers; its own
-- bar, notifications and OSD are switched off in ~/.config/omarchy/shell.json.
o.exec_on_start("friday shell")

-- FRIDAY: one-time first-login greeting (offers AI setup if no key yet).
o.exec_on_start("friday welcome")
