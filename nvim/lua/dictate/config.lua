local M = {}

---@class DictateConfig
---@field daemon_cmd string[]|nil Command to run the daemon
---@field use_global_daemon boolean Use globally installed daemon (npm/bun) instead of local build
---@field keymap string|nil Keymap to toggle dictation
---@field ghost_hl string Highlight group for ghost text
---@field insert_trailing_space boolean Add space after inserted text
---@field on_start fun()|nil Callback when dictation starts
---@field on_stop fun()|nil Callback when dictation stops
---@field disabled_filetypes string[] Filetypes where dictation is disabled (replaces defaults)
---@field extra_disabled_filetypes string[]|nil Extra filetypes to disable (adds to defaults)
---@field disabled_buftypes string[] Buffer types where dictation is disabled (replaces defaults)
---@field extra_disabled_buftypes string[]|nil Extra buffer types to disable (adds to defaults)
---@field notify_backend 'native'|'nvim-notify'|'auto' Notification backend
---@field debug boolean Enable debug logging

---@type DictateConfig
local defaults = {
  daemon_cmd = nil, -- Auto-detect
  use_global_daemon = false, -- Prefer local build; set true to use npm/bun global install
  keymap = nil, -- No default keymap; set in lazy.nvim keys or opts.keymap
  ghost_hl = 'Comment',
  insert_trailing_space = true,
  on_start = nil,
  on_stop = nil,
  disabled_filetypes = { 'help', 'qf', 'netrw', 'NvimTree', 'neo-tree', 'lazy', 'mason', 'TelescopePrompt' },
  disabled_buftypes = { 'terminal', 'nofile', 'prompt', 'quickfix' },
  notify_backend = 'auto', -- 'native', 'nvim-notify', or 'auto'
  debug = false,
}

---@class ConfigValidationError
---@field field string
---@field message string
---@field got any

---Validate a single config field
---@param field string
---@param value any
---@param expected_type string
---@param optional boolean|nil
---@return ConfigValidationError|nil
local function validate_type(field, value, expected_type, optional)
  if value == nil then
    if optional then
      return nil
    end
    return { field = field, message = 'required field is missing', got = value }
  end
  if type(value) ~= expected_type then
    return { field = field, message = 'expected ' .. expected_type, got = type(value) }
  end
  return nil
end

