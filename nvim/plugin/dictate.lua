-- Prevent double loading
if vim.g.loaded_dictate then
  return
end
vim.g.loaded_dictate = true

-- Track if setup has been called
vim.g.dictate_setup_done = false

---Ensure setup has been called (for zero-config usage)
local function ensure_setup()
  if not vim.g.dictate_setup_done then
    require('dictate').setup()
  end
end

-- Register commands immediately for zero-config usage
-- Commands lazy-load modules and ensure setup on first use

vim.api.nvim_create_user_command('DictateToggle', function()
  ensure_setup()
  require('dictate.job').toggle()
end, { desc = 'Toggle dictation' })

vim.api.nvim_create_user_command('DictateStart', function()
  ensure_setup()
  require('dictate.job').start()
end, { desc = 'Start dictation' })

vim.api.nvim_create_user_command('DictateStop', function()
  ensure_setup()
  require('dictate.job').stop()
end, { desc = 'Stop dictation' })

-- Provide a setup command for explicit initialization
vim.api.nvim_create_user_command('DictateSetup', function()
  require('dictate').setup()
end, { desc = 'Initialize dictate plugin' })

-- Health check command - always works, even when lazy-loaded
-- Use this if :checkhealth dictate doesn't work with your lazy-loader
vim.api.nvim_create_user_command('DictateHealth', function()
  require('dictate.health').check_standalone()
end, { desc = 'Run dictate health check' })
