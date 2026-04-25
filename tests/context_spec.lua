describe("dictate.context", function()
  local Context

  before_each(function()
    package.loaded["dictate.context"] = nil
    Context = require("dictate.context")
  end)

  it("extracts markdown identifiers near the insertion point", function()
    vim.cmd("enew")
    vim.bo.filetype = "markdown"
    vim.api.nvim_buf_set_lines(0, 0, -1, false, { "This is SNAKE_CASE.", "" })

    local context = Context.extract_for_buffer(0, 1, {
      enabled = true,
      filetypes = { "markdown" },
      max_lines_before = 20,
      max_lines_after = 5,
      max_chars = 1000,
    })

    assert.equals("SNAKE_CASE", context)
    vim.cmd("bwipeout!")
  end)

  it("ignores non-markdown buffers", function()
    vim.cmd("enew")
    vim.bo.filetype = "lua"
    vim.api.nvim_buf_set_lines(0, 0, -1, false, { "local value = SNAKE_CASE" })

    local context = Context.extract_for_buffer(0, 0, {
      enabled = true,
      filetypes = { "markdown" },
      max_lines_before = 20,
      max_lines_after = 5,
      max_chars = 1000,
    })

    assert.equals("", context)
    vim.cmd("bwipeout!")
  end)

  it("deduplicates terms and truncates at term boundaries", function()
    local context = Context.extract_terms({
      "SNAKE_CASE HTTP2 SNAKE_CASE camelCase kebab-case",
    }, 16)

    assert.equals("SNAKE_CASE\nHTTP2", context)
  end)

  it("preserves identifier boundary underscores", function()
    local context = Context.extract_terms({
      "__init__ _PRIVATE",
    }, 1000)

    assert.equals("__init__\n_PRIVATE", context)
  end)
end)
