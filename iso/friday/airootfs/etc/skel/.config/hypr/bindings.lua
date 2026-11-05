-- Keep only your personal keybinding overrides here. Add new bindings or
-- unbind defaults before replacing them.

-- FRIDAY (same keys as the Dots-Hyprland desktop).
o.bind("SUPER + Y", "FRIDAY: Ask", "friday ask")
o.bind("SUPER + ALT + Y", "FRIDAY: Ask about selected text", "friday ask --selection")
o.bind("SUPER + CTRL + Y", "FRIDAY: Ask about the screen", "friday ask --screen")

-- FRIDAY Shell drawers. Omarchy's keys for its bar panels and notifications
-- now open the matching FRIDAY Shell drawer instead.
o.bind("SUPER + A", "FRIDAY: Open FRIDAY", "friday shell friday")
o.bind("SUPER + N", "FRIDAY Shell: Controls and notifications", "friday shell control")
hl.unbind("SUPER + ALT + SPACE")
o.bind("SUPER + ALT + SPACE", "FRIDAY Shell: Apps", "friday shell launcher")
hl.unbind("SUPER + ESCAPE")
o.bind("SUPER + ESCAPE", "FRIDAY Shell: Session", "friday shell session")
for _, keys in ipairs({ "SUPER + CTRL + A", "SUPER + CTRL + B", "SUPER + CTRL + W" }) do
  hl.unbind(keys)
  o.bind(keys, "FRIDAY Shell: Controls", "friday shell control")
end
hl.unbind("SUPER + CTRL + ALT + D")
o.bind("SUPER + CTRL + ALT + D", "FRIDAY Shell: Calendar", "friday shell calendar")
hl.unbind("SUPER + CTRL + P")
o.bind("SUPER + CTRL + P", "FRIDAY Shell: Session", "friday shell session")
hl.unbind("SUPER + comma")
o.bind("SUPER + comma", "Dismiss last notification", "friday shell ipc notifications dismissOne")
hl.unbind("SUPER + SHIFT + comma")
o.bind("SUPER + SHIFT + comma", "Dismiss all notifications", "friday shell ipc notifications dismissAll")
hl.unbind("SUPER + ALT + comma")
o.bind("SUPER + ALT + comma", "Invoke last notification", "friday shell ipc notifications invokeLast")
hl.unbind("SUPER + SHIFT + ALT + comma")
o.bind("SUPER + SHIFT + ALT + comma", "Open notification history", "friday shell control")
hl.unbind("SUPER + CTRL + comma")
o.bind("SUPER + CTRL + comma", "Toggle do not disturb", "friday shell ipc notifications toggleDnd")

-- See current bindings and descriptions:
--   omarchy menu keybindings --print

-- To disable every Omarchy default binding, set this in
-- ~/.config/hypr/hyprland.lua before require("default.hypr.omarchy"), then add
-- only the bindings you want below:
--   omarchy_default_bindings = false

-- To disable all preinstalled app/webapp bindings, set:
--   omarchy_preinstalled_bindings = false

-- Add a new binding.
-- o.bind("SUPER + SHIFT + R", "SSH", "alacritty -e ssh your-server")

-- Change an existing binding by unbinding it first, then binding the key again.
-- This example changes SUPER+SPACE from the launcher to the Omarchy root menu.
-- hl.unbind("SUPER + SPACE")
-- o.bind("SUPER + SPACE", "Omarchy menu", "omarchy-menu toggle root")

-- Disable a default binding without replacing it.
-- hl.unbind("SUPER + SHIFT + B")

-- Logitech MX Keys examples:
-- o.bind("SUPER + SHIFT + S", nil, "omarchy-capture-screenshot")
-- o.bind("SUPER + H", nil, "voxtype record toggle")
-- o.bind("SUPER + PERIOD", nil, "omarchy-shell shell toggle omarchy.emojis")
