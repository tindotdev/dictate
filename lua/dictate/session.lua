local Capabilities = require("dictate.capabilities")
local Config = require("dictate.config")
local Context = require("dictate.context")

local M = {}

local namespace = vim.api.nvim_create_namespace("dictate")
local title = "dictate.nvim"

local state = {
  current = nil,
  phase = "idle",
  last_transcript = nil,
}

local function notify(message, level)
  vim.notify(message, level or vim.log.levels.INFO, { title = title })
end

local function is_modifiable_buffer(bufnr)
  return vim.api.nvim_buf_is_valid(bufnr) and vim.bo[bufnr].modifiable and not vim.bo[bufnr].readonly
end

local function split_lines(text)
  return vim.split(text, "\n", { plain = true })
end

local function strip_trailing_newlines(text)
  return (text:gsub("[\r\n]+$", ""))
end

local function has_flag(args, flag)
  for _, arg in ipairs(args) do
    local name = arg:match("^([^=]+)=") or arg
    if name == flag then
      return true
    end
  end

  return false
end

local function has_post_process_flag(args)
  return has_flag(args, "--post-process") or has_flag(args, "-p")
end

local function append_context_args(args, context, force_post_process)
  if context == "" then
    return
  end

  if force_post_process and not has_post_process_flag(args) then
    table.insert(args, "--post-process")
  end

  if has_flag(args, "--post-process-context") then
    return
  end

  table.insert(args, "--post-process-context")
  table.insert(args, context)
end

local function append_stdout(session, data)
  for index, chunk in ipairs(data) do
    if index == 1 then
      session.stdout_tail = session.stdout_tail .. chunk
    else
      session.stdout = session.stdout .. session.stdout_tail .. "\n"
      session.stdout_tail = chunk
    end
  end
end

local function flush_stdout(session)
  session.stdout = session.stdout .. session.stdout_tail
  session.stdout_tail = ""
end

local function build_record_command(bufnr, row)
  local cfg = Config.get()
  local args = { "record" }
  local record_args = vim.deepcopy(cfg.args)
  local context = Context.extract_for_buffer(bufnr, row, cfg.context_enrichment)
  if Capabilities.supports_save_last_audio() and not has_flag(record_args, "--save-last-audio") then
    table.insert(record_args, "--save-last-audio")
  end
  vim.list_extend(args, record_args)
  append_context_args(args, context, true)
  vim.list_extend(args, { "--format", "text", "--json-events" })
  table.insert(args, cfg.clipboard and "--stdout" or "--no-clipboard")
  return Config.command(args)
end

local retry_ignored_args = {
  ["--device"] = true,
  ["--save-last-audio"] = false,
  ["--stop-after"] = true,
  ["--timestamps"] = true,
}

local function retry_args(cfg_args)
  local args = {}
  local skip_next = false

  for _, arg in ipairs(cfg_args) do
    if skip_next then
      skip_next = false
    else
      local name = arg:match("^([^=]+)=") or arg
      local expects_value = retry_ignored_args[name]

      if expects_value ~= nil then
        skip_next = expects_value and name == arg
      else
        table.insert(args, arg)
      end
    end
  end

  return args
end

local function build_retry_command(bufnr, row)
  local cfg = Config.get()
  local args = { "retry" }
  local filtered_args = retry_args(vim.deepcopy(cfg.args))
  local context = has_flag(filtered_args, "--no-post-process") and ""
    or Context.extract_for_buffer(bufnr, row, cfg.context_enrichment)
  vim.list_extend(args, filtered_args)
  append_context_args(args, context, true)
  vim.list_extend(args, { "--format", "text", "--json-events" })
  table.insert(args, cfg.clipboard and "--stdout" or "--no-clipboard")
  return Config.command(args)
end

local function send_signal(session, signal)
  if not session.pid or session.pid <= 0 then
    return false, "dictate process is missing a PID"
  end
  local ok, result, err, code = pcall(vim.uv.kill, session.pid, signal)
  if not ok then
    return false, result
  end
  if result == nil or result == false then
    if type(err) == "string" and err ~= "" and code ~= nil then
      return false, ("%s (%s)"):format(err, tostring(code))
    end
    if type(err) == "string" and err ~= "" then
      return false, err
    end
    if code ~= nil then
      return false, tostring(code)
    end
    return false, "signal delivery failed"
  end
  return true
end

local function set_phase(session, phase)
  session.phase = phase
  state.phase = phase
end

