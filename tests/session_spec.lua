local fixture = vim.fn.fnamemodify("tests/fixtures/fake-dictate.sh", ":p")

describe("dictate session", function()
  local Dictate
  local Session
  local original_notify
  local notifications

  before_each(function()
    package.loaded["dictate"] = nil
    package.loaded["dictate.config"] = nil
    package.loaded["dictate.session"] = nil
    package.loaded["dictate.health"] = nil

    original_notify = vim.notify
    notifications = {}
    vim.notify = function(message, level)
      table.insert(notifications, { message = message, level = level })
    end

    Dictate = require("dictate")
    Session = require("dictate.session")
    Dictate.setup({
      cmd = { fixture },
      disabled_filetypes = {},
      disabled_buftypes = {},
    })

    vim.cmd("enew")
    vim.api.nvim_buf_set_lines(0, 0, -1, false, { "" })
    vim.api.nvim_win_set_cursor(0, { 1, 0 })
  end)

  after_each(function()
    Session.teardown()
    vim.notify = original_notify
    pcall(vim.cmd, "bwipeout!")
  end)

  it("starts and inserts transcript at the original cursor", function()
    vim.env.DICTATE_FIXTURE_SCENARIO = "success"
    vim.env.DICTATE_FIXTURE_TRANSCRIPT = "hello from dictate"

    assert.is_true(Dictate.start())
    assert.equals("recording", Dictate.get_state())

    vim.wait(1000, function()
      return Dictate.get_state() == "recording"
    end)
    assert.is_true(Dictate.stop())

    vim.wait(3000, function()
      return Dictate.get_state() == "idle"
    end)

    local line = vim.api.nvim_buf_get_lines(0, 0, 1, false)[1]
    assert.equals("hello from dictate ", line)
  end)

  it("cancels during transcription", function()
    vim.env.DICTATE_FIXTURE_SCENARIO = "cancel_during_transcribing"
    assert.is_true(Dictate.start())
    assert.is_true(Dictate.stop())

    vim.wait(1000, function()
      return Dictate.get_state() == "transcribing"
    end)
    assert.is_true(Dictate.toggle())

    vim.wait(3000, function()
      return Dictate.get_state() == "idle"
    end)

    local line = vim.api.nvim_buf_get_lines(0, 0, 1, false)[1]
    assert.equals("", line)
    assert.is_true(vim.iter(notifications):any(function(item)
      return item.message == "Dictation cancelled"
    end))
  end)

  it("cancels when stop is pressed again before the phase update arrives", function()
    vim.env.DICTATE_FIXTURE_SCENARIO = "cancel_during_transcribing"
    assert.is_true(Dictate.start())

    vim.wait(100, function()
      return false
    end)

    assert.is_true(Dictate.stop())
    assert.is_true(Dictate.stop())

    assert.is_true(vim.wait(3000, function()
      return Dictate.get_state() == "idle"
    end))
    assert.is_true(vim.iter(notifications):any(function(item)
      return item.message == "Dictation cancelled"
    end))
  end)

  it("falls back to the current buffer when the original buffer is gone", function()
    vim.env.DICTATE_FIXTURE_SCENARIO = "success"
    vim.env.DICTATE_FIXTURE_TRANSCRIPT = "fallback text"

    assert.is_true(Dictate.start())
    local original = vim.api.nvim_get_current_buf()
    vim.cmd("enew")
    vim.cmd("bwipeout! " .. original)

    assert.is_true(Dictate.stop())
    vim.wait(3000, function()
      return Dictate.get_state() == "idle"
    end)

    local line = vim.api.nvim_buf_get_lines(0, 0, 1, false)[1]
    assert.equals("fallback text ", line)
  end)

  it("reassembles partial stdout chunks without inserting extra newlines", function()
    local script = vim.fn.tempname()
    vim.fn.writefile({
      "#!/usr/bin/env bash",
      "set -euo pipefail",
      'for arg in "$@"; do',
      '  if [[ "$arg" == "--help" ]]; then',
      "    printf 'dictate record\\n  --json-events\\n'",
      "    exit 0",
      "  fi",
      "done",
      'printf \'{"event":"session","mode":"record","phase":"recording","stop_after_ms":null}\\n\' >&2',
      "sleep 0.05",
      "printf 'hel'",
      "sleep 0.05",
      "printf 'lo\\n'",
      'printf \'{"event":"result","status":"completed","char_count":5,"copied_to_clipboard":false}\\n\' >&2',
    }, script)
    vim.fn.setfperm(script, "rwx------")

    Dictate.setup({
      cmd = { script },
      disabled_filetypes = {},
      disabled_buftypes = {},
    })

    local ok, err = pcall(function()
      assert.is_true(Dictate.start())

      vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end)

      local line = vim.api.nvim_buf_get_lines(0, 0, 1, false)[1]
      assert.equals("hello ", line)
    end)

    os.remove(script)
    if not ok then
      error(err, 0)
    end
  end)

  it("shows signal delivery errors when uv.kill returns nil", function()
    local original_kill = vim.uv.kill

    vim.env.DICTATE_FIXTURE_SCENARIO = "cancel_during_transcribing"
    assert.is_true(Dictate.start())
    vim.wait(100, function()
      return false
    end)

    vim.uv.kill = function()
      return nil, "no such process", "ESRCH"
    end

    local ok, err = pcall(function()
      assert.is_false(Dictate.stop())
    end)

    vim.uv.kill = original_kill

    if not ok then
      error(err, 0)
    end

    assert.is_true(vim.iter(notifications):any(function(item)
      return item.message == "Failed to signal dictate: no such process (ESRCH)"
    end))
  end)

  it("surfaces the deepest CLI failure cause", function()
    local script = vim.fn.tempname()
    vim.fn.writefile({
      "#!/usr/bin/env bash",
      "set -euo pipefail",
      'for arg in "$@"; do',
      '  if [[ "$arg" == "--help" ]]; then',
      "    printf 'dictate record\\n  --json-events\\n'",
      "    exit 0",
      "  fi",
      "done",
      'printf \'{"event":"session","mode":"record","phase":"recording","stop_after_ms":null}\\n\' >&2',
      "sleep 0.05",
      'printf \'{"event":"result","status":"failed","message":"recording failed","causes":["audio operation failed","GROQ_API_KEY is not set"]}\\n\' >&2',
      "exit 1",
    }, script)
    vim.fn.setfperm(script, "rwx------")

    Dictate.setup({
      cmd = { script },
      disabled_filetypes = {},
      disabled_buftypes = {},
    })

    local ok, err = pcall(function()
      assert.is_true(Dictate.start())
      assert.is_true(vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end))
    end)

    os.remove(script)
    if not ok then
      error(err, 0)
    end

    assert.is_true(vim.iter(notifications):any(function(item)
      return item.message == "recording failed: GROQ_API_KEY is not set"
    end))
  end)
end)
