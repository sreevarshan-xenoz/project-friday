require("hyprland.lib")
require("hyprland.variables")
if is_file_exists(HOME .. "/.config/hypr/custom/variables.lua") then
    require("custom.variables")
end

local qsScripts = "$HOME/.config/quickshell/$qsConfig/scripts"
local hyprScripts = "$HOME/.config/hypr/hyprland/scripts"
local qsIpcCall = "qs -c $qsConfig ipc call"
local qsIsAlive = qsIpcCall .. " TEST_ALIVE"

-- Shell / Launchers
hl.bind("SUPER + Space", hl.dsp.exec_cmd(qsIsAlive .. " && qs -c $qsConfig ipc call searchToggle || omarchy-menu toggle || fuzzel"), { description = "Shell: Toggle search" })
hl.bind("SUPER + SUPER_L", hl.dsp.global("quickshell:searchToggleRelease"), { description = "Shell: Toggle search" })
hl.bind("SUPER + SUPER_R", hl.dsp.global("quickshell:searchToggleRelease"))
hl.bind("SUPER_L", hl.dsp.global("quickshell:workspaceNumber"), { ignore_mods = true, transparent = true })
hl.bind("SUPER_R", hl.dsp.global("quickshell:workspaceNumber"), { ignore_mods = true, transparent = true })
hl.bind("SUPER_L", hl.dsp.global("quickshell:workspaceNumber"), { ignore_mods = true, transparent = true, release = true })
hl.bind("SUPER_R", hl.dsp.global("quickshell:workspaceNumber"), { ignore_mods = true, transparent = true, release = true })

-- Overview & Sidebars
hl.bind("SUPER + Tab", hl.dsp.global("quickshell:overviewWorkspacesToggle"), { description = "Shell: Toggle overview" })
hl.bind("SUPER + V", hl.dsp.global("quickshell:overviewClipboardToggle"), { description = "Shell: Toggle clipboard" })
hl.bind("SUPER + V", hl.dsp.exec_cmd(qsIsAlive .. " || pkill fuzzel || cliphist list | fuzzel --match-mode fzf --dmenu | cliphist decode | wl-copy"))
hl.bind("SUPER + Period", hl.dsp.global("quickshell:overviewEmojiToggle"), { description = "Shell: Toggle emoji" })
hl.bind("SUPER + Period", hl.dsp.exec_cmd(qsIsAlive .. " || pkill fuzzel || " .. hyprScripts .. "/fuzzel-emoji.sh copy"))

hl.bind("SUPER + A", hl.dsp.global("quickshell:sidebarLeftToggle"), { description = "Shell: Toggle left sidebar" })
hl.bind("SUPER + ALT + A", hl.dsp.global("quickshell:sidebarLeftToggleDetach"))
hl.bind("SUPER + N", hl.dsp.global("quickshell:sidebarRightToggle"), { description = "Shell: Toggle right sidebar" })
hl.bind("SUPER + Slash", hl.dsp.global("quickshell:cheatsheetToggle"), { description = "Shell: Toggle cheatsheet" })
hl.bind("SUPER + K", hl.dsp.exec_cmd("omarchy-menu-keybindings"), { description = "Shell: Keybindings" })
hl.bind("SUPER + ALT + K", hl.dsp.exec_cmd("omarchy-menu-tmux-keybindings"), { description = "Shell: Tmux keybindings" })

hl.bind("SUPER + M", hl.dsp.global("quickshell:mediaControlsToggle"), { description = "Shell: Toggle media controls" })
hl.bind("SUPER + G", hl.dsp.global("quickshell:overlayToggle"), { description = "Shell: Toggle widget overlay" })
hl.bind("SUPER + L", hl.dsp.exec_cmd("quickshell:lock || omarchy-system-lock || hyprlock"), { description = "System: Lock screen" })
hl.bind("SUPER + CTRL + L", hl.dsp.exec_cmd("omarchy-system-lock"), { description = "System: Lock screen" })
hl.bind("CTRL + ALT + Delete", hl.dsp.exec_cmd(qsIsAlive .. " && qs -c $qsConfig ipc call sessionToggle || wlogout -p layer-shell"), { description = "Shell: Toggle session menu" })

-- Media keys
hl.bind("XF86MonBrightnessUp", hl.dsp.exec_cmd(qsIpcCall .. " brightness increment || brightnessctl s 5%+"), { locked = true, repeating = true })
hl.bind("XF86MonBrightnessDown", hl.dsp.exec_cmd(qsIpcCall .. " brightness decrement || brightnessctl s 5%-"), { locked = true, repeating = true })
hl.bind("XF86AudioRaiseVolume", hl.dsp.exec_cmd("wpctl set-volume @DEFAULT_AUDIO_SINK@ 2%+ -l 1.5"), { locked = true, repeating = true })
hl.bind("XF86AudioLowerVolume", hl.dsp.exec_cmd("wpctl set-volume @DEFAULT_AUDIO_SINK@ 2%-"), { locked = true, repeating = true })

