-- Minimal init for testing with plenary.nvim
local plugin_root = vim.fn.fnamemodify(debug.getinfo(1, 'S').source:sub(2), ':h:h')
vim.opt.rtp:prepend(plugin_root)
vim.opt.rtp:append(vim.fn.stdpath('data') .. '/lazy/plenary.nvim')

vim.cmd('runtime plugin/plenary.vim')
