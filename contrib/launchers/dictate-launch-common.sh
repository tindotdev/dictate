#!/usr/bin/env bash

# Shared helpers for launcher adapters. Source from a Bash script that already
# enabled strict mode.

launcher_init() {
	local prefix="$1"

	export PATH="$HOME/.cargo/bin:$PATH"

	export STATE_DIR="${DICTATE_STATE_DIR:-${XDG_RUNTIME_DIR:-/tmp}}"
	export PIDFILE="$STATE_DIR/${prefix}.pid"
	export STARTED_AT_FILE="$STATE_DIR/${prefix}.started-at"
	export STATE_FILE="$STATE_DIR/${prefix}.state"
	export WORKER_PID_FILE="$STATE_DIR/${prefix}.worker.pid"
	export NOTIFY_ID_FILE="$STATE_DIR/${prefix}.notify-id"
	export OUTFILE="$STATE_DIR/${prefix}.out"

	export DICTATE_BIN="${DICTATE_BIN:-dictate}"
	export TRANSCRIPTION_MODEL="${DICTATE_TRANSCRIPTION_MODEL:-whisper-large-v3}"
	export MAX_DURATION="${DICTATE_TIMEOUT:-180}"
	export TRANSCRIBE_TIMEOUT="${DICTATE_TRANSCRIBE_TIMEOUT:-45}"
	export MIN_RECORD_SECONDS="${DICTATE_MIN_RECORD_SECONDS:-1}"

	mkdir -p "$STATE_DIR"
	launcher_init_debug "$prefix"
}

launcher_init_debug() {
	local prefix="$1"
	LAUNCHER_NAME="$prefix"
	LAUNCH_LOG="${DICTATE_LAUNCH_LOG:-}"

	if [[ -n "$LAUNCH_LOG" ]]; then
		mkdir -p "$(dirname "$LAUNCH_LOG")"
		touch "$LAUNCH_LOG"
	fi

	if [[ "${DICTATE_LAUNCH_TRACE:-0}" == "1" ]]; then
		if [[ -n "$LAUNCH_LOG" ]]; then
			exec 3>>"$LAUNCH_LOG"
			export BASH_XTRACEFD=3
		fi
		set -x
	fi
}

log() {
	[[ -n "${LAUNCH_LOG:-}" ]] || return 0
	printf '[%s] [%s] %s\n' \
		"$(date '+%Y-%m-%d %H:%M:%S')" \
		"${LAUNCHER_NAME:-launcher}" \
		"$*" >>"$LAUNCH_LOG"
}

set_state() {
	printf '%s\n' "$1" >"$STATE_FILE"
	log "state=$1"
}

get_state() {
	cat "$STATE_FILE" 2>/dev/null || true
}

clear_state() {
	rm -f "$STATE_FILE" "$WORKER_PID_FILE"
	log "cleared state files"
}

clear_runtime_files() {
	rm -f "$PIDFILE" "$STARTED_AT_FILE" "$NOTIFY_ID_FILE" "$OUTFILE"
	clear_state
	log "cleared runtime files"
}

signal_dictate() {
	local signal="$1" pid="$2"
	log "signal=$signal pid=$pid"
	kill "-$signal" -- "-$pid" 2>/dev/null || kill "-$signal" "$pid" 2>/dev/null
}

stop_recording_dictate() {
	signal_dictate USR1 "$1"
}

cancel_dictate() {
	signal_dictate INT "$1"
}

worker_is_running() {
	[[ -f "$WORKER_PID_FILE" ]] || return 1
	local worker_pid
	worker_pid=$(<"$WORKER_PID_FILE") || return 1
	kill -0 "$worker_pid" 2>/dev/null
}

clear_stale_state_if_needed() {
	[[ -f "$STATE_FILE" ]] || return 0
	[[ -f "$PIDFILE" ]] && return 0
	worker_is_running && return 0
	rm -f "$STARTED_AT_FILE"
	clear_state
	log "cleared stale state"
}

run_transcription_command() {
	local extra_args=(--transcription-model "$TRANSCRIPTION_MODEL")
	if [[ "${1:-}" != "retry" ]]; then
		extra_args=(--save-last-audio "${extra_args[@]}")
	fi

	log "exec: $DICTATE_BIN $* ${extra_args[*]}"
	"$DICTATE_BIN" "$@" "${extra_args[@]}"
}

parse_mode() {
	MODE="record"
	if [[ "${1:-}" == "retry" ]]; then
		MODE="retry"
		shift
	fi

	# shellcheck disable=SC2034
	REMAINING_ARGS=("$@")
	export MODE
}
