#!/usr/bin/env -S nvim -l

vim.opt.loadplugins = false
vim.opt.swapfile = false
vim.opt.shadafile = "NONE"

local cwd = vim.fn.getcwd()
vim.opt.rtp:prepend(cwd)
package.path = table.concat({
  package.path,
  cwd .. "/lua/?.lua",
  cwd .. "/lua/?/init.lua",
  cwd .. "/tests/?.lua",
}, ";")

local suites = {}
local current_suite = nil

local function fail(message)
  error(message, 0)
end

local function deep_equal(left, right)
  return vim.deep_equal(left, right)
end

local assert_api = {
  equals = function(expected, actual)
    if expected ~= actual then
      fail(("expected %s, got %s"):format(vim.inspect(expected), vim.inspect(actual)))
    end
  end,
  is_true = function(value)
    if value ~= true then
      fail(("expected true, got %s"):format(vim.inspect(value)))
    end
  end,
  is_false = function(value)
    if value ~= false then
      fail(("expected false, got %s"):format(vim.inspect(value)))
    end
  end,
  is_nil = function(value)
    if value ~= nil then
      fail(("expected nil, got %s"):format(vim.inspect(value)))
    end
  end,
  is_not_nil = function(value)
    if value == nil then
      fail("expected non-nil value")
    end
  end,
}

assert_api.are = {
  same = function(expected, actual)
    if not deep_equal(expected, actual) then
      fail(("expected %s, got %s"):format(vim.inspect(expected), vim.inspect(actual)))
    end
  end,
}

assert = setmetatable({}, {
  __index = assert_api,
  __call = function(_, condition, message)
    if not condition then
      fail(message or "assertion failed")
    end
  end,
})

function describe(name, callback)
  local suite = {
    name = name,
    tests = {},
    before_each = {},
    after_each = {},
  }
  local previous = current_suite
  current_suite = suite
  callback()
  current_suite = previous
  table.insert(suites, suite)
end

function before_each(callback)
  table.insert(current_suite.before_each, callback)
end

function after_each(callback)
  table.insert(current_suite.after_each, callback)
end

function it(name, callback)
  table.insert(current_suite.tests, { name = name, callback = callback })
end

local files = #arg > 0 and arg or vim.fn.globpath("tests", "*_spec.lua", true, true)
for _, file in ipairs(files) do
  dofile(file)
end

local failures = 0
for _, suite in ipairs(suites) do
  for _, test in ipairs(suite.tests) do
    local ok, err
    for _, hook in ipairs(suite.before_each) do
      ok, err = pcall(hook)
      if not ok then
        break
      end
    end
    if ok ~= false then
      ok, err = pcall(test.callback)
    end
    for _, hook in ipairs(suite.after_each) do
      local hook_ok, hook_err = pcall(hook)
      if ok ~= false and not hook_ok then
        ok, err = hook_ok, hook_err
      end
    end
    if not ok then
      failures = failures + 1
      io.write(("%s :: %s\n%s\n"):format(suite.name, test.name, err))
    end
  end
end

if failures > 0 then
  os.exit(1)
end
