#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"

fail() {
	echo "FAIL: $*" >&2
	exit 1
}

wait_for() {
	local description="$1"
	local command="$2"
	local attempts="${3:-50}"
	local delay="${4:-0.1}"

	local attempt
	for ((attempt = 0; attempt < attempts; attempt += 1)); do
		if eval "$command"; then
			return 0
		fi
		sleep "$delay"
	done

	fail "timed out waiting for ${description}"
}

assert_contains() {
	local file="$1"
	local pattern="$2"
	grep -F -- "$pattern" "$file" >/dev/null || fail "expected '$pattern' in $file"
}

assert_not_exists() {
	local path="$1"
	[[ ! -e "$path" ]] || fail "expected $path to be absent"
}

setup_env() {
	TEST_ROOT="$(mktemp -d)"
	HOME="$TEST_ROOT/home"
	STATE_DIR="$TEST_ROOT/state"
	mkdir -p "$HOME/.cargo/bin" "$HOME/.local/bin" "$HOME/repos/brew/bin" "$STATE_DIR"

	export HOME STATE_DIR
	export DICTATE_STATE_DIR="$STATE_DIR"
	export DICTATE_LAUNCH_LOG="$TEST_ROOT/launcher.log"
	export DICTATE_BIN="$HOME/.cargo/bin/dictate"
	export FAKE_DICTATE_LOG="$TEST_ROOT/dictate.log"
	export FAKE_DICTATE_SIGNAL_LOG="$TEST_ROOT/dictate-signals.log"
	export FAKE_NOTIFY_LOG="$TEST_ROOT/notify.log"
	export FAKE_GDBUS_LOG="$TEST_ROOT/gdbus.log"
	export FAKE_KITTEN_LOG="$TEST_ROOT/kitten.log"
	export FAKE_KITTEN_CONTENT_LOG="$TEST_ROOT/kitten-content.log"
	export FAKE_DICTATE_STDOUT="typed text"
	export FAKE_RETRY_STDOUT="retry text"
	export PATH="$HOME/.cargo/bin:$PATH"

	create_fake_binaries
}

run_launcher() {
	setsid "$@" >/dev/null 2>&1
}

cleanup_env() {
	local pidfile="$STATE_DIR/dictate.pid"
	local kitty_pidfile="$STATE_DIR/dictate-kitty.pid"

	if [[ -f "$pidfile" ]]; then
		kill "$(cat "$pidfile")" 2>/dev/null || true
	fi
	if [[ -f "$kitty_pidfile" ]]; then
		kill "$(cat "$kitty_pidfile")" 2>/dev/null || true
	fi

	rm -rf "$TEST_ROOT"
}

create_fake_binaries() {
	cat >"$HOME/.cargo/bin/dictate" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exec python3 - "$@" <<'PY'
import os
import signal
import sys
import time

args = sys.argv[1:]
with open(os.environ["FAKE_DICTATE_LOG"], "a", encoding="utf-8") as handle:
    handle.write(" ".join(args) + "\n")

stdout_mode = "--stdout" in args
retry_mode = "retry" in args

if retry_mode:
    if stdout_mode:
        sys.stdout.write(os.environ.get("FAKE_RETRY_STDOUT", "retry text"))
        sys.stdout.flush()
    raise SystemExit(int(os.environ.get("FAKE_DICTATE_RETRY_EXIT_CODE", "0")))

def handle_signal(signum, _frame):
    name = signal.Signals(signum).name
    with open(os.environ["FAKE_DICTATE_SIGNAL_LOG"], "a", encoding="utf-8") as handle:
        handle.write(f"signal:{name}\n")
    if stdout_mode:
        sys.stdout.write(os.environ.get("FAKE_DICTATE_STDOUT", "typed text"))
        sys.stdout.flush()
    raise SystemExit(0)

signal.signal(signal.SIGINT, handle_signal)
signal.signal(signal.SIGTERM, handle_signal)

while True:
    time.sleep(0.1)
PY
EOF

	cat >"$HOME/.cargo/bin/notify-send" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$FAKE_NOTIFY_LOG"
for arg in "$@"; do
    if [[ "$arg" == "--print-id" ]]; then
        echo 42
        break
    fi
done
EOF

	cat >"$HOME/.cargo/bin/gdbus" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$FAKE_GDBUS_LOG"
EOF

	cat >"$HOME/.cargo/bin/wl-paste" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s' "${FAKE_CLIPBOARD_TEXT:-clipboard text}"
EOF

	cat >"$HOME/.cargo/bin/kitten" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$FAKE_KITTEN_LOG"

from_file=""
prev=""
for arg in "$@"; do
    if [[ "$prev" == "--from-file" ]]; then
        from_file="$arg"
        break
    fi
    prev="$arg"
done

if [[ -n "$from_file" ]]; then
    cat "$from_file" >>"$FAKE_KITTEN_CONTENT_LOG"
fi
EOF

	chmod +x \
		"$HOME/.cargo/bin/dictate" \
		"$HOME/.cargo/bin/notify-send" \
		"$HOME/.cargo/bin/gdbus" \
		"$HOME/.cargo/bin/wl-paste" \
		"$HOME/.cargo/bin/kitten"
}

