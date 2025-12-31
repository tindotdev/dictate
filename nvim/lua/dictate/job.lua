local M = {}
local config = require('dictate.config')
local ui = require('dictate.ui')
local notify = require('dictate.notify')

---@type integer|nil
local job_id = nil

---@type 'stopped'|'connecting'|'ready'|'recording'|'error'
local state = 'stopped'

---@type 'stopped'|'connecting'|'ready'|'recording'|'error'
local prev_state = 'stopped'

-- Buffer for incomplete JSONL lines
local line_buffer = ''

---Call on_start callback if configured
local function call_on_start()
  local cfg = config.get()
  if cfg.on_start and type(cfg.on_start) == 'function' then
    local ok, err = pcall(cfg.on_start)
    if not ok then
      notify.warn('on_start callback error: ' .. tostring(err))
    end
  end
end

---Call on_stop callback if configured
local function call_on_stop()
  local cfg = config.get()
  if cfg.on_stop and type(cfg.on_stop) == 'function' then
    local ok, err = pcall(cfg.on_stop)
    if not ok then
      notify.warn('on_stop callback error: ' .. tostring(err))
    end
  end
end

---Update state and trigger callbacks on transitions
---@param new_state 'stopped'|'connecting'|'ready'|'recording'|'error'
local function set_state(new_state)
  prev_state = state
  state = new_state

  -- Trigger callbacks on state transitions
  if prev_state ~= 'recording' and new_state == 'recording' then
    call_on_start()
  elseif prev_state == 'recording' and new_state ~= 'recording' then
    call_on_stop()
  end
end

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
    set_state(msg.state)
    if state == 'error' then
      notify.error(msg.message or 'unknown error')
    elseif state == 'ready' then
      notify.debug('daemon ready')
    elseif state == 'recording' then
      notify.debug('recording started')
    end
  elseif t == 'speech_started' then
    ui.on_speech_started(msg.item_id)
    notify.debug('speech detected')
  elseif t == 'speech_stopped' then
    ui.on_speech_stopped(msg.item_id)
    notify.debug('speech ended')
  elseif t == 'delta' then
    ui.on_delta(msg.item_id, msg.text)
  elseif t == 'final' then
    ui.on_final(msg.item_id, msg.text)
    notify.debug('transcription: ' .. (msg.text or ''))
  elseif t == 'error' then
    notify.error('[' .. (msg.code or '?') .. '] ' .. (msg.message or 'unknown'))
  elseif t == 'debug' then
    notify.debug(msg.message or '')
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
  -- Check if buffer is allowed
  local allowed, reason = config.is_buffer_allowed()
  if not allowed then
    notify.warn(reason or 'buffer not allowed')
    return
  end

  if job_id then
    -- Already running, just send start
    M.send({ type = 'start' })
    return
  end

  local cfg = config.get()
  if not cfg.daemon_cmd then
    notify.error('daemon_cmd not configured. Run :checkhealth dictate for help.')
    return
  end

  notify.debug('starting daemon: ' .. table.concat(cfg.daemon_cmd, ' '))

  line_buffer = ''
  set_state('connecting')

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
            notify.debug('daemon stderr: ' .. line)
          end
        end
      end)
    end,
    on_exit = function(_, code, _)
      vim.schedule(function()
        job_id = nil
        set_state('stopped')
        line_buffer = ''
        ui.clear_all()
        if code ~= 0 then
          notify.warn('daemon exited with code ' .. code .. '. Run :checkhealth dictate for troubleshooting.')
        else
          notify.debug('daemon stopped')
        end
      end)
    end,
    stdin = 'pipe',
    stdout_buffered = false,
  })

  if job_id <= 0 then
    notify.error('failed to start daemon. Is bun installed? Run :checkhealth dictate for help.')
    job_id = nil
    set_state('error')
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
  set_state('stopped')
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
