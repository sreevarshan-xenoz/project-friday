-- FRIDAY keybindings for a Lua Hyprland config. Load from hyprland.lua:
--   dofile("/usr/share/friday/hyprland/friday.lua")
-- (Same keys as FRIDAY OS.)
hl.bind("SUPER + Y", hl.dsp.exec_cmd("friday ask"), { description = "FRIDAY: Ask" })
hl.bind("SUPER + ALT + Y", hl.dsp.exec_cmd("friday ask --selection"), { description = "FRIDAY: Ask about selected text" })
hl.bind("SUPER + CTRL + Y", hl.dsp.exec_cmd("friday ask --screen"), { description = "FRIDAY: Ask about the screen" })
