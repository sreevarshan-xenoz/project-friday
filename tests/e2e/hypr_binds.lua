-- Load a Hyprland Lua config the way Hyprland does, with a recording stand-in
-- for its `hl` API, and print what each key bound at the end would run:
--   <KEYS>\t<action>        one line per action, keys normalised (sorted
--                           modifiers, upper case), e.g. "ALT+SUPER+Y"
-- An action is "exec <command>" (hl.dsp.exec_cmd), "global <name>"
-- (hl.dsp.global) or "other"; a Lua-function bind is run to see what it
-- dispatches. A bind that fires on the key's release ({ release = true })
-- is prefixed "release ". Unbinds are honoured, so this is the effective keymap.
--
-- Usage: lua hypr_binds.lua <path/to/hyprland.lua>
-- HOME must point at the home the config expects (~/.config/hypr is its dir).
-- The real Hyprland can't run on a GPU-less CI runner (no GBM allocator), so
-- the hotkey test reads the keymap this way and then runs the commands.

local config = assert(arg[1], "usage: lua hypr_binds.lua <hyprland.lua>")
local dir = config:match("^(.*)/[^/]*$") or "."
local home = os.getenv("HOME") or ""
package.path = dir .. "/?.lua;" .. dir .. "/?/init.lua;" .. home .. "/.config/?.lua;" .. package.path

-- Anything not modelled below: callable, indexable, and returns more of itself.
local Any = {}
Any.__index = function() return setmetatable({}, Any) end
Any.__call = function() return setmetatable({}, Any) end
Any.__concat = function() return "" end
Any.__tostring = function() return "" end
local function any() return setmetatable({}, Any) end

local function norm(keys)
  local parts = {}
  for p in tostring(keys):gmatch("[^+]+") do
    parts[#parts + 1] = p:gsub("^%s+", ""):gsub("%s+$", ""):upper()
  end
  local key = table.remove(parts)
  table.sort(parts)
  parts[#parts + 1] = key
  return table.concat(parts, "+")
end

local binds = {} -- ordered list of { keys, dispatcher, active }
local function unbind(keys)
  local k = norm(keys)
  for _, b in ipairs(binds) do
    if b.keys == k then b.active = false end
  end
end

-- Other dispatchers, plain (hl.dsp.focus(...)) or namespaced
-- (hl.dsp.window.close()), all describe themselves as "other".
local Other = {}
Other.__index = function() return setmetatable({}, Other) end
Other.__call = function() return { kind = "other" } end
local dsp = setmetatable({
  exec_cmd = function(cmd) return { kind = "exec", arg = cmd } end,
  global = function(name) return { kind = "global", arg = name } end,
}, Other)

local dispatched -- collects dispatches while a function bind runs
hl = setmetatable({
  dsp = dsp,
  bind = function(keys, dispatcher, opts)
    local b = { keys = norm(keys), dispatcher = dispatcher, active = true,
      release = type(opts) == "table" and opts.release == true }
    binds[#binds + 1] = b
    return { unbind = function() b.active = false end }
  end,
  unbind = unbind,
  dispatch = function(d) if dispatched then dispatched[#dispatched + 1] = d end end,
}, {
  -- Queries (hl.get_active_monitor(), ...) answer nil, as with no monitor yet.
  __index = function(_, k)
    if type(k) == "string" and k:sub(1, 4) == "get_" then return function() return nil end end
    return any()
  end,
})

dofile(config)

local function describe(d)
  if type(d) == "table" and d.kind == "exec" then return "exec " .. tostring(d.arg) end
  if type(d) == "table" and d.kind == "global" then return "global " .. tostring(d.arg) end
  return "other"
end

for _, b in ipairs(binds) do
  if b.active then
    local actions = {}
    if type(b.dispatcher) == "function" then
      dispatched = {}
      pcall(b.dispatcher)
      for _, d in ipairs(dispatched) do actions[#actions + 1] = describe(d) end
      dispatched = nil
    else
      actions[1] = describe(b.dispatcher)
    end
    for _, a in ipairs(actions) do io.write(b.keys, "\t", b.release and "release " or "", a, "\n") end
  end
end