---Validate config options and return errors
---@param opts table
---@return ConfigValidationError[]
local function validate_config(opts)
  local errors = {}

  -- daemon_cmd: string[] or nil
  if opts.daemon_cmd ~= nil then
    if type(opts.daemon_cmd) ~= 'table' then
      table.insert(
        errors,
        { field = 'daemon_cmd', message = 'expected string[] (array of strings)', got = type(opts.daemon_cmd) }
      )
    else
      for i, v in ipairs(opts.daemon_cmd) do
        if type(v) ~= 'string' then
          table.insert(errors, { field = 'daemon_cmd[' .. i .. ']', message = 'expected string', got = type(v) })
        end
      end
    end
  end

  -- keymap: string or nil
  if opts.keymap ~= nil then
    local err = validate_type('keymap', opts.keymap, 'string', true)
    if err then
      table.insert(errors, err)
    end
  end

  -- ghost_hl: string
  if opts.ghost_hl ~= nil then
    local err = validate_type('ghost_hl', opts.ghost_hl, 'string', true)
    if err then
      table.insert(errors, err)
    end
  end

  -- insert_trailing_space: boolean
  if opts.insert_trailing_space ~= nil then
    local err = validate_type('insert_trailing_space', opts.insert_trailing_space, 'boolean', true)
    if err then
      table.insert(errors, err)
    end
  end

  -- on_start: function or nil
  if opts.on_start ~= nil then
    local err = validate_type('on_start', opts.on_start, 'function', true)
    if err then
      table.insert(errors, err)
    end
  end

  -- on_stop: function or nil
  if opts.on_stop ~= nil then
    local err = validate_type('on_stop', opts.on_stop, 'function', true)
    if err then
      table.insert(errors, err)
    end
  end

  -- disabled_filetypes: string[]
  if opts.disabled_filetypes ~= nil then
    if type(opts.disabled_filetypes) ~= 'table' then
      table.insert(errors, {
        field = 'disabled_filetypes',
        message = 'expected string[] (array of strings)',
        got = type(opts.disabled_filetypes),
      })
    else
      for i, v in ipairs(opts.disabled_filetypes) do
        if type(v) ~= 'string' then
          table.insert(
            errors,
            { field = 'disabled_filetypes[' .. i .. ']', message = 'expected string', got = type(v) }
          )
        end
      end
    end
  end

  -- disabled_buftypes: string[]
  if opts.disabled_buftypes ~= nil then
    if type(opts.disabled_buftypes) ~= 'table' then
      table.insert(errors, {
        field = 'disabled_buftypes',
        message = 'expected string[] (array of strings)',
        got = type(opts.disabled_buftypes),
      })
    else
      for i, v in ipairs(opts.disabled_buftypes) do
        if type(v) ~= 'string' then
          table.insert(errors, { field = 'disabled_buftypes[' .. i .. ']', message = 'expected string', got = type(v) })
        end
      end
    end
  end

  -- extra_disabled_filetypes: string[] (optional, appends to defaults)
  if opts.extra_disabled_filetypes ~= nil then
    if type(opts.extra_disabled_filetypes) ~= 'table' then
      table.insert(errors, {
        field = 'extra_disabled_filetypes',
        message = 'expected string[] (array of strings)',
        got = type(opts.extra_disabled_filetypes),
      })
    else
      for i, v in ipairs(opts.extra_disabled_filetypes) do
        if type(v) ~= 'string' then
          table.insert(
            errors,
            { field = 'extra_disabled_filetypes[' .. i .. ']', message = 'expected string', got = type(v) }
          )
        end
      end
    end
  end

  -- extra_disabled_buftypes: string[] (optional, appends to defaults)
  if opts.extra_disabled_buftypes ~= nil then
    if type(opts.extra_disabled_buftypes) ~= 'table' then
      table.insert(errors, {
        field = 'extra_disabled_buftypes',
        message = 'expected string[] (array of strings)',
        got = type(opts.extra_disabled_buftypes),
      })
    else
      for i, v in ipairs(opts.extra_disabled_buftypes) do
        if type(v) ~= 'string' then
          table.insert(
            errors,
            { field = 'extra_disabled_buftypes[' .. i .. ']', message = 'expected string', got = type(v) }
          )
        end
      end
    end
  end

  -- notify_backend: 'native' | 'nvim-notify' | 'auto'
  if opts.notify_backend ~= nil then
    local valid_backends = { native = true, ['nvim-notify'] = true, auto = true }
    if type(opts.notify_backend) ~= 'string' then
      table.insert(errors, {
        field = 'notify_backend',
        message = "expected 'native', 'nvim-notify', or 'auto'",
        got = type(opts.notify_backend),
      })
    elseif not valid_backends[opts.notify_backend] then
      table.insert(
        errors,
        { field = 'notify_backend', message = "expected 'native', 'nvim-notify', or 'auto'", got = opts.notify_backend }
      )
    end
  end

  -- debug: boolean
  if opts.debug ~= nil then
    local err = validate_type('debug', opts.debug, 'boolean', true)
    if err then
      table.insert(errors, err)
    end
  end

  -- use_global_daemon: boolean
  if opts.use_global_daemon ~= nil then
    local err = validate_type('use_global_daemon', opts.use_global_daemon, 'boolean', true)
    if err then
      table.insert(errors, err)
    end
  end

  return errors
end

---Format validation errors into a readable message
---@param errors ConfigValidationError[]
---@return string
local function format_errors(errors)
  local lines = { 'dictate.nvim: configuration errors:' }
  for _, err in ipairs(errors) do
    local got_str = type(err.got) == 'string' and ('"' .. err.got .. '"') or tostring(err.got)
    table.insert(lines, string.format('  - %s: %s (got %s)', err.field, err.message, got_str))
  end
  return table.concat(lines, '\n')
end

---@type DictateConfig
local config = vim.deepcopy(defaults)