desktop_start_stop() {
	setup_env
	trap cleanup_env RETURN

	run_launcher "$REPO_ROOT/contrib/dictate-launch" --language en
	wait_for "desktop recording state" "[[ \$(cat \"$STATE_DIR/dictate.state\") == recording ]]"
	wait_for "desktop pidfile" "[[ -f \"$STATE_DIR/dictate.pid\" ]]"
	wait_for "desktop dictate argv" "[[ -f \"$FAKE_DICTATE_LOG\" ]] && grep -F -- '-p --language en --save-last-audio --transcription-model whisper-large-v3' \"$FAKE_DICTATE_LOG\" >/dev/null"

	sleep 1.1
	run_launcher "$REPO_ROOT/contrib/dictate-launch"
	wait_for "desktop INT signal" "[[ -f \"$FAKE_DICTATE_SIGNAL_LOG\" ]] && grep -F 'signal:SIGINT' \"$FAKE_DICTATE_SIGNAL_LOG\" >/dev/null"
	wait_for "desktop cleanup" "[[ ! -e \"$STATE_DIR/dictate.state\" && ! -e \"$STATE_DIR/dictate.pid\" ]]"
	assert_contains "$FAKE_NOTIFY_LOG" "Recording…"
	assert_contains "$FAKE_NOTIFY_LOG" "Transcribing…"
}

desktop_retry() {
	setup_env
	trap cleanup_env RETURN

	run_launcher "$REPO_ROOT/contrib/dictate-launch" retry --language fr
	wait_for "desktop retry command" "[[ -f \"$FAKE_DICTATE_LOG\" ]] && grep -F 'retry -p --language fr --save-last-audio --transcription-model whisper-large-v3' \"$FAKE_DICTATE_LOG\" >/dev/null"
	wait_for "desktop retry cleanup" "[[ ! -e \"$STATE_DIR/dictate.state\" ]]"
	assert_contains "$FAKE_NOTIFY_LOG" "Retrying transcription…"
	assert_contains "$FAKE_NOTIFY_LOG" "Copied to clipboard"
}

kitty_start_stop() {
	setup_env
	trap cleanup_env RETURN
	export KITTY_LISTEN_ON="unix:/tmp/fake-kitty.sock"

	run_launcher "$REPO_ROOT/contrib/dictate-kitty" --language en
	wait_for "kitty recording state" "[[ \$(cat \"$STATE_DIR/dictate-kitty.state\") == recording ]]"
	wait_for "kitty pidfile" "[[ -f \"$STATE_DIR/dictate-kitty.pid\" ]]"
	wait_for "kitty dictate argv" "[[ -f \"$FAKE_DICTATE_LOG\" ]] && grep -F -- '--stdout -p --language en --save-last-audio --transcription-model whisper-large-v3' \"$FAKE_DICTATE_LOG\" >/dev/null"

	sleep 1.1
	run_launcher "$REPO_ROOT/contrib/dictate-kitty"
	wait_for "kitty INT signal" "[[ -f \"$FAKE_DICTATE_SIGNAL_LOG\" ]] && grep -F 'signal:SIGINT' \"$FAKE_DICTATE_SIGNAL_LOG\" >/dev/null"
	wait_for "kitty send-text content" "[[ -f \"$FAKE_KITTEN_CONTENT_LOG\" ]] && grep -F 'typed text' \"$FAKE_KITTEN_CONTENT_LOG\" >/dev/null"
	wait_for "kitty cleanup" "[[ ! -e \"$STATE_DIR/dictate-kitty.state\" && ! -e \"$STATE_DIR/dictate-kitty.pid\" ]]"
}

kitty_retry() {
	setup_env
	trap cleanup_env RETURN
	export KITTY_LISTEN_ON="unix:/tmp/fake-kitty.sock"

	run_launcher "$REPO_ROOT/contrib/dictate-kitty" retry --language de
	wait_for "kitty retry command" "[[ -f \"$FAKE_DICTATE_LOG\" ]] && grep -F 'retry --stdout -p --language de --save-last-audio --transcription-model whisper-large-v3' \"$FAKE_DICTATE_LOG\" >/dev/null"
	wait_for "kitty retry content" "[[ -f \"$FAKE_KITTEN_CONTENT_LOG\" ]] && grep -F 'retry text' \"$FAKE_KITTEN_CONTENT_LOG\" >/dev/null"
	wait_for "kitty retry cleanup" "[[ ! -e \"$STATE_DIR/dictate-kitty.state\" ]]"
}

main() {
	desktop_start_stop
	echo "ok - desktop start/stop"

	desktop_retry
	echo "ok - desktop retry"

	kitty_start_stop
	echo "ok - kitty start/stop"

	kitty_retry
	echo "ok - kitty retry"
}

main "$@"
