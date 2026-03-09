# Default recipe: list available commands
default:
    @just --list

export TMPDIR := justfile_directory() + "/tmp"

dictate_bin := "target/debug/dictate"
nvim_test_files := "tests/config_spec.lua tests/session_spec.lua tests/health_spec.lua"
manual_nvim_init := "tests/manual/init.lua"
manual_nvim_fixture_config_dir := "tests/manual/fixtures/config/dictate"
manual_nvim_tmp_root := "tmp/nvim-dev-real"

_ensure-tmp:
    mkdir -p "{{ justfile_directory() }}/tmp"

# Format all Rust code
fmt: fmt-rust fmt-nvim

fmt-rust: _ensure-tmp
    cargo fmt --all

# Format Markdown docs with global prettier
fmt-md:
    prettier --write AGENTS.md PLANS.md README.md

# Check formatting without modifying files
fmt-check: fmt-rust-check fmt-nvim-check

fmt-rust-check: _ensure-tmp
    cargo fmt --all -- --check

# Format Neovim plugin code
fmt-nvim:
    stylua lua tests

# Check Neovim plugin formatting without modifying files
fmt-nvim-check:
    stylua --check lua tests

# Format launcher and shell test scripts (requires shfmt)
fmt-launchers:
    shfmt -w \
        contrib/launchers/dictate-launch-common.sh \
        contrib/launchers/dictate-launch \
        contrib/launchers/dictate-kitty \
        contrib/dictate-launch \
        contrib/dictate-kitty \
        tests/launchers/run.sh

# Run clippy lints with auto-fix (lint levels configured in workspace Cargo.toml)
clippy: _ensure-tmp
    cargo clippy --workspace --all-targets --fix --allow-dirty -- -D warnings

# Run Rust tests only
test-rust: _ensure-tmp
    cargo test --workspace

# Run Neovim plugin tests
test-nvim:
    chmod +x tests/fixtures/fake-dictate.sh tests/fixtures/fake-dictate-no-json.sh
    nvim --headless -l tests/run.lua {{nvim_test_files}}

# Launch a minimal Neovim profile wired to the fake dictate fixture.
nvim-dev-fake scenario="success" transcript="hello from fixture":
    chmod +x tests/fixtures/fake-dictate.sh
    env \
        DICTATE_NVIM_MODE=fake \
        DICTATE_FIXTURE_SCENARIO="{{scenario}}" \
        DICTATE_FIXTURE_TRANSCRIPT="{{transcript}}" \
        nvim --clean -u {{manual_nvim_init}}

# Launch a minimal Neovim profile wired to the local debug dictate binary.
nvim-dev-real: build-cli
    mkdir -p {{manual_nvim_tmp_root}}/config/dictate {{manual_nvim_tmp_root}}/data
    cp {{manual_nvim_fixture_config_dir}}/vocabulary.json {{manual_nvim_tmp_root}}/config/dictate/vocabulary.json
    cp {{manual_nvim_fixture_config_dir}}/dictionary.json {{manual_nvim_tmp_root}}/config/dictate/dictionary.json
    env \
        DICTATE_NVIM_MODE=real \
        XDG_CONFIG_HOME="{{ justfile_directory() }}/{{manual_nvim_tmp_root}}/config" \
        XDG_DATA_HOME="{{ justfile_directory() }}/{{manual_nvim_tmp_root}}/data" \
        nvim --clean -u {{manual_nvim_init}}

# Run launcher integration tests with fake binaries
test-launchers:
    bash tests/launchers/run.sh

# Lint launcher and shell test scripts (requires shellcheck)
lint-launchers:
    shellcheck \
        contrib/launchers/dictate-launch-common.sh \
        contrib/launchers/dictate-launch \
        contrib/launchers/dictate-kitty \
        contrib/dictate-launch \
        contrib/dictate-kitty \
        tests/launchers/run.sh

# Lint Neovim plugin Lua code
lint-nvim:
    selene lua tests

# Run all tests
test: test-rust test-launchers test-nvim

# Build the dictate CLI binary (debug)
build-cli: _ensure-tmp
    cargo build -p dictate-cli

# Build all crates (debug)
build: _ensure-tmp
    cargo build --workspace

# Build all crates (release)
build-release: _ensure-tmp
    cargo build --workspace --release

# Install dictate CLI to ~/.cargo/bin/
install: _ensure-tmp
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

