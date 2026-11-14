-- FRIDAY OS keys. Omarchy's default bindings are off (hyprland.lua sets
-- omarchy_default_bindings = false); this file is the whole keymap, laid out
-- the end-4 way. See every binding with: omarchy menu keybindings --print

-- Hardware keys (volume, brightness, media, keyboard backlight) stay Omarchy's.
require("default.hypr.bindings.media")

-- FRIDAY
-- Tap Super+Y to type a question; hold it and speak (friday voice setup).
o.bind("SUPER + Y", "FRIDAY: Ask", "friday hold start")
o.bind("SUPER + Y", "FRIDAY: Ask (let go)", "friday hold end", { release = true })
o.bind("SUPER + ALT + Y", "FRIDAY: Ask about selected text", "friday ask --selection")
o.bind("SUPER + CTRL + Y", "FRIDAY: Ask about the screen", "friday ask --screen")
o.bind("SUPER + A", "FRIDAY: Open FRIDAY", "friday shell friday")
o.bind("SUPER + B", "FRIDAY: Open FRIDAY", "friday shell friday")
o.bind("SUPER + O", "FRIDAY: Open FRIDAY", "friday shell friday")

-- FRIDAY Shell
o.bind("SUPER + Super_L", "Apps", "friday shell launcher", { release = true })
o.bind("SUPER + N", "Controls and notifications", "friday shell control")
o.bind("SUPER + M", "Controls and notifications", "friday shell control")
o.bind("SUPER + CTRL + ALT + D", "Calendar", "friday shell calendar")
o.bind("CTRL + ALT + Delete", "Session", "friday shell session")
o.bind("XF86PowerOff", "Session", "friday shell session", { locked = true })
o.bind("SUPER + CTRL + R", "Restart FRIDAY Shell", "friday shell restart")
o.bind("SUPER + K", "Every key", "omarchy-menu-keybindings")
o.bind("SUPER + comma", "Dismiss last notification", "friday shell ipc notifications dismissOne")
o.bind("SUPER + SHIFT + comma", "Dismiss all notifications", "friday shell ipc notifications dismissAll")
o.bind("SUPER + ALT + comma", "Invoke last notification", "friday shell ipc notifications invokeLast")
o.bind("SUPER + CTRL + comma", "Toggle do not disturb", "friday shell ipc notifications toggleDnd")

-- Tools
o.bind("SUPER + V", "Clipboard history", "omarchy-shell shell toggle omarchy.clipboard")
o.bind("SUPER + period", "Emojis", "omarchy-shell shell toggle omarchy.emojis")
o.bind("SUPER + CTRL + T", "Wallpapers", "omarchy-menu toggle background")
o.bind("SUPER + SHIFT + C", "Colour picker", "pkill hyprpicker || hyprpicker -a")
o.bind("SUPER + SHIFT + S", "Screenshot", "omarchy-capture-screenshot")
o.bind("SUPER + SHIFT + R", "Screen recording", "omarchy-capture-screenrecording --stop-recording || omarchy-menu toggle trigger.capture.screenrecord")
local grab = "grim -o \"$(hyprctl activeworkspace -j | jq -r '.monitor')\""
o.bind("Print", "Copy this screen", grab .. " - | wl-copy", { locked = true })
o.bind("CTRL + Print", "Save this screen to Pictures/Screenshots",
  "mkdir -p \"$(xdg-user-dir PICTURES)/Screenshots\" && " .. grab ..
  " \"$(xdg-user-dir PICTURES)/Screenshots/Screenshot_$(date '+%Y-%m-%d_%H.%M.%S').png\"", { locked = true })

-- Zoom
local function zoom(step)
  local z = (hl.get_config("cursor:zoom_factor") or 1) + step
  hl.config({ cursor = { zoom_factor = math.max(1.0, math.min(3.0, z)) } })
end
o.bind("SUPER + minus", "Zoom out", function() zoom(-0.3) end, { repeating = true })
o.bind("SUPER + equal", "Zoom in", function() zoom(0.3) end, { repeating = true })

