-- Extra autostart processes.
-- o.launch_on_start("my-service")

-- FRIDAY Shell: the bar, drawers, notifications, OSD and password prompts.
-- Omarchy's shell doesn't run (omarchy_shell = false in hyprland.lua).
o.exec_on_start("friday shell")

-- The wallpaper (friday wallpaper), and dim, lock (friday lock) and screen
-- off when idle (friday idle).
o.exec_on_start("friday wallpaper show")
o.exec_on_start("friday idle")

-- A password or key left on the clipboard is cleared after 2 minutes
-- (friday clipguard; switch: friday feature clipboard-expiry off).
o.exec_on_start("friday clipguard")

-- FRIDAY: one-time first-login greeting (offers AI setup if no key yet).
o.exec_on_start("friday welcome")
