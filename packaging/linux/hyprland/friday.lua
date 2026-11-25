-- FRIDAY keybindings for a Lua Hyprland config. Load from hyprland.lua:
--   dofile("/usr/share/friday/hyprland/friday.lua")
-- (Same keys as FRIDAY OS.)
-- Tap Super+Y to type a question; hold it and speak (friday voice setup).
hl.bind("SUPER + Y", hl.dsp.exec_cmd("friday hold start"), { description = "FRIDAY: Ask" })
hl.bind("SUPER + Y", hl.dsp.exec_cmd("friday hold end"), { description = "FRIDAY: Ask (let go)", release = true })
-- Voice typing: hold Super+Shift+Y and speak; the words are typed where you are (nothing is asked).
hl.bind("SUPER + SHIFT + Y", hl.dsp.exec_cmd("friday hold start --type"), { description = "FRIDAY: Voice typing" })
hl.bind("SUPER + SHIFT + Y", hl.dsp.exec_cmd("friday hold end --type"), { description = "FRIDAY: Voice typing (let go)", release = true })
hl.bind("SUPER + ALT + Y", hl.dsp.exec_cmd("friday ask --selection"), { description = "FRIDAY: Ask about selected text" })
-- Paste the clipboard transformed (bullets, JSON, shorter, translated…).
hl.bind("SUPER + ALT + V", hl.dsp.exec_cmd("friday paste-as"), { description = "FRIDAY: Paste as…" })
hl.bind("SUPER + CTRL + V", hl.dsp.exec_cmd("friday clips pick"), { description = "FRIDAY: Clips and snippets" })
hl.bind("SUPER + CTRL + Y", hl.dsp.exec_cmd("friday ask --screen"), { description = "FRIDAY: Ask about the screen" })
-- The kill key: stops whatever FRIDAY is doing on your desktop, at once.
hl.bind("SUPER + CTRL + Escape", hl.dsp.exec_cmd("friday cancel"), { description = "FRIDAY: Stop what it is doing" })
-- The panic switch: agents, voice, capture and routines off until pressed again.
hl.bind("SUPER + CTRL + SHIFT + Escape", hl.dsp.exec_cmd("friday panic toggle"), { description = "FRIDAY: Stop everything (again: back on)" })
hl.bind("SUPER + I", hl.dsp.exec_cmd("friday settings"), { description = "FRIDAY: Settings" })
-- Qt 6 and Chromium apps join the accessibility bus only when asked; without
-- it FRIDAY can't read or operate them (`friday doctor` checks).
hl.env("QT_LINUX_ACCESSIBILITY_ALWAYS_ON", "1")
hl.env("ACCESSIBILITY_ENABLED", "1")
