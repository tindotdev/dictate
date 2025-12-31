local M = {}
local config = require('dictate.config')

---@type table|nil Cached nvim-notify module
local nvim_notify = nil

---@type boolean
local nvim_notify_checked = false

---Check if nvim-notify is available
---@return table|nil
local function get_nvim_notify()
  if nvim_notify_checked then
    return nvim_notify
  end

  nvim_notify_checked = true
  local ok, notify = pcall(require, 'notify')
  if ok then
    nvim_notify = notify
  end
  return nvim_notify
end

---Get the notification function based on config
---@return fun(msg: string, level: integer, opts: table|nil)
local function get_notify_fn()
  local cfg = config.get()
  local backend = cfg.notify_backend

  if backend == 'nvim-notify' then
    local notify = get_nvim_notify()
    if notify then
      return function(msg, level, opts)
        opts = opts or {}
        opts.title = opts.title or 'dictate.nvim'
        notify(msg, level, opts)
      end
    end
    -- Fall back to native if nvim-notify not available
  elseif backend == 'auto' then
    local notify = get_nvim_notify()
    if notify then
      return function(msg, level, opts)
        opts = opts or {}
        opts.title = opts.title or 'dictate.nvim'
        notify(msg, level, opts)
      end
    end
  end

  -- Native vim.notify
  return function(msg, level, _)
    vim.notify(msg, level)
  end
end

---Send a notification
---@param msg string
---@param level integer|nil vim.log.levels value (default: INFO)
---@param opts table|nil Additional options (title, timeout, etc.)
function M.notify(msg, level, opts)
  level = level or vim.log.levels.INFO
  local notify_fn = get_notify_fn()
  notify_fn(msg, level, opts)
end

---Send an info notification
---@param msg string
---@param opts table|nil
function M.info(msg, opts)
  M.notify(msg, vim.log.levels.INFO, opts)
end

---Send a warning notification
---@param msg string
---@param opts table|nil
function M.warn(msg, opts)
  M.notify(msg, vim.log.levels.WARN, opts)
end

---Send an error notification
---@param msg string
---@param opts table|nil
function M.error(msg, opts)
  M.notify(msg, vim.log.levels.ERROR, opts)
end

---Send a debug notification (only if debug mode is enabled)
---@param msg string
---@param opts table|nil
function M.debug(msg, opts)
  local cfg = config.get()
  if cfg.debug then
    M.notify('[debug] ' .. msg, vim.log.levels.DEBUG, opts)
  end
end

return M
