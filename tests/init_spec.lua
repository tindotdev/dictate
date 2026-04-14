local healthy_fixture = vim.fn.fnamemodify("tests/fixtures/fake-dictate.sh", ":p")
local legacy_fixture = vim.fn.fnamemodify("tests/fixtures/fake-dictate-no-json.sh", ":p")

describe("dictate init", function()
  local Dictate
  local original_notify
  local notifications

  local function commands()
    return vim.api.nvim_get_commands({ builtin = false })
  end

  local function write_fixture(lines)
    local script = vim.fn.tempname()
    vim.fn.writefile(lines, script)
    vim.fn.setfperm(script, "rwx------")
    return script
  end

  before_each(function()
    package.loaded["dictate"] = nil
    package.loaded["dictate.capabilities"] = nil
    package.loaded["dictate.config"] = nil
    package.loaded["dictate.health"] = nil
    package.loaded["dictate.session"] = nil

    original_notify = vim.notify
    notifications = {}
    vim.notify = function(message, level)
      table.insert(notifications, { message = message, level = level })
    end

    Dictate = require("dictate")
  end)

  after_each(function()
    vim.notify = original_notify

    for _, name in ipairs({ "DictateStart", "DictateStop", "DictateToggle", "DictateRetry" }) do
      pcall(vim.api.nvim_del_user_command, name)
    end
  end)

  it("registers DictateRetry when the CLI supports retry", function()
    Dictate.setup({
      cmd = { healthy_fixture },
    })

    assert.is_not_nil(commands().DictateRetry)
  end)

  it("does not register DictateRetry when the CLI does not support retry", function()
    Dictate.setup({
      cmd = { legacy_fixture },
    })

    assert.is_nil(commands().DictateRetry)
  end)

  it("does not fail setup when the configured command is missing", function()
    Dictate.setup({
      cmd = { "dictate-command-does-not-exist" },
    })

    assert.is_nil(commands().DictateRetry)
  end)

  it("does not register DictateRetry when retry help exits 0 without advertising retry", function()
    local script = write_fixture({
      "#!/usr/bin/env bash",
      "set -euo pipefail",
      'if [[ "${1:-}" == "record" && "${2:-}" == "--help" ]]; then',
      "  printf 'dictate record\\n  --json-events\\n'",
      "  exit 0",
      "fi",
      'if [[ "${1:-}" == "retry" && "${2:-}" == "--help" ]]; then',
      "  printf 'dictate wrapper\\n  --json-events\\n'",
      "  exit 0",
      "fi",
      "exit 0",
    })

    local ok, err = pcall(function()
      Dictate.setup({
        cmd = { script },
      })

      assert.is_nil(commands().DictateRetry)
    end)

    os.remove(script)
    if not ok then
      error(err, 0)
    end
  end)

  it("does not register DictateRetry when retry help omits json-events", function()
    local script = write_fixture({
      "#!/usr/bin/env bash",
      "set -euo pipefail",
      'if [[ "${1:-}" == "record" && "${2:-}" == "--help" ]]; then',
      "  printf 'dictate record\\n  --json-events\\n  --save-last-audio\\n'",
      "  exit 0",
      "fi",
      'if [[ "${1:-}" == "retry" && "${2:-}" == "--help" ]]; then',
      "  printf 'dictate retry\\n'",
      "  exit 0",
      "fi",
      "exit 0",
    })

    local ok, err = pcall(function()
      Dictate.setup({
        cmd = { script },
      })

      assert.is_nil(commands().DictateRetry)
    end)

    os.remove(script)
    if not ok then
      error(err, 0)
    end
  end)

  it("removes DictateRetry on setup when the configured CLI no longer supports retry", function()
    Dictate.setup({
      cmd = { healthy_fixture },
    })
    assert.is_not_nil(commands().DictateRetry)

    Dictate.setup({
      cmd = { legacy_fixture },
    })

    assert.is_nil(commands().DictateRetry)
  end)

  it("warns and returns false when retry is invoked against a legacy CLI", function()
    Dictate.setup({
      cmd = { legacy_fixture },
    })

    assert.is_false(Dictate.retry())
    assert.is_true(vim.iter(notifications):any(function(item)
      return item.message:find("does not support retry", 1, true) ~= nil
    end))
  end)

  it("warns and returns false when retry is invoked with a missing command", function()
    Dictate.setup({
      cmd = { "dictate-command-does-not-exist" },
    })

    assert.is_false(Dictate.retry())
    assert.is_true(vim.iter(notifications):any(function(item)
      return item.message:find("does not support retry", 1, true) ~= nil
    end))
  end)
end)
