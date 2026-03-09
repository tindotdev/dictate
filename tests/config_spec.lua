describe("dictate.config", function()
  local Config

  before_each(function()
    package.loaded["dictate.config"] = nil
    Config = require("dictate.config")
    Config.setup({
      cmd = { "dictate" },
      args = {},
      clipboard = false,
      insert_trailing_space = true,
      disabled_filetypes = { "help" },
      disabled_buftypes = { "terminal" },
    })
  end)

  it("normalizes a string command into a list", function()
    Config.setup({ cmd = "dictate" })
    assert.are.same({ "dictate" }, Config.get().cmd)
  end)

  it("rejects invalid args", function()
    local ok = pcall(function()
      Config.setup({ args = "oops" })
    end)
    assert.is_false(ok)
  end)

  it("rejects disabled buffers", function()
    vim.cmd("enew")
    vim.bo.filetype = "help"
    local allowed, reason = Config.is_buffer_allowed(0)
    assert.is_false(allowed)
    assert.is_not_nil(reason)
    vim.cmd("bwipeout!")
  end)
end)