local function resolve_target(session)
  local target = session.target
  if target and is_modifiable_buffer(target.bufnr) then
    local pos = vim.api.nvim_buf_get_extmark_by_id(target.bufnr, namespace, target.extmark, {})
    if #pos == 2 then
      return {
        bufnr = target.bufnr,
        row = pos[1],
        col = pos[2],
        source = "original",
      }
    end
  end

  local current = vim.api.nvim_get_current_buf()
  if is_modifiable_buffer(current) then
    local cursor = vim.api.nvim_win_get_cursor(0)
    return {
      bufnr = current,
      row = cursor[1] - 1,
      col = cursor[2],
      source = "current",
    }
  end

  return nil
end

local function open_scratch_with_text(text)
  vim.cmd("enew")
  local bufnr = vim.api.nvim_get_current_buf()
  vim.bo[bufnr].buftype = "nofile"
  vim.bo[bufnr].bufhidden = "wipe"
  vim.bo[bufnr].swapfile = false
  vim.bo[bufnr].modifiable = true
  vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, split_lines(text))
  vim.bo[bufnr].modified = false
end

local function insert_transcript(session, transcript)
  local cfg = Config.get()
  local text = cfg.insert_trailing_space and (transcript .. " ") or transcript
  local target = resolve_target(session)

  if target then
    vim.api.nvim_buf_set_text(target.bufnr, target.row, target.col, target.row, target.col, split_lines(text))
    if target.source == "original" then
      return "Inserted transcript"
    end
    return "Inserted transcript into the current buffer"
  end

  open_scratch_with_text(text)
  return "Original buffer was unavailable; opened transcript in a scratch buffer"
end

local function finalize_session(session, exit_code)
  if state.current ~= session then
    return
  end

  state.current = nil
  state.phase = "idle"

  local result = session.result
  local transcript = strip_trailing_newlines(session.stdout)
  if transcript ~= "" then
    state.last_transcript = transcript
  end

  if result and result.status == "completed" then
    if transcript ~= "" then
      notify(insert_transcript(session, transcript))
    else
      notify(result.message or "No speech detected")
    end
    return
  end

  if (result and result.status == "cancelled") or exit_code == 130 then
    notify("Dictation cancelled")
    return
  end

  local message = result and result.message or ("dictate exited with code " .. tostring(exit_code))
  if result and type(result.causes) == "table" then
    local cause
    for _, entry in ipairs(result.causes) do
      if type(entry) == "string" and entry ~= "" then
        cause = entry
      end
    end
    if cause and cause ~= message then
      message = ("%s: %s"):format(message, cause)
    end
  end
  notify(message, vim.log.levels.ERROR)
end

local function handle_event(session, event)
  if type(event) ~= "table" or type(event.event) ~= "string" then
    if not session.invalid_stderr then
      session.invalid_stderr = true
      notify("Received unexpected stderr from dictate; verify dictate-cli supports --json-events", vim.log.levels.ERROR)
    end
    return
  end

  if event.event == "session" then
    session.ready_for_signal = true
    if type(event.phase) == "string" then
      set_phase(session, event.phase)
    end
    if session.pending_signal then
      local signal = session.pending_signal
      session.pending_signal = nil
      local ok, err = send_signal(session, signal)
      if not ok then
        notify(("Failed to signal dictate: %s"):format(err), vim.log.levels.ERROR)
      elseif signal == "sigusr1" then
        session.stop_requested = true
      end
    end
    return
  end

  if event.event == "phase" then
    if type(event.phase) == "string" then
      local previous = session.phase
      set_phase(session, event.phase)
      if event.phase ~= previous then
        if event.phase == "transcribing" then
          notify("Transcribing…")
        elseif event.phase == "post_processing" then
          notify("Post-processing…")
        end
      end
    end
    return
  end

  if event.event == "warning" and type(event.message) == "string" then
    notify(event.message, vim.log.levels.WARN)
    return
  end

  if event.event == "result" then
    session.result = event
  end
end

local function consume_stderr(session, data)
  for index, chunk in ipairs(data) do
    if index == 1 then
      session.stderr_tail = session.stderr_tail .. chunk
    else
      if session.stderr_tail ~= "" then
        local ok, event = pcall(vim.json.decode, session.stderr_tail)
        if ok then
          handle_event(session, event)
        else
          handle_event(session, session.stderr_tail)
        end
      end
      session.stderr_tail = chunk
    end
  end
end

local function flush_stderr(session)
  if session.stderr_tail == "" then
    return
  end
  local ok, event = pcall(vim.json.decode, session.stderr_tail)
  if ok then
    handle_event(session, event)
  else
    handle_event(session, session.stderr_tail)
  end
  session.stderr_tail = ""
end

