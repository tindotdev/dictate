-- Prevent double loading
if vim.g.loaded_say then
  return
end
vim.g.loaded_say = true

-- Provide a setup command for lazy loading
vim.api.nvim_create_user_command('SaySetup', function()
  require('say').setup()
end, { desc = 'Initialize say plugin' })
