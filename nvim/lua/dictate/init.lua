local M = {}

---Setup the dictate plugin
---@param opts table|nil Configuration options
---@return boolean success
function M.setup(opts)
  -- Prevent double setup
  if vim.g.dictate_setup_done then
    return true
  end

  -- Initialize configuration
  local ok = require('dictate.config').setup(opts)
  if not ok then
    return false
  end

  -- Mark setup as done (commands are registered in plugin/dictate.lua)
  vim.g.dictate_setup_done = true

  -- Setup keymap if configured
  local cfg = require('dictate.config').get()
  if cfg.keymap then
    vim.keymap.set('n', cfg.keymap, ':DictateToggle<CR>', {
      desc = 'Toggle dictation',
      silent = true,
    })
  end

  return true
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

---@class DictateStatuslineOpts
---@field icons table<string, string>|nil Icons for each state
---@field labels table<string, string>|nil Text labels for each state
---@field show_label boolean|nil Show text label (default: false)

---Get statusline component string
---Returns empty string when stopped, icon/label otherwise
---@param opts DictateStatuslineOpts|nil
---@return string
function M.statusline(opts)
  opts = opts or {}

  local icons = opts.icons or {
    connecting = '󰍰',
    ready = '󰍬',
    recording = '󰍮',
    error = '󰍭',
  }

  local labels = opts.labels or {
    connecting = 'Connecting...',
    ready = 'Ready',
    recording = 'Recording',
    error = 'Error',
  }

  local state = M.get_state()

  -- Don't show anything when stopped
  if state == 'stopped' then
    return ''
  end

  local icon = icons[state] or ''
  local label = labels[state] or state

  if opts.show_label then
    return icon .. ' ' .. label
  end

  return icon
end

---Get lualine component table
---@param opts DictateStatuslineOpts|nil
---@return table
function M.lualine(opts)
  return {
    function()
      return M.statusline(opts)
    end,
    cond = function()
      return M.get_state() ~= 'stopped'
    end,
    color = function()
      local state = M.get_state()
      if state == 'recording' then
        return { fg = '#f38ba8' } -- Red for recording
      elseif state == 'connecting' then
        return { fg = '#f9e2af' } -- Yellow for connecting
      elseif state == 'ready' then
        return { fg = '#a6e3a1' } -- Green for ready
      elseif state == 'error' then
        return { fg = '#fab387' } -- Orange for error
      end
      return nil
    end,
  }
end

return M
