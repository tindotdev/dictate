local Capabilities = require("dictate.capabilities")
local Config = require("dictate.config")

local M = {}

local health = vim.health or require("health")

local function run_checks(h)
  h.start("dictate.nvim")

  local version = vim.version()
  if version.major == 0 and version.minor < 10 then
    h.error("Neovim 0.10+ is required", { "Upgrade Neovim before using dictate.nvim" })
  else
    h.ok(("Neovim %d.%d.%d"):format(version.major, version.minor, version.patch))
  end

  if vim.uv.os_uname().sysname == "Windows_NT" then
    h.error("Windows is not supported", { "dictate.nvim requires Unix signals (SIGUSR1 and SIGINT)" })
  else
    h.ok("Unix-style signal support is available")
  end

  local cfg = Config.get()
  local executable = cfg.cmd[1]
  if vim.fn.executable(executable) == 1 then
    h.ok(("Command found: %s"):format(executable))
  else
    h.error(("Command not found: %s"):format(executable), {
      "Install dictate-cli or set opts.cmd to the full command list",
    })
    return
  end

  if Capabilities.supports_json_events() then
    h.ok("dictate supports --json-events")
  else
    h.error("dictate does not advertise --json-events", {
      "Update dictate-cli to a version that includes the Neovim integration event stream",
    })
  end

  if Capabilities.supports_save_last_audio() then
    h.ok("dictate supports --save-last-audio")
  else
    h.warn("dictate does not advertise --save-last-audio", {
      "dictate.nvim will skip forcing audio persistence for DictateRetry on plugin-managed recordings",
    })
  end

  if Capabilities.supports_retry() then
    h.ok("dictate supports retry")
  else
    h.warn("dictate does not support retry", {
      "Update dictate-cli to enable the DictateRetry command",
    })
  end

  if vim.env.GROQ_API_KEY and vim.env.GROQ_API_KEY ~= "" then
    h.ok("GROQ_API_KEY is set")
  else
    h.error("GROQ_API_KEY is not set", {
      "Export GROQ_API_KEY before starting a dictate.nvim session",
    })
  end
end

function M.check()
  run_checks(health)
end

function M.check_standalone()
  local messages = {}
  local reporter = {
    start = function(message)
      table.insert(messages, ("== %s =="):format(message))
    end,
    ok = function(message)
      table.insert(messages, ("OK: %s"):format(message))
    end,
    warn = function(message)
      table.insert(messages, ("WARN: %s"):format(message))
    end,
    error = function(message, advice)
      table.insert(messages, ("ERROR: %s"):format(message))
      for _, item in ipairs(advice or {}) do
        table.insert(messages, ("  - %s"):format(item))
      end
    end,
  }
  run_checks(reporter)
  print(table.concat(messages, "\n"))
end

function M._run_checks(reporter)
  run_checks(reporter)
end

return M
