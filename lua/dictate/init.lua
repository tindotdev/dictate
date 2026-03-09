local M = {}

local commands_registered = false
local setup_done = false

local function ensure_setup()
  if not setup_done then
    M.setup({})
  end
end

local function register_commands()
  if commands_registered then
    return
  end

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

function M.setup(opts)
  require("dictate.config").setup(opts)
  register_commands()

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

function M.get_state()
  ensure_setup()
  return require("dictate.session").get_state()
end

return M
