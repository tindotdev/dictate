#!/usr/bin/env bash
set -euo pipefail

print_help() {
	cat <<'EOF'
dictate record
  --json-events
  --save-last-audio
dictate retry
  --json-events
EOF
}

for arg in "$@"; do
	if [[ "$arg" == "--help" ]]; then
		print_help
		exit 0
	fi
done

scenario="${DICTATE_FIXTURE_SCENARIO:-success}"
transcript="${DICTATE_FIXTURE_TRANSCRIPT:-fixture transcript}"

command="record"
for arg in "$@"; do
	if [[ "$arg" == "retry" ]]; then
		command="retry"
		break
	fi
done

if [[ "$command" == "retry" ]]; then
	phase="retrying"
else
	phase="recording"
fi

emit() {
	printf '%s\n' "$1" >&2
}

emit_session() {
	local mode="$1"
	emit "{\"event\":\"session\",\"mode\":\"$mode\",\"phase\":\"$phase\",\"stop_after_ms\":null}"
}

emit_phase() {
	local value="$1"
	emit "{\"event\":\"phase\",\"phase\":\"$value\",\"chunk_count\":1,\"model\":null}"
}

emit_result() {
	local status="$1"
	emit "{\"event\":\"result\",\"status\":\"$status\"}"
}

cancelled() {
	emit_result "cancelled"
	exit 130
}

if [[ "$command" == "retry" ]]; then
	# Retry is one-shot, no signal handling needed
	emit_session "retry"

	if [[ "$scenario" == "fail_immediately" ]]; then
		emit '{"event":"result","status":"failed","message":"retry failed","causes":["no saved recording"]}'
		exit 1
	fi

	emit_phase "transcribing"

	if [[ "$scenario" == "cancel_during_transcribing" ]]; then
		while true; do
			sleep 0.05
		done
	fi

	sleep 0.05
	printf '%s\n' "$transcript"
	emit '{"event":"result","status":"completed","char_count":18,"copied_to_clipboard":false}'
	exit 0
fi

# Record command with signal handling
trap 'phase="transcribing"' USR1
trap 'cancelled' INT

emit_session "record"

if [[ "$scenario" == "fail_immediately" ]]; then
	emit '{"event":"result","status":"failed","message":"fixture failure","causes":[]}'
	exit 1
fi

while [[ "$phase" == "recording" ]]; do
	sleep 0.05
done

emit_phase "transcribing"

if [[ "$scenario" == "cancel_during_transcribing" ]]; then
	while true; do
		sleep 0.05
	done
fi

if [[ "$scenario" == "post_process" ]]; then
	sleep 0.05
	emit '{"event":"phase","phase":"post_processing","chunk_count":null,"model":"openai/gpt-oss-20b"}'
fi

sleep 0.05
printf '%s\n' "$transcript"
emit '{"event":"result","status":"completed","char_count":18,"copied_to_clipboard":false}'
