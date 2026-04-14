local healthy_fixture = vim.fn.fnamemodify("tests/fixtures/fake-dictate.sh", ":p")
local legacy_fixture = vim.fn.fnamemodify("tests/fixtures/fake-dictate-no-json.sh", ":p")

describe("dictate health", function()
  local Dictate
  local Health

  local function collect()
    local checks = {}
    Health._run_checks({
      start = function(message)
        table.insert(checks, { kind = "start", message = message })
      end,
      ok = function(message)
        table.insert(checks, { kind = "ok", message = message })
      end,
      warn = function(message)
        table.insert(checks, { kind = "warn", message = message })
      end,
      error = function(message)
        table.insert(checks, { kind = "error", message = message })
      end,
    })
    return checks
  end

  before_each(function()
    package.loaded["dictate"] = nil
    package.loaded["dictate.capabilities"] = nil
    package.loaded["dictate.config"] = nil
    package.loaded["dictate.health"] = nil
    Dictate = require("dictate")
    Health = require("dictate.health")
  end)

  it("passes when the configured command supports json events", function()
    vim.env.GROQ_API_KEY = "test-key"
    Dictate.setup({
      cmd = { healthy_fixture },
    })

    local checks = collect()
    assert.is_true(vim.iter(checks):any(function(item)
      return item.kind == "ok" and item.message:find("json%-events")
    end))
    assert.is_true(vim.iter(checks):any(function(item)
      return item.kind == "ok" and item.message:find("supports retry", 1, true) ~= nil
    end))
    assert.is_true(vim.iter(checks):any(function(item)
      return item.kind == "ok" and item.message:find("save%-last%-audio")
    end))
  end)

  it("fails when the configured command does not support json events", function()
    vim.env.GROQ_API_KEY = "test-key"
    Dictate.setup({
      cmd = { legacy_fixture },
    })

    local checks = collect()
    assert.is_true(vim.iter(checks):any(function(item)
      return item.kind == "error" and item.message:find("json%-events")
    end))
  end)

  it("warns when the configured command does not support retry", function()
    local script = vim.fn.tempname()
    vim.fn.writefile({
      "#!/usr/bin/env bash",
      "set -euo pipefail",
      'if [[ "${1:-}" == "record" && "${2:-}" == "--help" ]]; then',
      "  printf 'dictate record\\n  --json-events\\n'",
      "  exit 0",
      "fi",
      'if [[ "${1:-}" == "retry" && "${2:-}" == "--help" ]]; then',
      "  printf 'unknown subcommand: retry\\n' >&2",
      "  exit 1",
      "fi",
      "exit 0",
    }, script)
    vim.fn.setfperm(script, "rwx------")

    vim.env.GROQ_API_KEY = "test-key"
    Dictate.setup({
      cmd = { script },
    })

    local ok, err = pcall(function()
      local checks = collect()
      assert.is_true(vim.iter(checks):any(function(item)
        return item.kind == "warn" and item.message:find("does not support retry", 1, true) ~= nil
      end))
    end)

    os.remove(script)
    if not ok then
      error(err, 0)
    end
  end)

  it("warns when the configured command does not advertise save-last-audio", function()
    local script = vim.fn.tempname()
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
      "exit 0",
    }, script)
    vim.fn.setfperm(script, "rwx------")

    vim.env.GROQ_API_KEY = "test-key"
    Dictate.setup({
      cmd = { script },
    })

    local ok, err = pcall(function()
      local checks = collect()
      assert.is_true(vim.iter(checks):any(function(item)
        return item.kind == "warn" and item.message:find("save%-last%-audio")
      end))
    end)

    os.remove(script)
    if not ok then
      error(err, 0)
    end
  end)
end)
