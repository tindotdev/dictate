local M = {}

local markdown_punctuation = "[%s!\"#%$%%&'%(%)%*,%.:;<=>%?@%[%]%^`{|}~]"
local trim_pattern = "^" .. markdown_punctuation .. "*(.-)" .. markdown_punctuation .. "*$"

local function trim_markdown_punctuation(term)
  return (term:gsub(trim_pattern, "%1"))
end

local function is_high_signal(term)
  if term:find("[_%-%./:%d]") then
    return true
  end
  if term:find("%l.*%u") then
    return true
  end
  if #term > 1 and term == term:upper() and term:find("%u") then
    return true
  end
  return false
end

local function append_bounded(terms, term, max_chars)
  local candidate = #terms == 0 and term or table.concat({ table.concat(terms, "\n"), term }, "\n")
  if #candidate > max_chars then
    return false
  end
  table.insert(terms, term)
  return true
end

function M.extract_terms(lines, max_chars)
  local terms = {}
  local seen = {}

  for _, line in ipairs(lines) do
    for raw in line:gmatch("[%w_][%w_%-%./:]*") do
      local term = trim_markdown_punctuation(raw)
      if term ~= "" and is_high_signal(term) and not seen[term] then
        if not append_bounded(terms, term, max_chars) then
          return table.concat(terms, "\n")
        end
        seen[term] = true
      end
    end
  end

  return table.concat(terms, "\n")
end

function M.extract_for_buffer(bufnr, row, config)
  if not config.enabled then
    return ""
  end

  local filetype = vim.bo[bufnr].filetype
  if not vim.tbl_contains(config.filetypes, filetype) then
    return ""
  end

  local line_count = vim.api.nvim_buf_line_count(bufnr)
  local start_row = math.max(0, row - config.max_lines_before)
  local end_row = math.min(line_count, row + config.max_lines_after + 1)
  local lines = vim.api.nvim_buf_get_lines(bufnr, start_row, end_row, false)

  return M.extract_terms(lines, config.max_chars)
end

return M
