-- chartgen.sty's in-process route: the engine, as the `chartgen_luatex` Lua module, runs
-- inside the LuaLaTeX run. Each round either asks the document to measure labels, which
-- happens here and now through tex.runtoks, or returns the chart's PGF to set in place.
-- LuaTeX loads binary modules only with shell escape enabled.
chartgen = chartgen or {}

local engine -- the module's table, false when it cannot be loaded
local load_error

-- The engine module: a path to the shared library, or a module name for require (found
-- through package.cpath and kpathsea's clua search)
function chartgen.load(module)
  if engine ~= nil then return engine end
  local ok, result
  if module:find("/", 1, true) or module:match("%.so$") or module:match("%.dll$") or module:match("%.dylib$") then
    local open, why = package.loadlib(module, "luaopen_chartgen_luatex")
    ok, result = open ~= nil, open and open() or why
  else
    ok, result = pcall(require, module)
  end
  if ok and type(result) == "table" and type(result.round) == "function" then
    engine = result
  else
    engine, load_error = false, tostring(result or "LuaTeX did not load it (is shell escape enabled?)")
  end
  return engine
end

-- 1 when the in-process route can be used
function chartgen.available(module)
  tex.sprint(chartgen.load(module) and "1" or "0")
end

function chartgen.why()
  return load_error or "unknown"
end

local function catcodes()
  -- \chartgen@… macros are read with @ as a letter, as in a package
  return luatexbase.registernumber("catcodetable@atletter")
end

local function lines(text)
  local out = {}
  for line in (text .. "\n"):gmatch("(.-)\r?\n") do out[#out + 1] = line end
  if out[#out] == "" then out[#out] = nil end
  return out
end

local function read(path)
  local found = kpse.find_file(path, "tex", true) or path
  local file = io.open(found, "rb")
  if not file then return nil end
  local text = file:read("a")
  file:close()
  return text
end

local collected

-- \chartgen@record{id}{wd}{ht}{dp}{max}, called by \chartgen@measure
function chartgen.record(id, wd, ht, dp, max)
  collected[#collected + 1] = string.format("%s %s %s %s %s\n", id, wd, ht, dp, max)
end

local function fail(message)
  tex.print(catcodes(), "\\chartgen@fail{" .. message .. "}")
end

-- Draw the chart of the specification at `path`, with the options as key=value lines
function chartgen.chart(path, options)
  local spec = read(path)
  if not spec then return fail("cannot read the chart specification " .. path) end
  local measured = {}
  for _ = 1, 64 do
    local reply = engine.round(spec, options, table.concat(measured))
    local status, body = reply:match("^(%a+)\n(.*)$")
    if status == "measure" then
      collected = measured
      tex.runtoks(function() tex.print(catcodes(), lines(body)) end)
    elseif status == "done" then
      tex.print(catcodes(), lines(body))
      return
    else
      return fail(body or reply)
    end
  end
  fail("the chart's layout did not settle")
end
