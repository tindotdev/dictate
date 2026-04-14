local Config = require("dictate.config")

local M = {}
local help_cache = {}

local function cache_key(args)
  return table.concat(Config.command(args), "\0")
end

local function command_output(args)
  local key = cache_key(args)
  local cached = help_cache[key]
  if cached then
    return cached.output, cached.status
  end

  local ok, output = pcall(vim.fn.system, Config.command(args))
  local result = {}
  if not ok then
    result.output = ""
    result.status = -1
  else
    result.output = output
    result.status = vim.v.shell_error
  end

  help_cache[key] = result
  return result.output, result.status
end

local function advertises_token(output, token)
  return output:find(token, 1, true) ~= nil
end

local function supports_help_token(command, token)
  local output, status = command_output({ command, "--help" })
  return status == 0 and advertises_token(output, token)
end

function M.supports_json_events()
  return supports_help_token("record", "--json-events")
end

function M.supports_save_last_audio()
  return supports_help_token("record", "--save-last-audio")
end

function M.supports_retry()
  local output, status = command_output({ "retry", "--help" })
  return status == 0 and advertises_token(output, "retry") and advertises_token(output, "--json-events")
end

function M.reset()
  help_cache = {}
end

return M