function M.start()
  if state.current then
    notify("Dictation is already active", vim.log.levels.WARN)
    return false
  end

  local bufnr = vim.api.nvim_get_current_buf()
  local allowed, reason = Config.is_buffer_allowed(bufnr)
  if not allowed then
    notify(("Cannot start dictate here: %s"):format(reason), vim.log.levels.WARN)
    return false
  end

  local cursor = vim.api.nvim_win_get_cursor(0)
  local session = {
    stdout = "",
    stdout_tail = "",
    stderr_tail = "",
    phase = "recording",
    stop_requested = false,
    result = nil,
    invalid_stderr = false,
    ready_for_signal = false,
    pending_signal = nil,
    target = {
      bufnr = bufnr,
      extmark = vim.api.nvim_buf_set_extmark(bufnr, namespace, cursor[1] - 1, cursor[2], {
        right_gravity = true,
      }),
    },
  }

  local job_id = vim.fn.jobstart(build_record_command(bufnr, cursor[1] - 1), {
    stdout_buffered = false,
    on_stdout = function(_, data)
      if state.current ~= session or not data then
        return
      end
      append_stdout(session, data)
    end,
    on_stderr = function(_, data)
      if state.current ~= session or not data then
        return
      end
      consume_stderr(session, data)
    end,
    on_exit = function(_, code)
      vim.schedule(function()
        flush_stdout(session)
        flush_stderr(session)
        finalize_session(session, code)
      end)
    end,
  })

  if job_id <= 0 then
    notify("Failed to start dictate", vim.log.levels.ERROR)
    return false
  end

  session.job_id = job_id
  session.pid = vim.fn.jobpid(job_id)
  state.current = session
  state.phase = "recording"
  notify("Recording…")
  return true
end

function M.stop()
  local session = state.current
  if not session then
    notify("Dictation is not active", vim.log.levels.WARN)
    return false
  end

  local signal = (session.phase == "recording" and not session.stop_requested) and "sigusr1" or "sigint"
  if not session.ready_for_signal then
    session.pending_signal = signal
  else
    local ok, err = send_signal(session, signal)
    if not ok then
      notify(("Failed to signal dictate: %s"):format(err), vim.log.levels.ERROR)
      return false
    end
    if signal == "sigusr1" then
      session.stop_requested = true
    end
  end

  if signal == "sigusr1" then
    notify("Stopping recording…")
  else
    notify("Cancelling…")
  end
  return true
end

function M.toggle()
  if state.current then
    return M.stop()
  end
  return M.start()
end

function M.retry()
  if state.current then
    notify("Dictation is already active", vim.log.levels.WARN)
    return false
  end

  local bufnr = vim.api.nvim_get_current_buf()
  local allowed, reason = Config.is_buffer_allowed(bufnr)
  if not allowed then
    notify(("Cannot retry dictate here: %s"):format(reason), vim.log.levels.WARN)
    return false
  end

  local cursor = vim.api.nvim_win_get_cursor(0)
  local session = {
    stdout = "",
    stdout_tail = "",
    stderr_tail = "",
    phase = "retrying",
    result = nil,
    invalid_stderr = false,
    target = {
      bufnr = bufnr,
      extmark = vim.api.nvim_buf_set_extmark(bufnr, namespace, cursor[1] - 1, cursor[2], {
        right_gravity = true,
      }),
    },
  }

  local job_id = vim.fn.jobstart(build_retry_command(bufnr, cursor[1] - 1), {
    stdout_buffered = false,
    on_stdout = function(_, data)
      if state.current ~= session or not data then
        return
      end
      append_stdout(session, data)
    end,
    on_stderr = function(_, data)
      if state.current ~= session or not data then
        return
      end
      consume_stderr(session, data)
    end,
    on_exit = function(_, code)
      vim.schedule(function()
        flush_stdout(session)
        flush_stderr(session)
        finalize_session(session, code)
      end)
    end,
  })

  if job_id <= 0 then
    notify("Failed to start dictate retry", vim.log.levels.ERROR)
    return false
  end

  session.job_id = job_id
  session.pid = vim.fn.jobpid(job_id)
  state.current = session
  state.phase = "retrying"
  notify("Retrying transcription…")
  return true
end

function M.get_state()
  return state.phase
end

function M.get_last_transcript()
  return state.last_transcript
end

function M.reset_for_test()
  state.current = nil
  state.phase = "idle"
  state.last_transcript = nil
end

function M.teardown()
  if state.current and state.current.pid and state.current.pid > 0 then
    pcall(vim.uv.kill, state.current.pid, "sigint")
  end
  M.reset_for_test()
end

return M
