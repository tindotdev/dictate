local M = {}

local health = vim.health

---Check if a command exists
---@param cmd string
---@return boolean
local function command_exists(cmd)
  return vim.fn.executable(cmd) == 1
end

---Get plugin root directory
---@return string
local function get_plugin_root()
  local source = debug.getinfo(1, 'S').source:sub(2)
  return vim.fn.fnamemodify(source, ':h:h:h:h')
end

---@class HealthReporter
---@field start fun(msg: string)
---@field ok fun(msg: string)
---@field warn fun(msg: string)
---@field error fun(msg: string, advice?: string[])
---@field info fun(msg: string)

---Create a standalone reporter that outputs to messages
---@return HealthReporter
local function create_message_reporter()
  local lines = {}
  local function add(icon, msg)
    table.insert(lines, icon .. ' ' .. msg)
  end
  return {
    start = function(msg)
      table.insert(lines, '\n== ' .. msg .. ' ==')
    end,
    ok = function(msg)
      add('OK:', msg)
    end,
    warn = function(msg)
      add('WARN:', msg)
    end,
    error = function(msg, advice)
      add('ERROR:', msg)
      if advice then
        for _, a in ipairs(advice) do
          table.insert(lines, '  - ' .. a)
        end
      end
    end,
    info = function(msg)
      add('INFO:', msg)
    end,
    output = function()
      return table.concat(lines, '\n')
    end,
  }
end

