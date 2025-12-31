local M = {}

---@class DictateConfig
---@field daemon_cmd string[]|nil Command to run the daemon
---@field keymap string|nil Keymap to toggle dictation
---@field ghost_hl string Highlight group for ghost text
---@field insert_trailing_space boolean Add space after inserted text

---@type DictateConfig
local defaults = {
  daemon_cmd = nil, -- Auto-detect
  keymap = nil, -- No default keymap; set in lazy.nvim keys or opts.keymap
  ghost_hl = 'Comment',
  insert_trailing_space = true,
}

---@type DictateConfig
local config = vim.deepcopy(defaults)

---Find the daemon command by looking for dist or dev paths
---@return string[]
local function find_daemon_cmd()
  -- Get the plugin root directory
  local source = debug.getinfo(1, 'S').source:sub(2)
  local plugin_root = vim.fn.fnamemodify(source, ':h:h:h:h')

  local dist = plugin_root .. '/daemon/dist/main.js'
  local dev = plugin_root .. '/daemon/src/main.ts'

  if vim.fn.filereadable(dist) == 1 then
    return { 'bun', dist }
  elseif vim.fn.filereadable(dev) == 1 then
    return { 'bun', dev }
  else
    error('dictate: daemon not found at ' .. dist .. ' or ' .. dev .. '. Run `bun run build` in daemon/')
  end
end

---Setup the plugin configuration
---@param opts DictateConfig|nil
function M.setup(opts)
  config = vim.tbl_deep_extend('force', defaults, opts or {})

  -- Auto-detect daemon path if not specified
  if not config.daemon_cmd then
    local ok, cmd = pcall(find_daemon_cmd)
    if ok then
      config.daemon_cmd = cmd
    else
      vim.notify(cmd, vim.log.levels.ERROR)
    end
  end
end

---Get the current configuration
---@return DictateConfig
function M.get()
  return config
end

return M
