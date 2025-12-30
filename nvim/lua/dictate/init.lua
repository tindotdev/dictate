local M = {}

---Setup the dictate plugin
---@param opts table|nil Configuration options
function M.setup(opts)
  -- Initialize configuration
  require('dictate.config').setup(opts)

  -- Register commands
  vim.api.nvim_create_user_command('DictateToggle', function()
    require('dictate.job').toggle()
  end, { desc = 'Toggle dictation' })

  vim.api.nvim_create_user_command('DictateStart', function()
    require('dictate.job').start()
  end, { desc = 'Start dictation' })

  vim.api.nvim_create_user_command('DictateStop', function()
    require('dictate.job').stop()
  end, { desc = 'Stop dictation' })

  -- Setup keymap if configured
  local cfg = require('dictate.config').get()
  if cfg.keymap then
    vim.keymap.set('n', cfg.keymap, ':DictateToggle<CR>', {
      desc = 'Toggle dictation',
      silent = true,
    })
  end
end

---Check if dictation is currently running
---@return boolean
function M.is_running()
  return require('dictate.job').is_running()
end

---Get current dictation state
---@return string
function M.get_state()
  return require('dictate.job').get_state()
end

return M
