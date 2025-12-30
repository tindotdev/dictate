-- Tests for the say plugin
-- Run with: nvim --headless -c "PlenaryBustedDirectory tests/ {minimal_init = 'tests/minimal_init.lua'}"

describe('say.config', function()
  local config = require('say.config')

  before_each(function()
    -- Reset to defaults before each test
    package.loaded['say.config'] = nil
    config = require('say.config')
  end)

  it('has default values', function()
    config.setup({
      daemon_cmd = { 'echo', 'test' }, -- Provide a valid command
    })
    local cfg = config.get()
    assert.equals('Comment', cfg.ghost_hl)
    assert.equals('<Leader>d', cfg.keymap)
    assert.is_true(cfg.insert_trailing_space)
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
end)

describe('say.ui', function()
  local ui = require('say.ui')
  local ns_id

  before_each(function()
    -- Reset UI state
    package.loaded['say.ui'] = nil
    package.loaded['say.config'] = nil

    -- Setup config with defaults
    local config = require('say.config')
    config.setup({
      daemon_cmd = { 'echo', 'test' },
    })

    ui = require('say.ui')
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

describe('say.job', function()
  local job = require('say.job')

  before_each(function()
    package.loaded['say.job'] = nil
    package.loaded['say.ui'] = nil
    package.loaded['say.config'] = nil

    local config = require('say.config')
    config.setup({
      daemon_cmd = { 'echo', 'test' },
    })

    job = require('say.job')
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
