# Default recipe: list available commands
default:
    @just --list

export TMPDIR := justfile_directory() + "/tmp"

dictate_bin := "target/debug/dictate"

# Format all Rust code
fmt:
    cargo fmt --all

# Check formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Run clippy lints with auto-fix (lint levels configured in workspace Cargo.toml)
clippy:
    cargo clippy --workspace --all-targets --fix --allow-dirty -- -D warnings

# Run all tests
test:
    cargo test --workspace

# Build the dictate CLI binary (debug)
build-cli:
    cargo build -p dictate-cli

# Build all crates (debug)
build:
    cargo build --workspace

# Build all crates (release)
build-release:
    cargo build --workspace --release

# Install dictate CLI to ~/.cargo/bin/
install:
    mkdir -p tmp/
    cargo install --path crates/dictate-cli

# Uninstall dictate CLI from ~/.cargo/bin/
uninstall:
    cargo uninstall dictate-cli

# Configure GROQ_API_KEY for dictate (required for transcription)
add-secret:
    #!/usr/bin/env bash
    set -euo pipefail

    # Check if updating existing key
    if [ -f ~/.config/environment.d/groq.conf ]; then
        echo "Current API key found. You can rotate it by entering a new one."
    fi

    echo "Enter your Groq API key (get one at: https://console.groq.com/keys)"
    read -r -p "GROQ_API_KEY: " api_key

    if [ -z "$api_key" ]; then
        echo "Error: API key cannot be empty"
        exit 1
    fi

    # Create systemd user environment directory
    mkdir -p ~/.config/environment.d

    # Write API key to environment file (overwrites if exists)
    echo "GROQ_API_KEY=$api_key" > ~/.config/environment.d/groq.conf

    # Import into current systemd session (so it takes effect immediately)
    source ~/.config/environment.d/groq.conf
    systemctl --user import-environment GROQ_API_KEY

    echo "✓ API key saved to ~/.config/environment.d/groq.conf"
    echo "✓ Imported into current session (no logout required)"

# Remove GROQ_API_KEY configuration
remove-secret:
    #!/usr/bin/env bash
    set -euo pipefail

    if [ ! -f ~/.config/environment.d/groq.conf ]; then
        echo "No API key found at ~/.config/environment.d/groq.conf"
        exit 0
    fi

    rm ~/.config/environment.d/groq.conf
    echo "✓ Removed ~/.config/environment.d/groq.conf"
    echo "  Note: Restart your session to fully clear the environment variable"

# Install dictate-launch script to ~/.local/bin/ (for global desktop activation)
install-launcher:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p ~/.local/bin
    cp contrib/dictate-launch ~/.local/bin/dictate-launch
    chmod +x ~/.local/bin/dictate-launch
    echo "✓ Installed dictate-launch to ~/.local/bin/dictate-launch"
    echo "  Next: configure your compositor's global shortcut (see README.md)"

# Uninstall dictate-launch script from ~/.local/bin/
uninstall-launcher:
    #!/usr/bin/env bash
    set -euo pipefail

    if [ ! -f ~/.local/bin/dictate-launch ]; then
        echo "No launcher found at ~/.local/bin/dictate-launch"
        exit 0
    fi

    rm ~/.local/bin/dictate-launch
    echo "✓ Removed ~/.local/bin/dictate-launch"
    echo "  Note: Remember to remove the keyboard shortcut from your compositor config"

# Uninstall everything (binary, launcher, and secret)
uninstall-all: uninstall uninstall-launcher remove-secret
    echo ""
    echo "✓ Complete uninstallation finished"

# Run the dictate CLI (debug)
run *ARGS:
    cargo run -p dictate-cli -- {{ARGS}}

# List available audio input devices
devices: build-cli
    {{dictate_bin}} devices

# Alias for muscle memory
device: devices

# Record audio with PipeWire (recommended for USB mics)
record device="": build-cli
    if [ -n "{{device}}" ]; then \
        {{dictate_bin}} record --device "{{device}}"; \
    else \
        {{dictate_bin}} record; \
    fi

# Record audio, then stop cleanly without Ctrl+C (press Enter to stop)
record-clean device="": build-cli
    if [ -n "{{device}}" ]; then \
        {{dictate_bin}} record --device "{{device}}" & pid=$!; \
    else \
        {{dictate_bin}} record & pid=$!; \
    fi; \
    echo "[dictate] press Enter to stop (avoids SIGINT interrupt message from just)"; \
    read -r _; \
    kill -INT "$pid" 2>/dev/null || true; \
    wait "$pid"

# Record audio with default device
record-default: build-cli
    {{dictate_bin}} record

