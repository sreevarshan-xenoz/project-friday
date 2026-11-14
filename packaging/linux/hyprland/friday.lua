-- FRIDAY keybindings for a Lua Hyprland config. Load from hyprland.lua:
--   dofile("/usr/share/friday/hyprland/friday.lua")
-- (Same keys as FRIDAY OS.)
-- Tap Super+Y to type a question; hold it and speak (friday voice setup).
hl.bind("SUPER + Y", hl.dsp.exec_cmd("friday hold start"), { description = "FRIDAY: Ask" })
hl.bind("SUPER + Y", hl.dsp.exec_cmd("friday hold end"), { description = "FRIDAY: Ask (let go)", release = true })
hl.bind("SUPER + ALT + Y", hl.dsp.exec_cmd("friday ask --selection"), { description = "FRIDAY: Ask about selected text" })
hl.bind("SUPER + CTRL + Y", hl.dsp.exec_cmd("friday ask --screen"), { description = "FRIDAY: Ask about the screen" })
