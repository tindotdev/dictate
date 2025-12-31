-- Tests for the dictate plugin
-- Run with: nvim --headless -c "PlenaryBustedDirectory tests/ {minimal_init = 'tests/minimal_init.lua'}"

describe('dictate.config', function()
  local config = require('dictate.config')

  before_each(function()
    -- Reset to defaults before each test
    package.loaded['dictate.config'] = nil
    package.loaded['dictate'] = nil
    vim.g.dictate_setup_done = false
    config = require('dictate.config')
  end)

  it('has default values', function()
    config.setup({
      daemon_cmd = { 'echo', 'test' }, -- Provide a valid command
    })
    local cfg = config.get()
    assert.equals('Comment', cfg.ghost_hl)
    assert.is_nil(cfg.keymap) -- No default keymap
    assert.is_true(cfg.insert_trailing_space)
    -- New M2 defaults
    assert.is_nil(cfg.on_start)
    assert.is_nil(cfg.on_stop)
    assert.is_table(cfg.disabled_filetypes)
    assert.is_table(cfg.disabled_buftypes)
    assert.equals('auto', cfg.notify_backend)
    assert.is_false(cfg.debug)
  end)

  it('merges user options', function()
    config.setup({
      daemon_cmd = { 'echo', 'test' },
      ghost_hl = 'Special',
      keymap = '<Leader>s',
      insert_trailing_space = false,
    })
    local cfg = config.get()
    assert.equals('Special', cfg.ghost_hl)
    assert.equals('<Leader>s', cfg.keymap)
    assert.is_false(cfg.insert_trailing_space)
  end)

  it('accepts custom daemon_cmd', function()
    config.setup({
      daemon_cmd = { 'node', '/path/to/daemon.js' },
    })
    local cfg = config.get()
    assert.same({ 'node', '/path/to/daemon.js' }, cfg.daemon_cmd)
  end)

  it('validates config types', function()
    -- Invalid ghost_hl should fail
    local result = config.setup({
      daemon_cmd = { 'echo', 'test' },
      ghost_hl = 123, -- Should be string
    })
    assert.is_false(result)
  end)

  it('validates callback types', function()
    local result = config.setup({
      daemon_cmd = { 'echo', 'test' },
      on_start = 'not a function',
    })
    assert.is_false(result)
  end)

  it('accepts valid callbacks', function()
    local called = false
    local result = config.setup({
      daemon_cmd = { 'echo', 'test' },
      on_start = function()
        called = true
      end,
    })
    assert.is_true(result)
  end)

  it('checks buffer is allowed', function()
    config.setup({
      daemon_cmd = { 'echo', 'test' },
      disabled_filetypes = { 'help', 'lua' },
      disabled_buftypes = { 'terminal' },
    })

    -- Create a normal buffer
    vim.cmd('enew!')
    vim.bo.filetype = 'python'
    vim.bo.buftype = ''

    local allowed, reason = config.is_buffer_allowed()
    assert.is_true(allowed)
    assert.is_nil(reason)

    -- Set disabled filetype
    vim.bo.filetype = 'lua'
    allowed, reason = config.is_buffer_allowed()
    assert.is_false(allowed)
    assert.is_not_nil(reason)

    vim.cmd('bwipeout!')
  end)

  it('appends with extra_disabled_filetypes', function()
    config.setup({
      daemon_cmd = { 'echo', 'test' },
      extra_disabled_filetypes = { 'markdown', 'python' },
    })
    local cfg = config.get()
    -- Should have defaults plus the added ones
    assert.is_true(vim.tbl_contains(cfg.disabled_filetypes, 'help')) -- default
    assert.is_true(vim.tbl_contains(cfg.disabled_filetypes, 'markdown')) -- added
    assert.is_true(vim.tbl_contains(cfg.disabled_filetypes, 'python')) -- added
  end)

  it('backward compat: appends with disabled_filetypes_add', function()
    config.setup({
      daemon_cmd = { 'echo', 'test' },
      disabled_filetypes_add = { 'markdown' },
    })
    local cfg = config.get()
    assert.is_true(vim.tbl_contains(cfg.disabled_filetypes, 'help')) -- default
    assert.is_true(vim.tbl_contains(cfg.disabled_filetypes, 'markdown')) -- added
  end)

  it('replaces with disabled_filetypes (not extra_)', function()
    config.setup({
      daemon_cmd = { 'echo', 'test' },
      disabled_filetypes = { 'custom' },
    })
    local cfg = config.get()
    -- Should only have the custom one, not defaults
    assert.equals(1, #cfg.disabled_filetypes)
    assert.equals('custom', cfg.disabled_filetypes[1])
  end)
end)

describe('dictate.ui', function()
  local ui = require('dictate.ui')
  local ns_id

  before_each(function()
    -- Reset UI state
    package.loaded['dictate.ui'] = nil
    package.loaded['dictate.config'] = nil
    package.loaded['dictate'] = nil
    package.loaded['dictate.notify'] = nil
    vim.g.dictate_setup_done = false

    -- Setup config with defaults
    local config = require('dictate.config')
    config.setup({
      daemon_cmd = { 'echo', 'test' },
    })

    ui = require('dictate.ui')
    ns_id = ui.get_namespace()

    -- Create a test buffer
    vim.cmd('enew!')
    vim.api.nvim_buf_set_lines(0, 0, -1, false, { '' })
    vim.api.nvim_win_set_cursor(0, { 1, 0 })
  end)

  after_each(function()
    ui.clear_all()
    vim.cmd('bwipeout!')
  end)

  it('creates ghost text on delta', function()
    ui.on_speech_started('item_1')
    ui.on_delta('item_1', 'hello')

    local marks = vim.api.nvim_buf_get_extmarks(0, ns_id, 0, -1, { details = true })
    assert.equals(1, #marks)

    local details = marks[1][4]
    assert.is_not_nil(details.virt_text)
    assert.equals('hello', details.virt_text[1][1])
  end)

  it('updates ghost text on subsequent deltas', function()
    ui.on_speech_started('item_1')
    ui.on_delta('item_1', 'hello')
    ui.on_delta('item_1', 'hello world')

    local marks = vim.api.nvim_buf_get_extmarks(0, ns_id, 0, -1, { details = true })
    assert.equals(1, #marks)

    local details = marks[1][4]
    assert.equals('hello world', details.virt_text[1][1])
  end)

  it('inserts final text and clears ghost', function()
    ui.on_speech_started('item_1')
    ui.on_delta('item_1', 'hello')
    ui.on_final('item_1', 'Hello!')

    -- Ghost should be cleared
    local marks = vim.api.nvim_buf_get_extmarks(0, ns_id, 0, -1, {})
    assert.equals(0, #marks)

    -- Text should be inserted (with trailing space by default)
    local lines = vim.api.nvim_buf_get_lines(0, 0, -1, false)
    assert.equals('Hello! ', lines[1])
  end)

  it('clears all ghost text', function()
    ui.on_speech_started('item_1')
    ui.on_delta('item_1', 'hello')
    ui.on_speech_started('item_2')
    ui.on_delta('item_2', 'world')

    ui.clear_all()

    local marks = vim.api.nvim_buf_get_extmarks(0, ns_id, 0, -1, {})
    assert.equals(0, #marks)
  end)
end)

describe('dictate.job', function()
  local job = require('dictate.job')

  before_each(function()
    package.loaded['dictate.job'] = nil
    package.loaded['dictate.ui'] = nil
    package.loaded['dictate.config'] = nil
    package.loaded['dictate'] = nil
    package.loaded['dictate.notify'] = nil
    vim.g.dictate_setup_done = false

    local config = require('dictate.config')
    config.setup({
      daemon_cmd = { 'echo', 'test' },
    })

    job = require('dictate.job')
  end)

  after_each(function()
    job.stop()
  end)

  it('starts in stopped state', function()
    assert.equals('stopped', job.get_state())
    assert.is_false(job.is_running())
  end)

  it('handles status message', function()
    job.handle_message({ type = 'status', state = 'ready' })
    assert.equals('ready', job.get_state())
  end)

  it('handles error message', function()
    -- Should not throw, just notify
    job.handle_message({ type = 'error', code = 'TEST', message = 'test error' })
  end)
end)

describe('dictate.statusline', function()
  local dictate

  before_each(function()
    package.loaded['dictate'] = nil
    package.loaded['dictate.job'] = nil
    package.loaded['dictate.config'] = nil
    package.loaded['dictate.ui'] = nil
    package.loaded['dictate.notify'] = nil

    local config = require('dictate.config')
    config.setup({
      daemon_cmd = { 'echo', 'test' },
    })

    dictate = require('dictate')
  end)

  it('returns empty string when stopped', function()
    assert.equals('', dictate.statusline())
  end)

  it('returns icon for non-stopped states', function()
    local job = require('dictate.job')
    job.handle_message({ type = 'status', state = 'ready' })
    assert.is_not.equals('', dictate.statusline())
  end)

  it('returns lualine component table', function()
    local component = dictate.lualine()
    assert.is_table(component)
    assert.is_function(component[1])
    assert.is_function(component.cond)
    assert.is_function(component.color)
  end)
end)

describe('dictate.notify', function()
  local notify_mod

  before_each(function()
    package.loaded['dictate.notify'] = nil
    package.loaded['dictate.config'] = nil

    local config = require('dictate.config')
    config.setup({
      daemon_cmd = { 'echo', 'test' },
      debug = true,
    })

    notify_mod = require('dictate.notify')
  end)

  it('has notify functions', function()
    assert.is_function(notify_mod.notify)
    assert.is_function(notify_mod.info)
    assert.is_function(notify_mod.warn)
    assert.is_function(notify_mod.error)
    assert.is_function(notify_mod.debug)
  end)
end)
