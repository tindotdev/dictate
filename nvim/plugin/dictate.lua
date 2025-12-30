-- Prevent double loading
if vim.g.loaded_dictate then
  return
end
vim.g.loaded_dictate = true

-- Provide a setup command for lazy loading
vim.api.nvim_create_user_command('DictateSetup', function()
  require('dictate').setup()
end, { desc = 'Initialize dictate plugin' })
