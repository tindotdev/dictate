vim.opt.swapfile = false
vim.opt.shadafile = "NONE"

local root = vim.fn.getcwd()
local mode = vim.env.DICTATE_NVIM_MODE or "fake"

vim.opt.runtimepath:prepend(root)
package.path = table.concat({
  package.path,
  root .. "/lua/?.lua",
  root .. "/lua/?/init.lua",
}, ";")

local cmd
if mode == "real" then
  cmd = { root .. "/target/debug/dictate" }
else
  cmd = { root .. "/tests/fixtures/fake-dictate.sh" }
end

require("dictate").setup({
  cmd = cmd,
  clipboard = false,
  disabled_filetypes = {},
  disabled_buftypes = {},
})

vim.keymap.set("n", "<F9>", function()
  require("dictate").toggle()
end, { desc = "Dictate Toggle" })

vim.api.nvim_create_user_command("DictateDevInfo", function()
  local lines = {
    ("dictate.nvim dev profile (%s)"):format(mode),
    ("cwd: %s"):format(root),
    ("cmd: %s"):format(table.concat(cmd, " ")),
    ("XDG_CONFIG_HOME: %s"):format(vim.env.XDG_CONFIG_HOME or "<unset>"),
    ("XDG_DATA_HOME: %s"):format(vim.env.XDG_DATA_HOME or "<unset>"),
    "keys: <F9> toggles start/stop/cancel",
    "commands: :DictateStart :DictateStop :DictateToggle :checkhealth dictate",
  }
  vim.notify(table.concat(lines, "\n"), vim.log.levels.INFO, { title = "dictate.nvim" })
end, { desc = "Show dictate.nvim dev profile info" })

vim.schedule(function()
  vim.notify(
    ("dictate.nvim dev profile loaded (%s). Run :DictateDevInfo or :checkhealth dictate."):format(mode),
    vim.log.levels.INFO,
    { title = "dictate.nvim" }
  )
end)
