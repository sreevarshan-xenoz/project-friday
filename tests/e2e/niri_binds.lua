-- Parse a niri KDL config (following its include directives) and print what
-- each key bound at the end would run:
--   <KEYS>\t<action>\t<argv...>
-- where:
--   <KEYS> is normalised (Mod, Ctrl, Alt, Shift in order, uppercase key),
--          e.g. "Mod+Y", "Mod+Alt+Y", "Mod+Ctrl+Y"
--   <action> is "spawn <command>" for spawn actions, or "<action-name>"
--   <argv...> tab-separated spawn argv elements, e.g. "friday\task"
--
-- Usage: lua niri_binds.lua <path/to/config.kdl>
-- FRIDAY_SHARE may point to the directory supplying /usr/share/friday
-- (defaults to packaging/linux in the repo).

local config_path = assert(arg[1], "usage: lua niri_binds.lua <path/to/config.kdl>")

local function dirname(p)
  return p:match("^(.*)/[^/]*$") or "."
end

local function normpath(path)
  local parts = {}
  local is_abs = path:sub(1, 1) == "/"
  for part in path:gmatch("[^/]+") do
    if part == ".." then
      if #parts > 0 and parts[#parts] ~= ".." then
        table.remove(parts)
      elseif not is_abs then
        parts[#parts + 1] = ".."
      end
    elseif part ~= "." and part ~= "" then
      parts[#parts + 1] = part
    end
  end
  local res = table.concat(parts, "/")
  if is_abs then
    res = "/" .. res
  elseif res == "" then
    res = "."
  end
  return res
end

local script_dir = dirname(arg[0] or "")
local repo_root = normpath(script_dir .. "/../..")
local default_share = (repo_root == "." and "packaging/linux") or (repo_root .. "/packaging/linux")

local function file_exists(p)
  local f = io.open(p, "r")
  if f then f:close(); return true end
  return false
end

local function resolve_include(path, current_dir)
  local friday_share = os.getenv("FRIDAY_SHARE") or default_share
  if path:sub(1, 17) == "/usr/share/friday" then
    local mapped = friday_share .. path:sub(18)
    if file_exists(mapped) or not file_exists(path) then
      path = mapped
    end
  elseif path:sub(1, 1) ~= "/" then
    path = current_dir .. "/" .. path
  end
  return normpath(path)
end

local function norm_key(k)
  local parts = {}
  for p in tostring(k):gmatch("[^+]+") do
    parts[#parts + 1] = p:gsub("^%s+", ""):gsub("%s+$", "")
  end
  local key = table.remove(parts)
  if not key then return "" end

  local has_mod, has_ctrl, has_alt, has_shift = false, false, false, false
  local other_mods = {}
  for _, m in ipairs(parts) do
    local lower = m:lower()
    if lower == "mod" or lower == "super" then
      has_mod = true
    elseif lower == "ctrl" or lower == "control" then
      has_ctrl = true
    elseif lower == "alt" then
      has_alt = true
    elseif lower == "shift" then
      has_shift = true
    else
      other_mods[#other_mods + 1] = m
    end
  end

  local res = {}
  if has_mod then res[#res + 1] = "Mod" end
  if has_ctrl then res[#res + 1] = "Ctrl" end
  if has_alt then res[#res + 1] = "Alt" end
  if has_shift then res[#res + 1] = "Shift" end
  for _, m in ipairs(other_mods) do res[#res + 1] = m end
  if #key == 1 then
    key = key:upper()
  end
  res[#res + 1] = key
  return table.concat(res, "+")
end

local function tokenize(src)
  local tokens = {}
  local i, n = 1, #src
  while i <= n do
    local c = src:sub(i, i)
    if c == "\\" then
      local j = i + 1
      while j <= n and (src:sub(j, j) == " " or src:sub(j, j) == "\t" or src:sub(j, j) == "\r") do
        j = j + 1
      end
      if j <= n and src:sub(j, j) == "\n" then
        i = j + 1
      else
        error("stray backslash at position " .. i)
      end
    elseif c == " " or c == "\t" or c == "\r" then
      i = i + 1
    elseif c == "\n" then
      tokens[#tokens + 1] = { type = "NEWLINE", val = "\n" }
      i = i + 1
    elseif c == ";" then
      tokens[#tokens + 1] = { type = "SEMI", val = ";" }
      i = i + 1
    elseif c == "=" then
      tokens[#tokens + 1] = { type = "EQUALS", val = "=" }
      i = i + 1
    elseif c == "{" then
      tokens[#tokens + 1] = { type = "LBRACE", val = "{" }
      i = i + 1
    elseif c == "}" then
      tokens[#tokens + 1] = { type = "RBRACE", val = "}" }
      i = i + 1
    elseif c == "/" and i + 1 <= n and src:sub(i + 1, i + 1) == "/" then
      i = i + 2
      while i <= n and src:sub(i, i) ~= "\n" do
        i = i + 1
      end
    elseif c == "/" and i + 1 <= n and src:sub(i + 1, i + 1) == "*" then
      local depth = 1
      i = i + 2
      while i <= n and depth > 0 do
        if src:sub(i, i + 1) == "/*" then
          depth = depth + 1
          i = i + 2
        elseif src:sub(i, i + 1) == "*/" then
          depth = depth - 1
          i = i + 2
        else
          i = i + 1
        end
      end
    elseif c == "/" and i + 1 <= n and src:sub(i + 1, i + 1) == "-" then
      tokens[#tokens + 1] = { type = "SLASHDASH", val = "/-" }
      i = i + 2
    elseif c == "\"" then
      i = i + 1
      local chars = {}
      while i <= n do
        local sc = src:sub(i, i)
        if sc == "\"" then
          i = i + 1
          break
        elseif sc == "\\" then
          i = i + 1
          local ec = src:sub(i, i)
          if ec == "\"" then chars[#chars + 1] = "\""
          elseif ec == "\\" then chars[#chars + 1] = "\\"
          elseif ec == "/" then chars[#chars + 1] = "/"
          elseif ec == "n" then chars[#chars + 1] = "\n"
          elseif ec == "r" then chars[#chars + 1] = "\r"
          elseif ec == "t" then chars[#chars + 1] = "\t"
          else chars[#chars + 1] = ec end
          i = i + 1
        else
          chars[#chars + 1] = sc
          i = i + 1
        end
      end
      tokens[#tokens + 1] = { type = "STRING", val = table.concat(chars) }
    elseif c == "r" and (src:sub(i + 1, i + 1) == "\"" or src:sub(i + 1, i + 1) == "#") then
      local hashes = 0
      local j = i + 1
      while j <= n and src:sub(j, j) == "#" do
        hashes = hashes + 1
        j = j + 1
      end
      if j <= n and src:sub(j, j) == "\"" then
        j = j + 1
        local ending = "\"" .. string.rep("#", hashes)
        local end_pos = src:find(ending, j, true)
        if not end_pos then error("unterminated raw string") end
        tokens[#tokens + 1] = { type = "STRING", val = src:sub(j, end_pos - 1) }
        i = end_pos + #ending
      else
        local j2 = i
        while j2 <= n do
          local jc = src:sub(j2, j2)
          if jc:match("[%s;{}=\"]") then break end
          if jc == "/" and j2 + 1 <= n and (src:sub(j2 + 1, j2 + 1) == "/" or src:sub(j2 + 1, j2 + 1) == "*") then break end
          j2 = j2 + 1
        end
        tokens[#tokens + 1] = { type = "IDENT", val = src:sub(i, j2 - 1) }
        i = j2
      end
    else
      local j = i
      while j <= n do
        local jc = src:sub(j, j)
        if jc:match("[%s;{}=\"]") then break end
        if jc == "/" and j + 1 <= n and (src:sub(j + 1, j + 1) == "/" or src:sub(j + 1, j + 1) == "*") then break end
        j = j + 1
      end
      tokens[#tokens + 1] = { type = "IDENT", val = src:sub(i, j - 1) }
      i = j
    end
  end
  return tokens
end

local function parse_nodes(tokens, idx)
  local nodes = {}
  local i = idx or 1
  local function peek() return tokens[i] end
  local function advance() local t = tokens[i]; if t then i = i + 1 end; return t end
  local function skip_delims()
    while true do
      local t = peek()
      if t and (t.type == "NEWLINE" or t.type == "SEMI") then advance() else break end
    end
  end

  while i <= #tokens do
    skip_delims()
    local t = peek()
    if not t or t.type == "RBRACE" then break end

    local is_commented = false
    if t.type == "SLASHDASH" then
      advance()
      is_commented = true
      skip_delims()
      t = peek()
    end
    if not t or t.type == "RBRACE" then break end
    if t.type ~= "IDENT" and t.type ~= "STRING" then
      error("unexpected token for node name: " .. t.type .. " (" .. tostring(t.val) .. ") at token " .. i)
    end

    local node = { name = advance().val, args = {}, props = {}, children = {} }

    while true do
      local next_t = peek()
      if not next_t or next_t.type == "NEWLINE" or next_t.type == "SEMI" or next_t.type == "RBRACE" then break end
      if next_t.type == "LBRACE" then
        advance()
        node.children, i = parse_nodes(tokens, i)
        local close = advance()
        if not close or close.type ~= "RBRACE" then error("expected '}' to close block for " .. node.name) end
        break
      elseif next_t.type == "IDENT" or next_t.type == "STRING" then
        local item = advance()
        local after = peek()
        if after and after.type == "EQUALS" then
          advance()
          local val = peek()
          if not val or (val.type ~= "IDENT" and val.type ~= "STRING") then error("expected value after '=' in " .. item.val) end
          advance()
          node.props[item.val] = val.val
        else
          node.args[#node.args + 1] = item.val
        end
      else
        error("unexpected token in " .. node.name .. ": " .. next_t.type)
      end
    end

    if not is_commented then nodes[#nodes + 1] = node end
  end
  return nodes, i
end

local function load_kdl(filepath, seen)
  seen = seen or {}
  local resolved = normpath(filepath)
  if seen[resolved] then return {} end
  seen[resolved] = true

  local fh = assert(io.open(resolved, "r"), "cannot open file: " .. resolved)
  local content = fh:read("*a")
  fh:close()

  local dir = dirname(resolved)
  local nodes = parse_nodes(tokenize(content))
  local all_nodes = {}

  for _, node in ipairs(nodes) do
    if node.name == "include" then
      local inc_path = resolve_include(node.args[1], dir)
      local inc_nodes = load_kdl(inc_path, seen)
      for _, n in ipairs(inc_nodes) do
        all_nodes[#all_nodes + 1] = n
      end
    else
      all_nodes[#all_nodes + 1] = node
    end
  end

  return all_nodes
end

local nodes = load_kdl(config_path)
for _, node in ipairs(nodes) do
  if node.name == "binds" then
    for _, b in ipairs(node.children) do
      local k = norm_key(b.name)
      for _, act in ipairs(b.children) do
        if act.name == "spawn" then
          local cmd = table.concat(act.args, " ")
          io.write(k, "\tspawn ", cmd)
          for _, a in ipairs(act.args) do
            io.write("\t", a)
          end
          io.write("\n")
        else
          io.write(k, "\t", act.name, "\n")
        end
      end
    end
  end
end