-- Wallpaper & Themes
hl.bind("CTRL + SUPER + T", hl.dsp.global("quickshell:wallpaperSelectorToggle"), { description = "Shell: Change wallpaper" })
hl.bind("CTRL + SUPER + T", hl.dsp.exec_cmd(qsIsAlive .. " || " .. qsScripts .. "/colors/switchwall.sh"))
hl.bind("CTRL + SUPER + ALT + T", hl.dsp.global("quickshell:wallpaperSelectorRandom"), { description = "Shell: Random wallpaper" })
hl.bind("CTRL + SUPER + SHIFT + D", hl.dsp.global("quickshell:toggleLightDark"), { description = "Shell: Toggle light/dark mode" })
hl.bind("CTRL + SUPER + R", hl.dsp.exec_cmd("killall ydotool qs quickshell; qs -c $qsConfig &"), { description = "Shell: Restart widgets" })

-- Screen Capture
hl.bind("PRINT", hl.dsp.exec_cmd("omarchy-capture-screenshot"), { description = "Capture: Fullscreen screenshot" })
hl.bind("SUPER + SHIFT + S", hl.dsp.global("quickshell:regionScreenshot"), { description = "Capture: Screen snip" })
hl.bind("SUPER + SHIFT + S", hl.dsp.exec_cmd(qsIsAlive .. " || pidof slurp || omarchy-capture-region || hyprshot --freeze --clipboard-only --mode region --silent"))
hl.bind("ALT + PRINT", hl.dsp.exec_cmd("omarchy-capture-screenrecording --stop-recording || omarchy-menu toggle trigger.capture.screenrecord"), { description = "Capture: Screen recording" })
hl.bind("SUPER + PRINT", hl.dsp.exec_cmd("pkill hyprpicker || hyprpicker -a"), { description = "Capture: Color picker" })

-- Window Closing (Universal & Non-conflicting: Super+Q, Alt+F4, Super+Shift+Q)
hl.bind("SUPER + Q", hl.dsp.window.close(), { description = "Window: Close" })
hl.bind("ALT + F4", hl.dsp.window.close(), { description = "Window: Close" })
hl.bind("SUPER + SHIFT + Q", hl.dsp.window.close(), { description = "Window: Close" })
hl.bind("SUPER + SHIFT + ALT + Q", hl.dsp.exec_cmd("hyprctl kill"), { description = "Window: Force kill" })

-- Window Tiling & Positioning
hl.bind("SUPER + F", hl.dsp.window.fullscreen({ mode = "fullscreen", action = "toggle" }), { description = "Window: Fullscreen" })
hl.bind("SUPER + D", hl.dsp.window.fullscreen({ mode = "maximized", action = "toggle" }), { description = "Window: Maximize" })
hl.bind("SUPER + ALT + Space", hl.dsp.window.float({ action = "toggle" }), { description = "Window: Float/Tile toggle" })
hl.bind("SUPER + J", hl.dsp.layout("togglesplit"), { description = "Window: Toggle split" })
hl.bind("SUPER + P", hl.dsp.window.pin(), { description = "Window: Pin" })

-- Scratchpad / Secret Window (Super+S, Super+grave)
hl.bind("SUPER + S", hl.dsp.workspace.toggle_special("scratchpad"), { description = "Window: Toggle secret window / scratchpad" })
hl.bind("SUPER + ALT + S", hl.dsp.window.move({ workspace = "special:scratchpad", follow = false }), { description = "Window: Move to secret window / scratchpad" })
hl.bind("SUPER + grave", hl.dsp.workspace.toggle_special("scratchpad"), { description = "Window: Toggle secret window / scratchpad" })
hl.bind("SUPER + SHIFT + grave", hl.dsp.window.move({ workspace = "special:scratchpad", follow = false }), { description = "Window: Move to secret window / scratchpad" })

-- Focus & Move
local arrowkey = { "Left", "Right", "Up", "Down" }
local focusdir = { "l", "r", "u", "d" }
for i = 1, 4 do
    hl.bind("SUPER + " .. arrowkey[i], hl.dsp.focus({ direction = focusdir[i] }), { description = "Focus: " .. arrowkey[i] })
    hl.bind("SUPER + SHIFT + " .. arrowkey[i], hl.dsp.window.move({ direction = focusdir[i] }), { description = "Move: " .. arrowkey[i] })