-- Media
o.bind("SUPER + SHIFT + N", "Next track", "playerctl next", { locked = true })
o.bind("SUPER + SHIFT + B", "Previous track", "playerctl previous", { locked = true })
o.bind("SUPER + SHIFT + P", "Play or pause", "playerctl play-pause", { locked = true })
o.bind("SUPER + SHIFT + M", "Mute", "wpctl set-mute @DEFAULT_SINK@ toggle", { locked = true })
o.bind("SUPER + ALT + M", "Mute microphone", "wpctl set-mute @DEFAULT_SOURCE@ toggle", { locked = true })

-- Windows
hl.bind("SUPER + mouse:272", hl.dsp.window.drag(), { mouse = true })
hl.bind("SUPER + mouse:274", hl.dsp.window.drag(), { mouse = true })
hl.bind("SUPER + mouse:273", hl.dsp.window.resize(), { mouse = true })
local arrows = { Left = "l", Right = "r", Up = "u", Down = "d" }
for key, dir in pairs(arrows) do
  o.bind("SUPER + " .. key, "Focus window", hl.dsp.focus({ direction = dir }))
  o.bind("SUPER + SHIFT + " .. key, "Move window", hl.dsp.window.move({ direction = dir }))
end
o.bind("SUPER + bracketleft", "Focus window on the left", hl.dsp.focus({ direction = "l" }))
o.bind("SUPER + bracketright", "Focus window on the right", hl.dsp.focus({ direction = "r" }))
o.bind("SUPER + Q", "Close window", hl.dsp.window.close())
o.bind("SUPER + SHIFT + ALT + Q", "Kill a window (click it)", "hyprctl kill")
-- Narrower / wider: the split in classic tiling, the column in scrolling tiling.
local function resize(delta)
  local cmd = hl.get_config("general.layout") == "scrolling" and "colresize " or "splitratio "
  hl.dispatch(hl.dsp.layout(cmd .. delta))
end
o.bind("SUPER + semicolon", "Narrower", function() resize("-0.1") end, { repeating = true })
o.bind("SUPER + apostrophe", "Wider", function() resize("+0.1") end, { repeating = true })
o.bind("SUPER + CTRL + Space", "Classic or scrolling tiling", "omarchy-hyprland-workspace-layout-toggle")
o.bind("SUPER + CTRL + F", "Column full width (scrolling)", hl.dsp.layout("colresize +conf"))
o.bind("SUPER + ALT + Space", "Float or tile", hl.dsp.window.float({ action = "toggle" }))
o.bind("SUPER + D", "Maximise", hl.dsp.window.fullscreen({ mode = "maximized", action = "toggle" }))
o.bind("SUPER + F", "Full screen", hl.dsp.window.fullscreen({ mode = "fullscreen", action = "toggle" }))
o.bind("SUPER + ALT + F", "Fake full screen", hl.dsp.window.fullscreen_state({ internal = 0, client = 3, action = "toggle" }))
o.bind("SUPER + P", "Pin window", hl.dsp.window.pin())

-- Workspaces
for i = 1, 10 do
  local key = tostring(i % 10)
  o.bind("SUPER + " .. key, "Workspace " .. i, hl.dsp.focus({ workspace = tostring(i) }))
  o.bind("SUPER + ALT + " .. key, "Send window to workspace " .. i, hl.dsp.window.move({ workspace = tostring(i), follow = false }))
