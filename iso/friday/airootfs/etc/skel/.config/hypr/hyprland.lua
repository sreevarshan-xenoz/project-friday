-- Learn how to configure Hyprland: https://wiki.hypr.land/Configuring/Start/

-- Omarchy's bootstrap keeps path setup out of this user config.
dofile((os.getenv("OMARCHY_PATH") or "/usr/share/omarchy") .. "/default/hypr/bootstrap.lua")

-- FRIDAY OS has its own keymap (hypr/bindings.lua), so Omarchy's default
-- bindings are off. Delete this line to get them back.
omarchy_default_bindings = false
--
-- Or disable only bindings for Omarchy's preinstalled apps/web apps while
-- keeping core window-manager bindings:
-- omarchy_preinstalled_bindings = false

-- Load Omarchy defaults.
require("default.hypr.omarchy")

-- Put your personal overrides in these files. They're loaded after Omarchy's
-- defaults so package updates can improve the defaults without rewriting your
-- ~/.config/hypr files.
require("hypr.monitors")
require("hypr.input")
require("hypr.bindings")
require("hypr.looknfeel")
-- FRIDAY Shell's look for windows (gaps, rounding, border colours), written
-- by `friday shell style|palette` and the shell's Look picker. dofile, not
-- require, so a `hyprctl reload` picks up a change.
local friday_look = (os.getenv("HOME") or "") .. "/.config/friday/hypr-look.lua"
local friday_look_file = io.open(friday_look, "r")
if friday_look_file then
  friday_look_file:close()
  dofile(friday_look)
end
require("hypr.autostart")

-- Toggle config flags dynamically.
require("default.hypr.toggles")

-- Add any other personal Hyprland configuration below.
-- o.window("qemu", { workspace = "5" })