end
hl.bind("ALT + Tab", hl.dsp.window.cycle_next(), { description = "Focus: Next window" })
hl.bind("ALT + SHIFT + Tab", hl.dsp.window.cycle_next({ next = false }), { description = "Focus: Previous window" })

-- Workspaces
for i = 1, 10 do
    hl.bind("SUPER + " .. (i % 10), hl.dsp.focus({ workspace = tostring(i) }), { description = "Workspace: Switch to " .. i })
    hl.bind("SUPER + ALT + " .. (i % 10), hl.dsp.window.move({ workspace = tostring(i), follow = false }), { description = "Workspace: Move to " .. i })
end

-- Mouse bindings
hl.bind("SUPER + mouse:272", hl.dsp.window.drag(), { mouse = true })
hl.bind("SUPER + mouse:273", hl.dsp.window.resize(), { mouse = true })

-- Application Launchers (User Explicit: Super+W=Browser, Super+E=File Manager, Super+T=Terminal)
hl.bind("SUPER + W", hl.dsp.exec_cmd(browser), { description = "App: Browser" })
hl.bind("SUPER + B", hl.dsp.exec_cmd(browser), { description = "App: Browser" })
hl.bind("SUPER + SHIFT + W", hl.dsp.exec_cmd(browser .. " --incognito || " .. browser), { description = "App: Browser (private)" })
hl.bind("SUPER + SHIFT + B", hl.dsp.exec_cmd(browser), { description = "App: Browser" })
hl.bind("SUPER + SHIFT + Return", hl.dsp.exec_cmd(browser), { description = "App: Browser" })

hl.bind("SUPER + E", hl.dsp.exec_cmd(fileManager), { description = "App: File manager" })
hl.bind("SUPER + SHIFT + F", hl.dsp.exec_cmd(fileManager), { description = "App: File manager" })

hl.bind("SUPER + T", hl.dsp.exec_cmd(terminal), { description = "App: Terminal" })
hl.bind("SUPER + Return", hl.dsp.exec_cmd(terminal), { description = "App: Terminal" })
hl.bind("CTRL + ALT + T", hl.dsp.exec_cmd(terminal), { description = "App: Terminal" })

hl.bind("SUPER + C", hl.dsp.exec_cmd(codeEditor), { description = "App: Code editor" })
hl.bind("SUPER + SHIFT + C", hl.dsp.exec_cmd(codeEditor), { description = "App: Code editor" })
hl.bind("SUPER + SHIFT + N", hl.dsp.exec_cmd("omarchy-launch-editor || " .. codeEditor), { description = "App: Editor" })
hl.bind("SUPER + X", hl.dsp.exec_cmd(textEditor), { description = "App: Text editor" })
hl.bind("SUPER + I", hl.dsp.exec_cmd(settingsApp), { description = "App: Settings app" })
hl.bind("CTRL + SUPER + V", hl.dsp.exec_cmd(volumeMixer), { description = "App: Volume mixer" })
hl.bind("CTRL + SHIFT + Escape", hl.dsp.exec_cmd(taskManager), { description = "App: Task manager" })
hl.bind("SUPER + CTRL + T", hl.dsp.exec_cmd("omarchy-launch-tui btop || " .. taskManager), { description = "App: Activity monitor (btop)" })

-- Omarchy Menus & Tools
hl.bind("SUPER + ALT + Space", hl.dsp.exec_cmd("omarchy-menu toggle"), { description = "Omarchy: Root Menu" })
hl.bind("SUPER + Escape", hl.dsp.exec_cmd("omarchy-menu toggle system"), { description = "Omarchy: System Menu" })
hl.bind("SUPER + CTRL + Space", hl.dsp.exec_cmd("omarchy-menu toggle background"), { description = "Omarchy: Backgrounds" })
hl.bind("SUPER + SHIFT + CTRL + Space", hl.dsp.exec_cmd("omarchy-menu toggle theme"), { description = "Omarchy: Themes" })
hl.bind("SUPER + CTRL + C", hl.dsp.exec_cmd("omarchy-menu toggle capture"), { description = "Omarchy: Capture menu" })
hl.bind("SUPER + CTRL + H", hl.dsp.exec_cmd("omarchy-menu toggle hardware"), { description = "Omarchy: Hardware menu" })

-- FRIDAY Cognitive Subsystem
hl.bind("SUPER + SHIFT + CTRL + A", hl.dsp.exec_cmd("friday menu || omarchy-agent --pick"), { description = "FRIDAY: Cognitive Assistant" })
hl.bind("SUPER + CTRL + F", hl.dsp.exec_cmd("friday status || omarchy-state"), { description = "FRIDAY: System State" })