end
o.bind("SUPER + SHIFT + mouse_up", "Move window to previous workspace", hl.dsp.window.move({ workspace = "r-1" }))
o.bind("SUPER + SHIFT + mouse_down", "Move window to next workspace", hl.dsp.window.move({ workspace = "r+1" }))
o.bind("SUPER + SHIFT + Page_Up", "Move window to previous workspace", hl.dsp.window.move({ workspace = "r-1" }))
o.bind("SUPER + SHIFT + Page_Down", "Move window to next workspace", hl.dsp.window.move({ workspace = "r+1" }))
o.bind("CTRL + SUPER + SHIFT + Left", "Move window to previous workspace", hl.dsp.window.move({ workspace = "r-1" }))
o.bind("CTRL + SUPER + SHIFT + Right", "Move window to next workspace", hl.dsp.window.move({ workspace = "r+1" }))
o.bind("CTRL + SUPER + Left", "Previous workspace", hl.dsp.focus({ workspace = "r-1" }))
o.bind("CTRL + SUPER + Right", "Next workspace", hl.dsp.focus({ workspace = "r+1" }))
o.bind("CTRL + SUPER + ALT + Left", "Previous workspace on this monitor", hl.dsp.focus({ workspace = "m-1" }))
o.bind("CTRL + SUPER + ALT + Right", "Next workspace on this monitor", hl.dsp.focus({ workspace = "m+1" }))
o.bind("SUPER + Page_Up", "Previous workspace", hl.dsp.focus({ workspace = "r-1" }))
o.bind("SUPER + Page_Down", "Next workspace", hl.dsp.focus({ workspace = "r+1" }))
o.bind("SUPER + mouse_up", "Previous workspace", hl.dsp.focus({ workspace = "-1" }))
o.bind("SUPER + mouse_down", "Next workspace", hl.dsp.focus({ workspace = "+1" }))
o.bind("CTRL + SUPER + bracketleft", "Previous workspace", hl.dsp.focus({ workspace = "-1" }))
o.bind("CTRL + SUPER + bracketright", "Next workspace", hl.dsp.focus({ workspace = "+1" }))
o.bind("CTRL + SUPER + Up", "Five workspaces back", hl.dsp.focus({ workspace = "r-5" }))
o.bind("CTRL + SUPER + Down", "Five workspaces on", hl.dsp.focus({ workspace = "r+5" }))

-- Scratchpad
o.bind("SUPER + S", "Scratchpad", hl.dsp.workspace.toggle_special("special"))
o.bind("CTRL + SUPER + S", "Scratchpad", hl.dsp.workspace.toggle_special("special"))
o.bind("SUPER + mouse:275", "Scratchpad", hl.dsp.workspace.toggle_special("special"))
o.bind("SUPER + ALT + S", "Send window to the scratchpad", hl.dsp.window.move({ workspace = "special:special", follow = false }))

-- Touchpad: 3 fingers swipe moves the window, pinch goes full screen;
-- 4 fingers sideways switches workspace.
hl.gesture({ fingers = 3, direction = "swipe", action = "move" })
hl.gesture({ fingers = 3, direction = "pinch", action = "fullscreen" })
hl.gesture({ fingers = 4, direction = "horizontal", action = "workspace" })
hl.config({
  gestures = {
    workspace_swipe_distance = 700,
    workspace_swipe_cancel_ratio = 0.2,
    workspace_swipe_min_speed_to_force = 5,
    workspace_swipe_direction_lock = true,
    workspace_swipe_direction_lock_threshold = 10,
    workspace_swipe_create_new = true,
  },
})

-- Session
o.bind("SUPER + L", "Lock", "omarchy-system-lock", { locked = true })
o.bind("SUPER + SHIFT + L", "Suspend", "systemctl suspend", { locked = true })
o.bind("SUPER + ALT + L", "Suspend", "systemctl suspend", { locked = true })
o.bind("CTRL + SHIFT + ALT + SUPER + Delete", "Power off", "systemctl poweroff")

-- Apps
o.bind("SUPER + Return", "Terminal", { omarchy = "terminal" })
o.bind("SUPER + T", "Terminal", { omarchy = "terminal" })
o.bind("CTRL + ALT + T", "Terminal", { omarchy = "terminal" })
o.bind("SUPER + E", "Files", { omarchy = "nautilus" })
o.bind("SUPER + W", "Browser", { omarchy = "browser" })
o.bind("CTRL + SUPER + V", "Sound", "omarchy-shell shell toggle omarchy.audio")
o.bind("CTRL + SHIFT + Escape", "Task manager", { tui = "btop" })

-- Add your own below. Change a key by unbinding it first:
-- hl.unbind("SUPER + W")
-- o.bind("SUPER + W", "Browser", "firefox")
