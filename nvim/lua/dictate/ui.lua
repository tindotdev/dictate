local M = {}
local config = require('dictate.config')

local ns_id = vim.api.nvim_create_namespace('dictate_ghost')

-- Track ghost text extmarks per item_id
---@type table<string, integer> item_id -> extmark_id
local ghost_marks = {}

-- Capture cursor position when speech starts
---@type { bufnr: integer, line: integer, col: integer }|nil
local cursor_pos = nil

---Called when speech starts for a new item
---@param item_id string
function M.on_speech_started(item_id)
  -- Capture current cursor position (0-indexed line, 0-indexed col)
  local bufnr = vim.api.nvim_get_current_buf()
  local pos = vim.api.nvim_win_get_cursor(0)
  cursor_pos = {
    bufnr = bufnr,
    line = pos[1] - 1, -- Convert to 0-indexed
    col = pos[2],
  }
  ghost_marks[item_id] = nil
end

---Called when a transcription delta is received
---@param item_id string
---@param text string Full accumulated text so far
function M.on_delta(item_id, text)
  if not cursor_pos then
    return
  end

  local bufnr = cursor_pos.bufnr
  local line = cursor_pos.line
  local col = cursor_pos.col

  -- Ensure buffer is still valid
  if not vim.api.nvim_buf_is_valid(bufnr) then
    return
  end

  -- Clear previous ghost for this item
  if ghost_marks[item_id] then
    pcall(vim.api.nvim_buf_del_extmark, bufnr, ns_id, ghost_marks[item_id])
  end

  -- Display ghost text at cursor position
  local cfg = config.get()
  local mark_id = vim.api.nvim_buf_set_extmark(bufnr, ns_id, line, col, {
    virt_text = { { text, cfg.ghost_hl } },
    virt_text_pos = 'inline',
    priority = 100,
  })

  ghost_marks[item_id] = mark_id
end

---Called when transcription is finalized
---@param item_id string
---@param text string Final transcript text
function M.on_final(item_id, text)
  if not cursor_pos then
    return
  end

  local bufnr = cursor_pos.bufnr
  local line = cursor_pos.line
  local col = cursor_pos.col

  -- Clear ghost text
  if ghost_marks[item_id] then
    pcall(vim.api.nvim_buf_del_extmark, bufnr, ns_id, ghost_marks[item_id])
    ghost_marks[item_id] = nil
  end

  -- Ensure buffer is still valid
  if not vim.api.nvim_buf_is_valid(bufnr) then
    return
  end

  -- Prepare text to insert
  local cfg = config.get()
  local insert_text = text
  if cfg.insert_trailing_space and text ~= '' then
    insert_text = text .. ' '
  end

  -- Insert text at captured cursor position
  local ok, err = pcall(vim.api.nvim_buf_set_text, bufnr, line, col, line, col, { insert_text })
  if not ok then
    vim.notify('dictate: failed to insert text: ' .. tostring(err), vim.log.levels.ERROR)
    return
  end

  -- Move cursor to end of inserted text
  local new_col = col + #insert_text
  pcall(vim.api.nvim_win_set_cursor, 0, { line + 1, new_col })

  -- Update cursor_pos for next segment
  cursor_pos.col = new_col
end

---Called when speech stops (but transcript may still be processing)
---@param _item_id string
function M.on_speech_stopped(_item_id)
  -- Nothing to do here for ghost text mode
  -- The ghost text stays until final arrives
end

---Clear all ghost text and reset state
function M.clear_all()
  -- Clear all extmarks
  for item_id, mark_id in pairs(ghost_marks) do
    if cursor_pos and vim.api.nvim_buf_is_valid(cursor_pos.bufnr) then
      pcall(vim.api.nvim_buf_del_extmark, cursor_pos.bufnr, ns_id, mark_id)
    end
  end
  ghost_marks = {}
  cursor_pos = nil
end

---Get the namespace ID for testing
---@return integer
function M.get_namespace()
  return ns_id
end

return M
