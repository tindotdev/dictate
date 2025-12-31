local M = {}
local config = require('dictate.config')
local ui = require('dictate.ui')

---@type integer|nil
local job_id = nil

---@type 'stopped'|'connecting'|'ready'|'recording'|'error'
local state = 'stopped'

-- Buffer for incomplete JSONL lines
local line_buffer = ''

---Parse JSONL data from stdout
---@param data string[]
local function parse_jsonl(data)
  -- Neovim's jobstart splits output on newlines, so each element
  -- in data[] is already a line. The last element may be incomplete
  -- (no newline after it yet), so we buffer it for the next call.

  -- Prepend any leftover buffer to first element
  if line_buffer ~= '' and #data > 0 then
    data[1] = line_buffer .. (data[1] or '')
    line_buffer = ''
  end

  -- Last element might be incomplete (no trailing newline)
  if #data > 0 then
    line_buffer = data[#data] or ''
  end

  -- Process all complete lines (all but the last element)
  for i = 1, #data - 1 do
    local line = data[i]
    if line and line ~= '' then
      local ok, msg = pcall(vim.json.decode, line)
      if ok and msg then
        M.handle_message(msg)
      end
    end
  end
end

---Handle a parsed message from the daemon
---@param msg table
function M.handle_message(msg)
  local t = msg.type

  if t == 'status' then
    state = msg.state
    if state == 'error' then
      vim.notify('dictate: ' .. (msg.message or 'unknown error'), vim.log.levels.ERROR)
    end
  elseif t == 'speech_started' then
    ui.on_speech_started(msg.item_id)
  elseif t == 'speech_stopped' then
    ui.on_speech_stopped(msg.item_id)
  elseif t == 'delta' then
    ui.on_delta(msg.item_id, msg.text)
  elseif t == 'final' then
    ui.on_final(msg.item_id, msg.text)
  elseif t == 'error' then
    vim.notify('dictate: [' .. (msg.code or '?') .. '] ' .. (msg.message or 'unknown'), vim.log.levels.ERROR)
  elseif t == 'debug' then
    -- Debug messages only shown when debug is enabled
    if vim.g.dictate_debug then
      vim.notify('dictate debug: ' .. (msg.message or ''), vim.log.levels.DEBUG)
    end
  end
end

---Send a JSONL message to the daemon
---@param msg table
function M.send(msg)
  if job_id then
    local line = vim.json.encode(msg) .. '\n'
    vim.fn.chansend(job_id, line)
  end
end

---Start the daemon and begin dictation
function M.start()
  if job_id then
    -- Already running, just send start
    M.send({ type = 'start' })
    return
  end

  local cfg = config.get()
  if not cfg.daemon_cmd then
    vim.notify('dictate: daemon_cmd not configured', vim.log.levels.ERROR)
    return
  end

  line_buffer = ''

  job_id = vim.fn.jobstart(cfg.daemon_cmd, {
    on_stdout = function(_, data, _)
      vim.schedule(function()
        parse_jsonl(data)
      end)
    end,
    on_stderr = function(_, data, _)
      vim.schedule(function()
        for _, line in ipairs(data) do
          if line and line ~= '' then
            vim.notify('dictate daemon: ' .. line, vim.log.levels.WARN)
          end
        end
      end)
    end,
    on_exit = function(_, code, _)
      vim.schedule(function()
        job_id = nil
        state = 'stopped'
        line_buffer = ''
        ui.clear_all()
        if code ~= 0 then
          vim.notify('dictate: daemon exited with code ' .. code, vim.log.levels.WARN)
        end
      end)
    end,
    stdin = 'pipe',
    stdout_buffered = false,
  })

  if job_id <= 0 then
    vim.notify('dictate: failed to start daemon', vim.log.levels.ERROR)
    job_id = nil
    return
  end

  -- Send start command to daemon
  M.send({ type = 'start' })
end

---Stop dictation and the daemon
function M.stop()
  if job_id then
    M.send({ type = 'stop' })
    -- Give it a moment to clean up, then force stop
    vim.defer_fn(function()
      if job_id then
        vim.fn.jobstop(job_id)
        job_id = nil
      end
    end, 100)
  end
  state = 'stopped'
  line_buffer = ''
  ui.clear_all()
end

---Toggle dictation on/off
function M.toggle()
  if job_id then
    M.stop()
  else
    M.start()
  end
end

---Check if the daemon is running
---@return boolean
function M.is_running()
  return job_id ~= nil
end

---Get the current state
---@return string
function M.get_state()
  return state
end

return M
