local fixture = vim.fn.fnamemodify("tests/fixtures/fake-dictate.sh", ":p")

local function write_command_logging_script(argv_log)
  local script = vim.fn.tempname()
  vim.fn.writefile({
    "#!/usr/bin/env bash",
    "set -euo pipefail",
    'if [[ "${1:-}" == "record" && "${2:-}" == "--help" ]]; then',
    "  printf 'dictate record\\n  --json-events\\n  --save-last-audio\\n'",
    "  exit 0",
    "fi",
    'if [[ "${1:-}" == "retry" && "${2:-}" == "--help" ]]; then',
    "  printf 'dictate retry\\n  --json-events\\n'",
    "  exit 0",
    "fi",
    'printf \'%s\\n\' "$*" > "' .. argv_log .. '"',
    'if [[ "${1:-}" == "retry" ]]; then',
    '  printf \'{"event":"session","mode":"retry","phase":"retrying","stop_after_ms":null}\\n\' >&2',
    "  printf 'retry logged\\n'",
    '  printf \'{"event":"result","status":"completed","char_count":12,"copied_to_clipboard":false}\\n\' >&2',
    "  exit 0",
    "fi",
    'phase="recording"',
    "trap 'phase=\"transcribing\"' USR1",
    'printf \'{"event":"session","mode":"record","phase":"recording","stop_after_ms":null}\\n\' >&2',
    'while [[ "$phase" == "recording" ]]; do',
    "  sleep 0.05",
    "done",
    'printf \'{"event":"phase","phase":"transcribing","chunk_count":1,"model":null}\\n\' >&2',
    "printf 'record logged\\n'",
    'printf \'{"event":"result","status":"completed","char_count":13,"copied_to_clipboard":false}\\n\' >&2',
  }, script)
  vim.fn.setfperm(script, "rwx------")
  return script
end