---Run health checks with a given reporter
---@param h HealthReporter
local function run_checks(h)
  h.start('dictate.nvim')

  -- Check Neovim version
  local nvim_version = vim.version()
  if nvim_version.major == 0 and nvim_version.minor < 9 then
    h.error('Neovim 0.9+ required', { 'Upgrade to Neovim 0.9 or later' })
  else
    h.ok('Neovim ' .. nvim_version.major .. '.' .. nvim_version.minor .. '.' .. nvim_version.patch)
  end

  -- Check Bun runtime
  if command_exists('bun') then
    local version = vim.fn.system('bun --version'):gsub('%s+', '')
    h.ok('bun ' .. version .. ' found')
  else
    h.error('bun not found', {
      'Install bun: curl -fsSL https://bun.sh/install | bash',
      'Or visit: https://bun.sh',
    })
  end

  -- Check daemon files (new architecture: dictatectl + daemon)
  local plugin_root = get_plugin_root()
  local dist_dictatectl = plugin_root .. '/daemon/dist/cli/dictatectl.js'
  local dev_dictatectl = plugin_root .. '/daemon/src/cli/dictatectl.ts'
  local dist_daemon = plugin_root .. '/daemon/dist/main.js'
  local dev_daemon = plugin_root .. '/daemon/src/main.ts'

  -- Check dictatectl (local build, dev, or global install)
  if vim.fn.filereadable(dist_dictatectl) == 1 then
    h.ok('dictatectl (local) found at ' .. dist_dictatectl)
  elseif vim.fn.filereadable(dev_dictatectl) == 1 then
    h.ok('dictatectl (dev) found at ' .. dev_dictatectl)
  elseif command_exists('dictatectl') then
    h.ok('dictatectl (global) found in PATH')
  else
    h.warn('dictatectl not found', {
      'Option 1: Build locally - run `cd ' .. plugin_root .. '/daemon && bun run build`',
      'Option 2: Install globally - run `npm install -g @tindotdev/dictate`',
    })
  end

  -- Check daemon
  if vim.fn.filereadable(dist_daemon) == 1 then
    h.ok('daemon found at ' .. dist_daemon)
  elseif vim.fn.filereadable(dev_daemon) == 1 then
    h.ok('daemon (dev) found at ' .. dev_daemon)
  else
    h.error('daemon not found', {
      'Run: cd ' .. plugin_root .. '/daemon && bun run build',
      'Expected: ' .. dist_daemon,
    })
  end

  -- Check OPENAI_API_KEY
  local api_key = vim.env.OPENAI_API_KEY
  if api_key and api_key ~= '' then
    local masked = api_key:sub(1, 7) .. '...' .. api_key:sub(-4)
    h.ok('OPENAI_API_KEY is set (' .. masked .. ')')
  else
    -- Check daemon/.env file
    local env_file = plugin_root .. '/daemon/.env'
    if vim.fn.filereadable(env_file) == 1 then
      h.ok('OPENAI_API_KEY found in ' .. env_file)
    else
      h.error('OPENAI_API_KEY not set', {
        'Set environment variable: export OPENAI_API_KEY=sk-...',
        'Or create ' .. env_file .. ' with: OPENAI_API_KEY=sk-...',
      })
    end
  end

  -- Check PipeWire / pw-cat
  if command_exists('pw-cat') then
    h.ok('pw-cat found (PipeWire audio)')
  else
    h.error('pw-cat not found', {
      'Install PipeWire: sudo apt install pipewire pipewire-audio-client-libraries',
      'Or: sudo dnf install pipewire pipewire-utils',
      'pw-cat is required for audio capture',
    })
  end

  -- Check optional: nvim-notify
  local has_notify = pcall(require, 'notify')
  if has_notify then
    h.ok('nvim-notify available (enhanced notifications)')
  else
    h.info('nvim-notify not installed (optional, for enhanced notifications)')
  end

  -- Check plugin configuration
  local ok, cfg = pcall(function()
    return require('dictate.config').get()
  end)
  if ok and cfg then
    h.ok('configuration loaded')

    if cfg.daemon_cmd then
      h.ok('daemon_cmd: ' .. table.concat(cfg.daemon_cmd, ' '))
    else
      h.warn('daemon_cmd not configured (will auto-detect)')
    end

    if cfg.debug then
      h.info('debug mode is enabled')
    end

    if #cfg.disabled_filetypes > 0 then
      h.info('disabled filetypes: ' .. table.concat(cfg.disabled_filetypes, ', '))
    end
  else
    h.warn('configuration not loaded (run setup() first)')
  end

  -- Check systemd socket activation (optional)
  h.start('systemd integration (optional)')

  if command_exists('systemctl') then
    -- Check if user session is available
    local socket_status = vim.fn.system('systemctl --user is-active dictate.socket 2>/dev/null'):gsub('%s+', '')
    local service_status = vim.fn.system('systemctl --user is-active dictate.service 2>/dev/null'):gsub('%s+', '')

    if socket_status == 'active' then
      h.ok('dictate.socket is active (socket activation enabled)')
    elseif socket_status == 'inactive' then
      h.info('dictate.socket is inactive')
    else
      h.info('dictate.socket not installed (systemd mode is optional)')
    end

    if service_status == 'active' then
      h.ok('dictate.service is running')
    elseif service_status == 'inactive' then
      h.info('dictate.service is not running (starts on demand with socket activation)')
    else
      h.info('dictate.service not installed')
    end

    -- Check socket file
    local xdg_runtime = vim.env.XDG_RUNTIME_DIR
    if xdg_runtime then
      local socket_path = xdg_runtime .. '/dictate/dictate.sock'
      if vim.fn.filereadable(socket_path) == 1 or vim.fn.isdirectory(vim.fn.fnamemodify(socket_path, ':h')) == 1 then
        h.ok('socket directory exists: ' .. vim.fn.fnamemodify(socket_path, ':h'))
      end
    end
  else
    h.info('systemctl not found (systemd mode not available)')
  end

  -- Test daemon connectivity
  h.start('daemon connectivity')

  local daemon_cmd = nil
  if ok and cfg and cfg.daemon_cmd then
    daemon_cmd = cfg.daemon_cmd
  elseif vim.fn.filereadable(dist_dictatectl) == 1 then
    daemon_cmd = { 'bun', dist_dictatectl }
  elseif vim.fn.filereadable(dev_dictatectl) == 1 then
    daemon_cmd = { 'bun', dev_dictatectl }
  elseif command_exists('dictatectl') then
    daemon_cmd = { 'dictatectl' }
  elseif vim.fn.filereadable(dist_daemon) == 1 then
    daemon_cmd = { 'bun', dist_daemon }
  elseif vim.fn.filereadable(dev_daemon) == 1 then
    daemon_cmd = { 'bun', dev_daemon }
  end

  if daemon_cmd then
    h.ok('daemon command: ' .. table.concat(daemon_cmd, ' '))
  else
    h.error('cannot determine daemon command', {
      'Option 1: Build locally - run `cd ' .. plugin_root .. '/daemon && bun run build`',
      'Option 2: Install globally - run `npm install -g @tindotdev/dictate`',
    })
  end
end

---Run health checks for :checkhealth dictate (uses vim.health buffer)
function M.check()
  run_checks(health)
end

---Run health checks and print to messages (for :DictateHealth command)
function M.check_standalone()
  local reporter = create_message_reporter()
  run_checks(reporter)
  print(reporter.output())
end

return M
