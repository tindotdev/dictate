local M = {}
local config = require('dictate.config')
local ui = require('dictate.ui')
local notify = require('dictate.notify')

---@type integer|nil
local job_id = nil

-- New daemon states: idle, audio_starting, listening, flushing, reconnecting, error
-- dictatectl states: connecting, connected, reconnecting
-- We track both the daemon state and dictatectl state
---@type 'stopped'|'connecting'|'connected'|'idle'|'audio_starting'|'listening'|'flushing'|'reconnecting'|'error'
local state = 'stopped'

---@type 'stopped'|'connecting'|'connected'|'idle'|'audio_starting'|'listening'|'flushing'|'reconnecting'|'error'
local prev_state = 'stopped'

-- Subsystem health (from daemon status messages)
local audio_ok = false
local ws_ok = false

-- Flag to send start_listening after dictatectl connects
local pending_start_listening = false

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

---Check if state is "active" (recording/listening)
---@param s string
---@return boolean
local function is_active_state(s)
  return s == 'listening' or s == 'recording' -- Support both old and new names
end

---Update state and trigger callbacks on transitions
---@param new_state string
local function set_state(new_state)
  prev_state = state
  state = new_state

  -- Trigger callbacks on state transitions
  if not is_active_state(prev_state) and is_active_state(new_state) then
    call_on_start()
  elseif is_active_state(prev_state) and not is_active_state(new_state) then
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

---Handle a parsed message from the daemon or sayctl
---@param msg table
function M.handle_message(msg)
  local t = msg.type

  if t == 'status' then
    -- Update subsystem health if provided (daemon messages)
    if msg.audio_ok ~= nil then audio_ok = msg.audio_ok end
    if msg.ws_ok ~= nil then ws_ok = msg.ws_ok end

    set_state(msg.state)

    -- Log state transitions
    if state == 'error' then
      notify.error(msg.message or 'unknown error')
    elseif state == 'idle' or state == 'ready' then
      notify.debug('daemon ready')
    elseif state == 'listening' or state == 'recording' then
      notify.debug('recording started')
    elseif state == 'connecting' then
      notify.debug('connecting to daemon...')
    elseif state == 'connected' then
      notify.debug('connected to daemon')
      -- Send pending start_listening now that dictatectl is connected
      if pending_start_listening then
        pending_start_listening = false
        M.send({ type = 'start_listening' })
      end
    elseif state == 'reconnecting' then
      notify.debug('reconnecting...')
    elseif state == 'audio_starting' then
      notify.debug('starting audio capture...')
    elseif state == 'flushing' then
      notify.debug('waiting for final transcript...')
    end

  elseif t == 'initialized' then
    -- Handshake response from daemon
    notify.debug('daemon v' .. (msg.daemon_version or '?') .. ' ready')

  elseif t == 'speech_started' then
    ui.on_speech_started(msg.item_id)
    notify.debug('speech detected')

  elseif t == 'speech_stopped' then
    ui.on_speech_stopped(msg.item_id)
    notify.debug('speech ended')

  elseif t == 'partial_transcript' or t == 'delta' then
    -- Support both new (partial_transcript) and legacy (delta) names
    ui.on_delta(msg.item_id, msg.text)

  elseif t == 'final_transcript' or t == 'final' then
    -- Support both new (final_transcript) and legacy (final) names
    ui.on_final(msg.item_id, msg.text)
    notify.debug('transcription: ' .. (msg.text or ''))

  elseif t == 'error' then
    local hint = msg.hint and (' (' .. msg.hint .. ')') or ''
    notify.error('[' .. (msg.code or '?') .. '] ' .. (msg.message or 'unknown') .. hint)

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
    -- Already running, just send start_listening
    M.send({ type = 'start_listening' })
    return
  end

  local cfg = config.get()
  if not cfg.daemon_cmd then
    notify.error('daemon_cmd not configured. Run :checkhealth dictate for help.')
    return
  end

  notify.debug('starting dictatectl: ' .. table.concat(cfg.daemon_cmd, ' '))

  line_buffer = ''
  audio_ok = false
  ws_ok = false
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
            notify.debug('dictatectl stderr: ' .. line)
          end
        end
      end)
    end,
    on_exit = function(_, code, _)
      vim.schedule(function()
        job_id = nil
        set_state('stopped')
        line_buffer = ''
        audio_ok = false
        ws_ok = false
        pending_start_listening = false
        ui.clear_all()
        if code ~= 0 then
          notify.warn('dictatectl exited with code ' .. code .. '. Run :checkhealth dictate for troubleshooting.')
        else
          notify.debug('dictatectl stopped')
        end
      end)
    end,
    stdin = 'pipe',
    stdout_buffered = false,
  })

  if job_id <= 0 then
    notify.error('failed to start dictatectl. Is bun installed? Run :checkhealth dictate for help.')
    job_id = nil
    set_state('error')
    return
  end

  -- Set flag to send start_listening after sayctl connects
  pending_start_listening = true
end

---Stop dictation and the daemon
function M.stop()
  if job_id then
    M.send({ type = 'stop_listening' })
    -- Give it a moment to clean up, then force stop dictatectl
    vim.defer_fn(function()
      if job_id then
        vim.fn.jobstop(job_id)
        job_id = nil
      end
    end, 100)
  end
  set_state('stopped')
  line_buffer = ''
  audio_ok = false
  ws_ok = false
  pending_start_listening = false
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

---Check if audio subsystem is healthy
---@return boolean
function M.is_audio_ok()
  return audio_ok
end

---Check if WebSocket subsystem is healthy
---@return boolean
function M.is_ws_ok()
  return ws_ok
end

---Check if currently recording/listening
---@return boolean
function M.is_active()
  return is_active_state(state)
end

return M