---Find the dictatectl command using fallback chain:
---1. Relative path from plugin root (local build or dev) - skipped if use_global is true
---2. System PATH lookup (global npm/bun install)
---@param use_global boolean Skip local paths and use global install
---@return string[]
local function find_daemon_cmd(use_global)
  -- Get the plugin root directory
  local source = debug.getinfo(1, 'S').source:sub(2)
  local plugin_root = vim.fn.fnamemodify(source, ':h:h:h:h')

  if not use_global then
    -- Priority 1: Local build (dist)
    local dist_dictatectl = plugin_root .. '/daemon/dist/cli/dictatectl.js'
    if vim.fn.filereadable(dist_dictatectl) == 1 then
      return { 'bun', dist_dictatectl }
    end

    -- Priority 2: Local dev (src)
    local dev_dictatectl = plugin_root .. '/daemon/src/cli/dictatectl.ts'
    if vim.fn.filereadable(dev_dictatectl) == 1 then
      return { 'bun', dev_dictatectl }
    end
  end

  -- Priority 3 (or 1 if use_global): Global install (dictatectl in PATH)
  if vim.fn.executable('dictatectl') == 1 then
    return { 'dictatectl' }
  end

  -- Not found - provide helpful error
  if use_global then
    error('dictate: dictatectl not found in PATH.\n  Run `npm install -g @tindotdev/dictate` to install globally.')
  else
    error(
      'dictate: dictatectl not found.\n'
        .. '  Option 1: Build locally - run `cd '
        .. plugin_root
        .. '/daemon && bun run build`\n'
        .. '  Option 2: Install globally - run `npm install -g @tindotdev/dictate`'
    )
  end
end

---Setup the plugin configuration
---@param opts DictateConfig|nil
---@return boolean success
function M.setup(opts)
  opts = opts or {}

  -- Validate config before applying
  local errors = validate_config(opts)
  if #errors > 0 then
    vim.notify(format_errors(errors), vim.log.levels.ERROR)
    return false
  end

  local extra_filetypes = opts.extra_disabled_filetypes
  local extra_buftypes = opts.extra_disabled_buftypes

  -- Create clean opts for merge (without extra_* fields)
  local merge_opts = {}
  for k, v in pairs(opts) do
    if k ~= 'extra_disabled_filetypes' and k ~= 'extra_disabled_buftypes' then
      merge_opts[k] = v
    end
  end

  -- Merge with defaults
  config = vim.tbl_deep_extend('force', vim.deepcopy(defaults), merge_opts)

  -- Handle extra_* options: always append to the final disabled list
  -- This works whether user specified a custom base or uses defaults
  if extra_filetypes then
    for _, ft in ipairs(extra_filetypes) do
      table.insert(config.disabled_filetypes, ft)
    end
  end

  if extra_buftypes then
    for _, bt in ipairs(extra_buftypes) do
      table.insert(config.disabled_buftypes, bt)
    end
  end

  -- Sync debug setting to global var for job.lua
  vim.g.dictate_debug = config.debug

  -- Auto-detect daemon path if not specified
  if not config.daemon_cmd then
    local ok, cmd = pcall(find_daemon_cmd, config.use_global_daemon)
    if ok then
      config.daemon_cmd = cmd
    else
      vim.notify(cmd, vim.log.levels.ERROR)
      return false
    end
  end

  return true
end

---Get the current configuration
---@return DictateConfig
function M.get()
  return config
end

---Check if dictation is allowed in the given buffer
---@param bufnr integer|nil Buffer number (default: current buffer)
---@return boolean allowed
---@return string|nil reason Why dictation is disabled (if not allowed)
function M.is_buffer_allowed(bufnr)
  bufnr = bufnr or vim.api.nvim_get_current_buf()

  local filetype = vim.bo[bufnr].filetype
  local buftype = vim.bo[bufnr].buftype

  -- Check disabled filetypes
  for _, ft in ipairs(config.disabled_filetypes) do
    if filetype == ft then
      return false, 'filetype "' .. filetype .. '" is disabled'
    end
  end

  -- Check disabled buftypes
  for _, bt in ipairs(config.disabled_buftypes) do
    if buftype == bt then
      return false, 'buftype "' .. buftype .. '" is disabled'
    end
  end

  return true, nil
end

---Get the defaults (for testing/reference)
---@return DictateConfig
function M.get_defaults()
  return vim.deepcopy(defaults)
end

return M
