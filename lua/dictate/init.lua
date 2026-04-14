local M = {}

local commands_registered = false
local retry_command_registered = false
local setup_done = false
local title = "dictate.nvim"

local function notify(message, level)
  vim.notify(message, level or vim.log.levels.INFO, { title = title })
end

local function ensure_setup()
  if not setup_done then
    M.setup({})
  end
end

local function register_commands()
  if not commands_registered then
    vim.api.nvim_create_user_command("DictateStart", function()
      ensure_setup()
      require("dictate.session").start()
    end, { desc = "Start dictate recording" })

    vim.api.nvim_create_user_command("DictateStop", function()
      ensure_setup()
      require("dictate.session").stop()
    end, { desc = "Stop or cancel dictate recording" })

    vim.api.nvim_create_user_command("DictateToggle", function()
      ensure_setup()
      require("dictate.session").toggle()
    end, { desc = "Toggle dictate recording" })

    commands_registered = true
  end
end

local function sync_retry_command()
  local supports_retry = require("dictate.capabilities").supports_retry()

  if supports_retry and not retry_command_registered then
    vim.api.nvim_create_user_command("DictateRetry", function()
      M.retry()
    end, { desc = "Retry the last saved dictate recording" })
    retry_command_registered = true
    return
  end

  if not supports_retry and retry_command_registered then
    pcall(vim.api.nvim_del_user_command, "DictateRetry")
    retry_command_registered = false
  end
end

local function ensure_retry_supported()
  if require("dictate.capabilities").supports_retry() then
    return true
  end

  notify("dictate-cli does not support retry; update dictate-cli to enable DictateRetry", vim.log.levels.WARN)
  return false
end

function M.setup(opts)
  require("dictate.config").setup(opts)
  local capabilities = require("dictate.capabilities")
  capabilities.reset()
  register_commands()
  sync_retry_command()
  capabilities.supports_save_last_audio()

  if not setup_done then
    vim.api.nvim_create_autocmd("VimLeavePre", {
      group = vim.api.nvim_create_augroup("dictate.nvim", { clear = true }),
      callback = function()
        require("dictate.session").teardown()
      end,
    })
  end

  setup_done = true
end

function M.start()
  ensure_setup()
  return require("dictate.session").start()
end

function M.stop()
  ensure_setup()
  return require("dictate.session").stop()
end

function M.toggle()
  ensure_setup()
  return require("dictate.session").toggle()
end

function M.retry()
  ensure_setup()
  if not ensure_retry_supported() then
    return false
  end
  return require("dictate.session").retry()
end

function M.get_state()
  ensure_setup()
  return require("dictate.session").get_state()
end

return M
