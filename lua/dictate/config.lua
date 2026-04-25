local M = {}

local defaults = {
  cmd = { "dictate" },
  args = {},
  clipboard = false,
  insert_trailing_space = true,
  disabled_filetypes = { "help", "lazy", "mason", "TelescopePrompt" },
  disabled_buftypes = { "nofile", "prompt", "quickfix", "terminal" },
  context_enrichment = {
    enabled = false,
    filetypes = { "markdown" },
    max_lines_before = 20,
    max_lines_after = 5,
    max_chars = 1000,
  },
}

local config = vim.deepcopy(defaults)

local function list_of_strings(name, value)
  if type(value) ~= "table" or not vim.islist(value) then
    error(("dictate.nvim: opts.%s must be a list of strings"):format(name))
  end
  for index, item in ipairs(value) do
    if type(item) ~= "string" then
      error(("dictate.nvim: opts.%s[%d] must be a string"):format(name, index))
    end
  end
end

local function normalize_cmd(value)
  if type(value) == "string" then
    return { value }
  end
  list_of_strings("cmd", value)
  if #value == 0 then
    error("dictate.nvim: opts.cmd must not be empty")
  end
  return vim.deepcopy(value)
end

local function validate_non_negative_integer(name, value)
  if type(value) ~= "number" or value % 1 ~= 0 or value < 0 then
    error(("dictate.nvim: opts.context_enrichment.%s must be a non-negative integer"):format(name))
  end
end

local function validate_positive_integer(name, value)
  if type(value) ~= "number" or value % 1 ~= 0 or value < 1 then
    error(("dictate.nvim: opts.context_enrichment.%s must be a positive integer"):format(name))
  end
end

function M.setup(opts)
  opts = opts or {}

  local merged = vim.tbl_deep_extend("force", vim.deepcopy(defaults), opts)
  merged.cmd = normalize_cmd(merged.cmd)
  list_of_strings("args", merged.args)
  list_of_strings("disabled_filetypes", merged.disabled_filetypes)
  list_of_strings("disabled_buftypes", merged.disabled_buftypes)
  list_of_strings("context_enrichment.filetypes", merged.context_enrichment.filetypes)
  validate_non_negative_integer("max_lines_before", merged.context_enrichment.max_lines_before)
  validate_non_negative_integer("max_lines_after", merged.context_enrichment.max_lines_after)
  validate_positive_integer("max_chars", merged.context_enrichment.max_chars)

  vim.validate({
    clipboard = { merged.clipboard, "boolean" },
    insert_trailing_space = { merged.insert_trailing_space, "boolean" },
    ["context_enrichment.enabled"] = { merged.context_enrichment.enabled, "boolean" },
  })

  config = merged
end

function M.get()
  return config
end

function M.command(extra_args)
  local cmd = vim.deepcopy(config.cmd)
  if extra_args and #extra_args > 0 then
    vim.list_extend(cmd, extra_args)
  end
  return cmd
end

function M.is_buffer_allowed(bufnr)
  local filetype = vim.bo[bufnr].filetype
  local buftype = vim.bo[bufnr].buftype

  if vim.tbl_contains(config.disabled_filetypes, filetype) then
    return false, ("filetype %q is disabled"):format(filetype)
  end

  if vim.tbl_contains(config.disabled_buftypes, buftype) then
    return false, ("buftype %q is disabled"):format(buftype)
  end

  if not vim.bo[bufnr].modifiable or vim.bo[bufnr].readonly then
    return false, "buffer is not modifiable"
  end

  return true
end

return M