# Record with language hint (e.g., "en", "es", "fr")
record-lang language device="": build-cli
    if [ -n "{{device}}" ]; then \
        {{dictate_bin}} record --language "{{language}}" --device "{{device}}"; \
    else \
        {{dictate_bin}} record --language "{{language}}"; \
    fi

# Record with prompt to guide transcription style/spelling
record-prompt prompt device="": build-cli
    if [ -n "{{device}}" ]; then \
        {{dictate_bin}} record --prompt "{{prompt}}" --device "{{device}}"; \
    else \
        {{dictate_bin}} record --prompt "{{prompt}}"; \
    fi

# Record with specific response format (json, verbose_json, text)
record-format format device="": build-cli
    if [ -n "{{device}}" ]; then \
        {{dictate_bin}} record --format "{{format}}" --device "{{device}}"; \
    else \
        {{dictate_bin}} record --format "{{format}}"; \
    fi

# Record with specific transcription model (whisper-large-v3-turbo, whisper-large-v3)
record-transcription-model model device="": build-cli
    if [ -n "{{device}}" ]; then \
        {{dictate_bin}} record --transcription-model "{{model}}" --device "{{device}}"; \
    else \
        {{dictate_bin}} record --transcription-model "{{model}}"; \
    fi

# Record with all Phase 2 options (language + prompt + format)
record-full language prompt format device="": build-cli
    if [ -n "{{device}}" ]; then \
        {{dictate_bin}} record --language "{{language}}" --prompt "{{prompt}}" --format "{{format}}" --device "{{device}}"; \
    else \
        {{dictate_bin}} record --language "{{language}}" --prompt "{{prompt}}" --format "{{format}}"; \
    fi

# Record with temperature parameter (0.0-1.0)
record-temp temperature device="": build-cli
    if [ -n "{{device}}" ]; then \
        {{dictate_bin}} record --temperature "{{temperature}}" --device "{{device}}"; \
    else \
        {{dictate_bin}} record --temperature "{{temperature}}"; \
    fi

# Record with timestamp granularities (requires verbose_json format)
# Usage: just record-timestamps word,segment
record-timestamps granularities device="": build-cli
    if [ -n "{{device}}" ]; then \
        {{dictate_bin}} record --format verbose_json --timestamps "{{granularities}}" --device "{{device}}"; \
    else \
        {{dictate_bin}} record --format verbose_json --timestamps "{{granularities}}"; \
    fi

# Record with word-level timestamps
record-words device="": build-cli
    if [ -n "{{device}}" ]; then \
        {{dictate_bin}} record --format verbose_json --timestamps word --device "{{device}}"; \
    else \
        {{dictate_bin}} record --format verbose_json --timestamps word; \
    fi

# Record with segment-level timestamps
record-segments device="": build-cli
    if [ -n "{{device}}" ]; then \
        {{dictate_bin}} record --format verbose_json --timestamps segment --device "{{device}}"; \
    else \
        {{dictate_bin}} record --format verbose_json --timestamps segment; \
    fi

# Record with both word and segment timestamps
record-both-timestamps device="": build-cli
    if [ -n "{{device}}" ]; then \
        {{dictate_bin}} record --format verbose_json --timestamps word,segment --device "{{device}}"; \
    else \
        {{dictate_bin}} record --format verbose_json --timestamps word,segment; \
    fi

# Record with LLM post-processing for punctuation and formatting
record-postprocess model="" device="": build-cli
    if [ -n "{{device}}" ]; then \
        if [ -n "{{model}}" ]; then \
            {{dictate_bin}} record --post-process --post-process-model "{{model}}" --device "{{device}}"; \
        else \
            {{dictate_bin}} record --post-process --device "{{device}}"; \
        fi; \
    else \
        if [ -n "{{model}}" ]; then \
            {{dictate_bin}} record --post-process --post-process-model "{{model}}"; \
        else \
            {{dictate_bin}} record --post-process; \
        fi; \
    fi

# Add a dictionary entry (interactive)
remember: build-cli
    {{dictate_bin}} remember

# Print dictionary
dictionary: build-cli
    {{dictate_bin}} dictionary

# Alias
dict: dictionary

# Evaluate post-processing prompt against golden test cases (requires GROQ_API_KEY)
eval-prompt:
    cargo test -p dictate-core golden_eval_against_live_api -- --ignored --nocapture

# Run 5-model × 2-prompt evaluation matrix (requires GROQ_API_KEY)
eval-matrix:
    cargo test -p dictate-core matrix_eval_models_x_prompts -- --ignored --nocapture

# Run all checks (fmt + clippy + test)
check: fmt-check clippy test