describe("dictate session", function()
  local Dictate
  local Session
  local original_notify
  local notifications

  before_each(function()
    package.loaded["dictate"] = nil
    package.loaded["dictate.capabilities"] = nil
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

  it("saves the last audio for plugin-managed recordings", function()
    local script = vim.fn.tempname()
    local argv_log = vim.fn.tempname()
    vim.fn.writefile({
      "#!/usr/bin/env bash",
      "set -euo pipefail",
      'for arg in "$@"; do',
      '  if [[ "$arg" == "--help" ]]; then',
      "    printf 'dictate record\\n  --json-events\\n  --save-last-audio\\n'",
      "    printf 'dictate retry\\n  --json-events\\n'",
      "    exit 0",
      "  fi",
      "done",
      'printf \'%s\\n\' "$*" > "' .. argv_log .. '"',
      'phase="recording"',
      "trap 'phase=\"transcribing\"' USR1",
      'printf \'{"event":"session","mode":"record","phase":"recording","stop_after_ms":null}\\n\' >&2',
      'while [[ "$phase" == "recording" ]]; do',
      "  sleep 0.05",
      "done",
      'printf \'{"event":"phase","phase":"transcribing","chunk_count":1,"model":null}\\n\' >&2',
      "printf 'logged args\\n'",
      'printf \'{"event":"result","status":"completed","char_count":11,"copied_to_clipboard":false}\\n\' >&2',
    }, script)
    vim.fn.setfperm(script, "rwx------")

    Dictate.setup({
      cmd = { script },
      disabled_filetypes = {},
      disabled_buftypes = {},
    })

    local ok, err = pcall(function()
      assert.is_true(Dictate.start())
      assert.is_true(Dictate.stop())
      assert.is_true(vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end))
    end)

    local argv = vim.fn.readfile(argv_log)
    os.remove(script)
    os.remove(argv_log)

    if not ok then
      error(err, 0)
    end

    assert.equals("record --save-last-audio --format text --json-events --no-clipboard", argv[1])
  end)

  it("does not duplicate an explicit save-last-audio flag", function()
    local script = vim.fn.tempname()
    local argv_log = vim.fn.tempname()
    vim.fn.writefile({
      "#!/usr/bin/env bash",
      "set -euo pipefail",
      'for arg in "$@"; do',
      '  if [[ "$arg" == "--help" ]]; then',
      "    printf 'dictate record\\n  --json-events\\n  --save-last-audio\\n'",
      "    printf 'dictate retry\\n  --json-events\\n'",
      "    exit 0",
      "  fi",
      "done",
      'printf \'%s\\n\' "$*" > "' .. argv_log .. '"',
      'phase="recording"',
      "trap 'phase=\"transcribing\"' USR1",
      'printf \'{"event":"session","mode":"record","phase":"recording","stop_after_ms":null}\\n\' >&2',
      'while [[ "$phase" == "recording" ]]; do',
      "  sleep 0.05",
      "done",
      'printf \'{"event":"phase","phase":"transcribing","chunk_count":1,"model":null}\\n\' >&2',
      "printf 'logged args\\n'",
      'printf \'{"event":"result","status":"completed","char_count":11,"copied_to_clipboard":false}\\n\' >&2',
    }, script)
    vim.fn.setfperm(script, "rwx------")

    Dictate.setup({
      cmd = { script },
      args = { "--save-last-audio", "--device=USB Mic" },
      disabled_filetypes = {},
      disabled_buftypes = {},
    })

    local ok, err = pcall(function()
      assert.is_true(Dictate.start())
      assert.is_true(Dictate.stop())
      assert.is_true(vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end))
    end)

    local argv = vim.fn.readfile(argv_log)
    os.remove(script)
    os.remove(argv_log)

    if not ok then
      error(err, 0)
    end

    assert.equals("record --save-last-audio --device=USB Mic --format text --json-events --no-clipboard", argv[1])
  end)

  it("does not force save-last-audio when record help omits it", function()
    local script = vim.fn.tempname()
    local argv_log = vim.fn.tempname()
    vim.fn.writefile({
      "#!/usr/bin/env bash",
      "set -euo pipefail",
      'if [[ "${1:-}" == "record" && "${2:-}" == "--help" ]]; then',
      "  printf 'dictate record\\n  --json-events\\n'",
      "  exit 0",
      "fi",
      'if [[ "${1:-}" == "retry" && "${2:-}" == "--help" ]]; then',
      "  printf 'dictate retry\\n  --json-events\\n'",
      "  exit 0",
      "fi",
      'printf \'%s\\n\' "$*" > "' .. argv_log .. '"',
      'phase="recording"',
      "trap 'phase=\"transcribing\"' USR1",
      'printf \'{"event":"session","mode":"record","phase":"recording","stop_after_ms":null}\\n\' >&2',
      'while [[ "$phase" == "recording" ]]; do',
      "  sleep 0.05",
      "done",
      'printf \'{"event":"phase","phase":"transcribing","chunk_count":1,"model":null}\\n\' >&2',
      "printf 'logged args\\n'",
      'printf \'{"event":"result","status":"completed","char_count":11,"copied_to_clipboard":false}\\n\' >&2',
    }, script)
    vim.fn.setfperm(script, "rwx------")

    Dictate.setup({
      cmd = { script },
      disabled_filetypes = {},
      disabled_buftypes = {},
    })

    local ok, err = pcall(function()
      assert.is_true(Dictate.start())
      assert.is_true(Dictate.stop())
      assert.is_true(vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end))
    end)

    local argv = vim.fn.readfile(argv_log)
    os.remove(script)
    os.remove(argv_log)

    if not ok then
      error(err, 0)
    end

    assert.equals("record --format text --json-events --no-clipboard", argv[1])
  end)

  it("does not probe record help on start after setup warms the cache", function()
    local script = vim.fn.tempname()
    local argv_log = vim.fn.tempname()
    vim.fn.writefile({
      "#!/usr/bin/env bash",
      "set -euo pipefail",
      'printf \'%s\\n\' "$*" >> "' .. argv_log .. '"',
      'if [[ "${1:-}" == "record" && "${2:-}" == "--help" ]]; then',
      "  printf 'dictate record\\n  --json-events\\n  --save-last-audio\\n'",
      "  exit 0",
      "fi",
      'if [[ "${1:-}" == "retry" && "${2:-}" == "--help" ]]; then',
      "  printf 'dictate retry\\n  --json-events\\n'",
      "  exit 0",
      "fi",
      'phase="recording"',
      "trap 'phase=\"transcribing\"' USR1",
      'printf \'{"event":"session","mode":"record","phase":"recording","stop_after_ms":null}\\n\' >&2',
      'while [[ "$phase" == "recording" ]]; do',
      "  sleep 0.05",
      "done",
      'printf \'{"event":"phase","phase":"transcribing","chunk_count":1,"model":null}\\n\' >&2',
      "printf 'logged args\\n'",
      'printf \'{"event":"result","status":"completed","char_count":11,"copied_to_clipboard":false}\\n\' >&2',
    }, script)
    vim.fn.setfperm(script, "rwx------")

    Dictate.setup({
      cmd = { script },
      disabled_filetypes = {},
      disabled_buftypes = {},
    })
    vim.fn.writefile({}, argv_log)

    local ok, err = pcall(function()
      assert.is_true(Dictate.start())
      assert.is_true(Dictate.stop())
      assert.is_true(vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end))
    end)

    local argv = vim.fn.readfile(argv_log)
    os.remove(script)
    os.remove(argv_log)

    if not ok then
      error(err, 0)
    end

    assert.are.same({ "record --save-last-audio --format text --json-events --no-clipboard" }, argv)
  end)

  it("does not send context args by default in markdown buffers", function()
    local argv_log = vim.fn.tempname()
    local script = write_command_logging_script(argv_log)

    Dictate.setup({
      cmd = { script },
      disabled_filetypes = {},
      disabled_buftypes = {},
    })
    vim.bo.filetype = "markdown"
    vim.api.nvim_buf_set_lines(0, 0, -1, false, { "This is SNAKE_CASE.", "" })
    vim.api.nvim_win_set_cursor(0, { 2, 0 })

    local ok, err = pcall(function()
      assert.is_true(Dictate.start())
      assert.is_true(Dictate.stop())
      assert.is_true(vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end))
    end)

    local argv = vim.fn.readfile(argv_log)
    os.remove(script)
    os.remove(argv_log)

    if not ok then
      error(err, 0)
    end

    assert.equals("record --save-last-audio --format text --json-events --no-clipboard", argv[1])
  end)

  it("adds post-processing context args for markdown record commands", function()
    local argv_log = vim.fn.tempname()
    local script = write_command_logging_script(argv_log)

    Dictate.setup({
      cmd = { script },
      disabled_filetypes = {},
      disabled_buftypes = {},
      context_enrichment = {
        enabled = true,
        filetypes = { "markdown" },
        max_lines_before = 20,
        max_lines_after = 5,
        max_chars = 1000,
      },
    })
    vim.bo.filetype = "markdown"
    vim.api.nvim_buf_set_lines(0, 0, -1, false, { "This is SNAKE_CASE.", "" })
    vim.api.nvim_win_set_cursor(0, { 2, 0 })

    local ok, err = pcall(function()
      assert.is_true(Dictate.start())
      assert.is_true(Dictate.stop())
      assert.is_true(vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end))
    end)

    local argv = vim.fn.readfile(argv_log)
    os.remove(script)
    os.remove(argv_log)

    if not ok then
      error(err, 0)
    end

    assert.equals(
      "record --save-last-audio --post-process --post-process-context SNAKE_CASE --format text --json-events --no-clipboard",
      argv[1]
    )
  end)

  it("does not duplicate short post-processing flags for enriched record commands", function()
    local argv_log = vim.fn.tempname()
    local script = write_command_logging_script(argv_log)

    Dictate.setup({
      cmd = { script },
      args = { "-p" },
      disabled_filetypes = {},
      disabled_buftypes = {},
      context_enrichment = {
        enabled = true,
        filetypes = { "markdown" },
        max_lines_before = 20,
        max_lines_after = 5,
        max_chars = 1000,
      },
    })
    vim.bo.filetype = "markdown"
    vim.api.nvim_buf_set_lines(0, 0, -1, false, { "This is SNAKE_CASE.", "" })
    vim.api.nvim_win_set_cursor(0, { 2, 0 })

    local ok, err = pcall(function()
      assert.is_true(Dictate.start())
      assert.is_true(Dictate.stop())
      assert.is_true(vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end))
    end)

    local argv = vim.fn.readfile(argv_log)
    os.remove(script)
    os.remove(argv_log)

    if not ok then
      error(err, 0)
    end

    assert.equals(
      "record -p --save-last-audio --post-process-context SNAKE_CASE --format text --json-events --no-clipboard",
      argv[1]
    )
  end)

  it("preserves manual post-processing context for enriched record commands", function()
    local argv_log = vim.fn.tempname()
    local script = write_command_logging_script(argv_log)

    Dictate.setup({
      cmd = { script },
      args = { "--post-process-context", "STATIC" },
      disabled_filetypes = {},
      disabled_buftypes = {},
      context_enrichment = {
        enabled = true,
        filetypes = { "markdown" },
        max_lines_before = 20,
        max_lines_after = 5,
        max_chars = 1000,
      },
    })
    vim.bo.filetype = "markdown"
    vim.api.nvim_buf_set_lines(0, 0, -1, false, { "This is SNAKE_CASE.", "" })
    vim.api.nvim_win_set_cursor(0, { 2, 0 })

    local ok, err = pcall(function()
      assert.is_true(Dictate.start())
      assert.is_true(Dictate.stop())
      assert.is_true(vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end))
    end)

    local argv = vim.fn.readfile(argv_log)
    os.remove(script)
    os.remove(argv_log)

    if not ok then
      error(err, 0)
    end

    assert.equals(
      "record --post-process-context STATIC --save-last-audio --post-process --format text --json-events --no-clipboard",
      argv[1]
    )
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

  it("retries and inserts transcript at the original cursor", function()
    vim.env.DICTATE_FIXTURE_SCENARIO = "success"
    vim.env.DICTATE_FIXTURE_TRANSCRIPT = "retry transcript"

    assert.is_true(Dictate.retry())
    assert.equals("retrying", Dictate.get_state())

    vim.wait(3000, function()
      return Dictate.get_state() == "idle"
    end)

    local line = vim.api.nvim_buf_get_lines(0, 0, 1, false)[1]
    assert.equals("retry transcript ", line)
  end)

  it("filters record-only args from retry while preserving retry overrides", function()
    local script = vim.fn.tempname()
    local argv_log = vim.fn.tempname()
    vim.fn.writefile({
      "#!/usr/bin/env bash",
      "set -euo pipefail",
      'for arg in "$@"; do',
      '  if [[ "$arg" == "--help" ]]; then',
      "    printf 'dictate record\\n  --json-events\\n'",
      "    printf 'dictate retry\\n  --json-events\\n'",
      "    exit 0",
      "  fi",
      "done",
      'printf \'%s\\n\' "$*" > "' .. argv_log .. '"',
      'for arg in "$@"; do',
      '  if [[ "$arg" == "--save-last-audio" || "$arg" == "--device" || "$arg" == "--stop-after" || "$arg" == "--timestamps" || "$arg" == "--device="* || "$arg" == "--stop-after="* || "$arg" == "--timestamps="* ]]; then',
      '    printf \'{"event":"result","status":"failed","message":"unexpected retry arg","causes":["%s"]}\\n\' "$arg" >&2',
      "    exit 1",
      "  fi",
      "done",
      'printf \'{"event":"session","mode":"retry","phase":"retrying","stop_after_ms":null}\\n\' >&2',
      'printf \'{"event":"phase","phase":"transcribing","chunk_count":1,"model":null}\\n\' >&2',
      "printf 'retry args filtered\\n'",
      'printf \'{"event":"result","status":"completed","char_count":19,"copied_to_clipboard":false}\\n\' >&2',
    }, script)
    vim.fn.setfperm(script, "rwx------")

    Dictate.setup({
      cmd = { script },
      args = {
        "--save-last-audio",
        "--stop-after",
        "30s",
        "--device=USB Mic",
        "--timestamps",
        "word,segment",
        "--post-process",
        "--transcription-model",
        "large-v3",
      },
      disabled_filetypes = {},
      disabled_buftypes = {},
    })

    local ok, err = pcall(function()
      assert.is_true(Dictate.retry())
      assert.is_true(vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end))
    end)

    local argv = vim.fn.readfile(argv_log)
    os.remove(script)
    os.remove(argv_log)

    if not ok then
      error(err, 0)
    end

    local line = vim.api.nvim_buf_get_lines(0, 0, 1, false)[1]
    assert.equals("retry args filtered ", line)
    assert.equals(
      "retry --post-process --transcription-model large-v3 --format text --json-events --no-clipboard",
      argv[1]
    )
  end)

  it("adds fresh current-buffer context args for retry commands", function()
    local argv_log = vim.fn.tempname()
    local script = write_command_logging_script(argv_log)

    Dictate.setup({
      cmd = { script },
      disabled_filetypes = {},
      disabled_buftypes = {},
      context_enrichment = {
        enabled = true,
        filetypes = { "markdown" },
        max_lines_before = 20,
        max_lines_after = 5,
        max_chars = 1000,
      },
    })
    vim.bo.filetype = "markdown"
    vim.api.nvim_buf_set_lines(0, 0, -1, false, { "Use HTTP2 here.", "" })
    vim.api.nvim_win_set_cursor(0, { 2, 0 })

    local ok, err = pcall(function()
      assert.is_true(Dictate.retry())
      assert.is_true(vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end))
    end)

    local argv = vim.fn.readfile(argv_log)
    os.remove(script)
    os.remove(argv_log)

    if not ok then
      error(err, 0)
    end

    assert.equals(
      "retry --post-process --post-process-context HTTP2 --format text --json-events --no-clipboard",
      argv[1]
    )
  end)

  it("does not duplicate short post-processing flags for enriched retry commands", function()
    local argv_log = vim.fn.tempname()
    local script = write_command_logging_script(argv_log)

    Dictate.setup({
      cmd = { script },
      args = { "-p" },
      disabled_filetypes = {},
      disabled_buftypes = {},
      context_enrichment = {
        enabled = true,
        filetypes = { "markdown" },
        max_lines_before = 20,
        max_lines_after = 5,
        max_chars = 1000,
      },
    })
    vim.bo.filetype = "markdown"
    vim.api.nvim_buf_set_lines(0, 0, -1, false, { "Use HTTP2 here.", "" })
    vim.api.nvim_win_set_cursor(0, { 2, 0 })

    local ok, err = pcall(function()
      assert.is_true(Dictate.retry())
      assert.is_true(vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end))
    end)

    local argv = vim.fn.readfile(argv_log)
    os.remove(script)
    os.remove(argv_log)

    if not ok then
      error(err, 0)
    end

    assert.equals("retry -p --post-process-context HTTP2 --format text --json-events --no-clipboard", argv[1])
  end)

  it("does not send retry context when post-processing is explicitly disabled", function()
    local argv_log = vim.fn.tempname()
    local script = write_command_logging_script(argv_log)

    Dictate.setup({
      cmd = { script },
      args = { "--no-post-process" },
      disabled_filetypes = {},
      disabled_buftypes = {},
      context_enrichment = {
        enabled = true,
        filetypes = { "markdown" },
        max_lines_before = 20,
        max_lines_after = 5,
        max_chars = 1000,
      },
    })
    vim.bo.filetype = "markdown"
    vim.api.nvim_buf_set_lines(0, 0, -1, false, { "Use HTTP2 here.", "" })
    vim.api.nvim_win_set_cursor(0, { 2, 0 })

    local ok, err = pcall(function()
      assert.is_true(Dictate.retry())
      assert.is_true(vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end))
    end)

    local argv = vim.fn.readfile(argv_log)
    os.remove(script)
    os.remove(argv_log)

    if not ok then
      error(err, 0)
    end

    assert.equals("retry --no-post-process --format text --json-events --no-clipboard", argv[1])
  end)

  it("preserves manual post-processing context for enriched retry commands", function()
    local argv_log = vim.fn.tempname()
    local script = write_command_logging_script(argv_log)

    Dictate.setup({
      cmd = { script },
      args = { "--post-process-context=STATIC" },
      disabled_filetypes = {},
      disabled_buftypes = {},
      context_enrichment = {
        enabled = true,
        filetypes = { "markdown" },
        max_lines_before = 20,
        max_lines_after = 5,
        max_chars = 1000,
      },
    })
    vim.bo.filetype = "markdown"
    vim.api.nvim_buf_set_lines(0, 0, -1, false, { "Use HTTP2 here.", "" })
    vim.api.nvim_win_set_cursor(0, { 2, 0 })

    local ok, err = pcall(function()
      assert.is_true(Dictate.retry())
      assert.is_true(vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end))
    end)

    local argv = vim.fn.readfile(argv_log)
    os.remove(script)
    os.remove(argv_log)

    if not ok then
      error(err, 0)
    end

    assert.equals(
      "retry --post-process-context=STATIC --post-process --format text --json-events --no-clipboard",
      argv[1]
    )
  end)

  it("cancels an in-flight retry", function()
    vim.env.DICTATE_FIXTURE_SCENARIO = "cancel_during_transcribing"

    assert.is_true(Dictate.retry())
    assert.equals("retrying", Dictate.get_state())

    assert.is_true(vim.wait(1000, function()
      return Dictate.get_state() == "transcribing"
    end))
    assert.is_true(Dictate.stop())

    assert.is_true(vim.wait(3000, function()
      return Dictate.get_state() == "idle"
    end))

    local line = vim.api.nvim_buf_get_lines(0, 0, 1, false)[1]
    assert.equals("", line)
    assert.is_true(vim.iter(notifications):any(function(item)
      return item.message == "Dictation cancelled"
    end))
  end)

  it("rejects retry when a dictate session is already active", function()
    vim.env.DICTATE_FIXTURE_SCENARIO = "success"
    vim.env.DICTATE_FIXTURE_TRANSCRIPT = "original"

    assert.is_true(Dictate.start())
    assert.equals("recording", Dictate.get_state())

    assert.is_false(Dictate.retry())
    assert.is_true(vim.iter(notifications):any(function(item)
      return item.message == "Dictation is already active"
    end))

    assert.is_true(Dictate.stop())
    vim.wait(3000, function()
      return Dictate.get_state() == "idle"
    end)
  end)

  it("surfaces retry CLI failure", function()
    vim.env.DICTATE_FIXTURE_SCENARIO = "fail_immediately"

    local ok, err = pcall(function()
      assert.is_true(Dictate.retry())
      assert.is_true(vim.wait(3000, function()
        return Dictate.get_state() == "idle"
      end))
    end)

    if not ok then
      error(err, 0)
    end

    assert.is_true(vim.iter(notifications):any(function(item)
      return item.message == "retry failed: no saved recording"
    end))
  end)
end)
