#!/bin/bash
# Debug script for OpenAI Realtime API
# Tests different model/session format combinations to find working configuration
#
# Prerequisites:
#   - bunx wscat (auto-installed via bun)
#   - OPENAI_API_KEY environment variable
#
# Usage:
#   ./scripts/debug-realtime-api.sh [test_number]
#   ./scripts/debug-realtime-api.sh 1  # Run only test 1
#   ./scripts/debug-realtime-api.sh    # Run all tests

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Check prerequisites
if ! command -v bun &> /dev/null; then
    echo -e "${RED}Error: bun not found${NC}"
    echo "Install with: curl -fsSL https://bun.sh/install | bash"
    exit 1
fi

if [[ -z "${OPENAI_API_KEY:-}" ]]; then
    # Try loading from config
    if [[ -f ~/.config/dictate/env ]]; then
        source ~/.config/dictate/env
    fi
fi

if [[ -z "${OPENAI_API_KEY:-}" ]]; then
    echo -e "${RED}Error: OPENAI_API_KEY not set${NC}"
    echo "Set it with: export OPENAI_API_KEY=sk-..."
    exit 1
fi

REALTIME_URL="wss://api.openai.com/v1/realtime"

# Session update payloads
FLAT_SESSION='{"type":"session.update","session":{"modalities":["text"],"input_audio_format":"pcm16","input_audio_transcription":{"model":"gpt-4o-transcribe"},"turn_detection":{"type":"server_vad","threshold":0.5,"prefix_padding_ms":300,"silence_duration_ms":500}}}'

NESTED_SESSION='{"type":"session.update","session":{"audio":{"input":{"format":{"type":"audio/pcm","rate":24000},"transcription":{"model":"gpt-4o-transcribe"},"turn_detection":{"type":"server_vad","threshold":0.5,"prefix_padding_ms":300,"silence_duration_ms":500}}}}}'

run_test() {
    local test_name="$1"
    local model="$2"
    local session_payload="$3"
    local wait_time="${4:-5}"

    echo -e "\n${BLUE}=== $test_name ===${NC}"
    echo -e "Model: ${YELLOW}$model${NC}"
    echo -e "URL: ${REALTIME_URL}?model=${model}"

    if [[ -n "$session_payload" ]]; then
        echo -e "Session payload: $(echo "$session_payload" | head -c 60)..."
    else
        echo -e "Session payload: ${YELLOW}(none - observe session.created)${NC}"
    fi

    echo -e "\n${GREEN}Response:${NC}"

    local ws_url="${REALTIME_URL}?model=${model}"

    if [[ -n "$session_payload" ]]; then
        # Send session.update with -x and wait for responses
        timeout "$wait_time" bunx wscat -c "$ws_url" \
            -H "Authorization: Bearer $OPENAI_API_KEY" \
            -H "OpenAI-Beta: realtime=v1" \
            -x "$session_payload" \
            -w "$wait_time" 2>&1 || true
    else
        # Just connect and wait for session.created
        timeout "$wait_time" bunx wscat -c "$ws_url" \
            -H "Authorization: Bearer $OPENAI_API_KEY" \
            -H "OpenAI-Beta: realtime=v1" \
            -w "$wait_time" 2>&1 || true
    fi

    echo ""
}

test1_realtime_flat() {
    run_test "Test 1: gpt-4o-realtime-preview + flat format (WORKING)" \
        "gpt-4o-realtime-preview" \
        "$FLAT_SESSION"
}

test2_transcribe_direct() {
    run_test "Test 2: gpt-4o-transcribe in URL (FAILS - not supported)" \
        "gpt-4o-transcribe" \
        ""
}

test3_realtime_observe() {
    run_test "Test 3: gpt-4o-realtime-preview default session" \
        "gpt-4o-realtime-preview" \
        ""
}

test4_nested_format() {
    run_test "Test 4: gpt-4o-realtime-preview + nested format (FAILS)" \
        "gpt-4o-realtime-preview" \
        "$NESTED_SESSION"
}

# Main
echo -e "${GREEN}OpenAI Realtime API Debug Script${NC}"
echo "=================================="
echo ""

if [[ "${1:-}" =~ ^[0-9]+$ ]]; then
    # Run specific test
    case "$1" in
        1) test1_realtime_flat ;;
        2) test2_transcribe_direct ;;
        3) test3_realtime_observe ;;
        4) test4_nested_format ;;
        *) echo "Unknown test: $1"; exit 1 ;;
    esac
else
    # Run all tests
    echo "Running all tests..."
    test1_realtime_flat
    test2_transcribe_direct
    test3_realtime_observe
    test4_nested_format

    echo -e "\n${GREEN}=== Summary ===${NC}"
    echo "Working configuration:"
    echo "  URL: wss://api.openai.com/v1/realtime?model=gpt-4o-realtime-preview"
    echo "  Format: Flat (modalities, input_audio_format, input_audio_transcription)"
    echo ""
    echo "NOT working:"
    echo "  - gpt-4o-transcribe in URL (not supported in realtime mode)"
    echo "  - Nested audio.input format (Unknown parameter: session.audio)"
fi
