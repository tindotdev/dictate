local Config = require("dictate.config")

local M = {}

local health = vim.health or require("health")

local function command_output(cmd)
  local output = vim.fn.system(cmd)
  return output, vim.v.shell_error
end

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

  local help_cmd = Config.command({ "record", "--help" })
  local help_output, help_status = command_output(help_cmd)
  if help_status == 0 and help_output:find("%-%-json%-events", 1, false) then
    h.ok("dictate supports --json-events")
  else
    h.error("dictate does not advertise --json-events", {
      "Update dictate-cli to a version that includes the Neovim integration event stream",
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