# Install the desktop launcher to ~/.local/bin/
install-launcher:
    #!/usr/bin/env bash
    set -euo pipefail
    dest_dir="${DICTATE_LAUNCHER_INSTALL_DIR:-$HOME/.local/bin}"
    mkdir -p "$dest_dir"
    cp contrib/launchers/dictate-launch "$dest_dir/dictate-launch"
    cp contrib/launchers/dictate-launch-common.sh "$dest_dir/dictate-launch-common.sh"
    chmod +x "$dest_dir/dictate-launch" "$dest_dir/dictate-launch-common.sh"
    echo "✓ Installed dictate-launch to $dest_dir/dictate-launch"
    echo "  Next: configure your compositor's global shortcut (see README.md)"

# Install both desktop and Kitty launchers plus their shared library.
install-launchers:
    #!/usr/bin/env bash
    set -euo pipefail
    dest_dir="${DICTATE_LAUNCHER_INSTALL_DIR:-$HOME/.local/bin}"
    mkdir -p "$dest_dir"
    cp contrib/launchers/dictate-launch "$dest_dir/dictate-launch"
    cp contrib/launchers/dictate-kitty "$dest_dir/dictate-kitty"
    cp contrib/launchers/dictate-launch-common.sh "$dest_dir/dictate-launch-common.sh"
    chmod +x \
        "$dest_dir/dictate-launch" \
        "$dest_dir/dictate-kitty" \
        "$dest_dir/dictate-launch-common.sh"
    echo "✓ Installed launchers to $dest_dir"
    echo "  - dictate-launch"
    echo "  - dictate-kitty"
    echo "  - dictate-launch-common.sh"

# Install only the Kitty launcher.
install-launcher-kitty:
    #!/usr/bin/env bash
    set -euo pipefail
    dest_dir="${DICTATE_LAUNCHER_INSTALL_DIR:-$HOME/.local/bin}"
    mkdir -p "$dest_dir"
    cp contrib/launchers/dictate-kitty "$dest_dir/dictate-kitty"
    cp contrib/launchers/dictate-launch-common.sh "$dest_dir/dictate-launch-common.sh"
    chmod +x "$dest_dir/dictate-kitty" "$dest_dir/dictate-launch-common.sh"
    echo "✓ Installed dictate-kitty to $dest_dir/dictate-kitty"

# Uninstall the desktop launcher from ~/.local/bin/
uninstall-launcher:
    #!/usr/bin/env bash
    set -euo pipefail
    dest_dir="${DICTATE_LAUNCHER_INSTALL_DIR:-$HOME/.local/bin}"

    if [ ! -f "$dest_dir/dictate-launch" ]; then
        echo "No launcher found at $dest_dir/dictate-launch"
        exit 0
    fi

    rm "$dest_dir/dictate-launch"
    echo "✓ Removed $dest_dir/dictate-launch"
    echo "  Note: Remember to remove the keyboard shortcut from your compositor config"

# Uninstall both launchers and their shared library from the install dir.
uninstall-launchers:
    #!/usr/bin/env bash
    set -euo pipefail
    dest_dir="${DICTATE_LAUNCHER_INSTALL_DIR:-$HOME/.local/bin}"
    rm -f \
        "$dest_dir/dictate-launch" \
        "$dest_dir/dictate-kitty" \
        "$dest_dir/dictate-launch-common.sh"
    echo "✓ Removed launcher files from $dest_dir"

# Uninstall everything (binary, launchers, and secret)
uninstall-all: uninstall uninstall-launchers remove-secret
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

# Record audio, then stop cleanly without Ctrl+C by sending the launcher stop signal.
record-clean device="": build-cli
    if [ -n "{{device}}" ]; then \
        {{dictate_bin}} record --device "{{device}}" & pid=$!; \
    else \
        {{dictate_bin}} record & pid=$!; \
    fi; \
    echo "[dictate] press Enter to stop (sends SIGUSR1 instead of cancellation)"; \
    read -r _; \
    kill -USR1 "$pid" 2>/dev/null || true; \
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

# Run multi-model × multi-prompt evaluation matrix (requires GROQ_API_KEY)
eval-matrix:
    cargo test -p dictate-core matrix_eval_models_x_prompts -- --ignored --nocapture

# Run all checks (fmt + clippy + test)
check: fmt-check clippy lint-nvim test
