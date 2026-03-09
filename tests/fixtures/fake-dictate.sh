#!/usr/bin/env bash
set -euo pipefail

print_help() {
	cat <<'EOF'
dictate record
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

phase="recording"

emit() {
	printf '%s\n' "$1" >&2
}

emit_session() {
	emit '{"event":"session","mode":"record","phase":"recording","stop_after_ms":null}'
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

trap 'phase="transcribing"' USR1
trap 'cancelled' INT

emit_session

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
