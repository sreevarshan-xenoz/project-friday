-- Game mode (Super+Alt+G): animations, blur and shadows off and the
-- performance power profile on; press again to put everything back.

local saved = nil

local function notify(body)
  hl.exec_cmd("notify-send -a 'Game mode' -i input-gaming 'Game mode' '" .. body .. "'")
end

local function toggle()
  if saved == nil then
    saved = {
      animations = hl.get_config("animations:enabled"),
      blur = hl.get_config("decoration:blur:enabled"),
      shadow = hl.get_config("decoration:shadow:enabled"),
    }
    hl.config({
      animations = { enabled = false },
      decoration = { blur = { enabled = false }, shadow = { enabled = false } },
    })
    hl.exec_cmd("powerprofilesctl get > \"$XDG_RUNTIME_DIR/friday-gamemode-profile\"; powerprofilesctl set performance")
    notify("On: no animations, blur or shadows; performance power profile")
  else
    hl.config({
      animations = { enabled = saved.animations },
      decoration = { blur = { enabled = saved.blur }, shadow = { enabled = saved.shadow } },
    })
    hl.exec_cmd("powerprofilesctl set \"$(cat \"$XDG_RUNTIME_DIR/friday-gamemode-profile\" 2>/dev/null || echo balanced)\"")
    notify("Off: effects and the previous power profile are back")
    saved = nil
  end
end

o.bind("SUPER + ALT + G", "Game mode", toggle)
