# Runbook

Quick reference for development and troubleshooting.

## Prerequisites

```bash
# Verify bun
bun --version

# Set API key
export OPENAI_API_KEY="sk-..."
```

Platform notes:

- Linux: verify PipeWire tools with `pw-cat --version`
- macOS: install `ffmpeg` and verify with `ffmpeg -version`

## Desktop CLI Usage

The `dictate` CLI is a one-shot dictation tool.

### Basic Usage

```bash
# Copy final transcript to clipboard (default)
dictate

# Print to stdout (and still copy to clipboard)
dictate --stdout

# Print to stdout only (no clipboard)
dictate --no-clipboard

# JSONL output for scripting
dictate --json

# Verbose mode (debug output)
dictate --verbose
```

### Using bunx (no install)

```bash
# Run without installing
bunx -p @tindotdev/dictate dictate

# With options
bunx -p @tindotdev/dictate dictate --no-clipboard --verbose
```

### Integration Examples

```bash
# Append to a file
dictate --no-clipboard >> notes.txt

# Pipe to another command
dictate --no-clipboard | tr '[:lower:]' '[:upper:]'

# Use in a script with JSON
dictate --json | jq '.text' -r
```

### Stopping Dictation

- Press `Ctrl+C` once: Stops listening, waits for final transcript
- Press `Ctrl+C` twice: Force quit immediately

## Build

```bash
cd daemon
bun install
bun run build
```

## Unit Tests

```bash
cd daemon
bun test              # Run all daemon tests
bun test --watch      # Watch mode
bun test backoff      # Run specific test file
```

## Integration Tests

Multi-client integration tests verify daemon behavior with multiple simultaneous
connections. These use mocked supervisors (no real audio/OpenAI) for fast CI runs.

```bash
cd daemon
bun test src/__tests__/integration/    # Run integration tests only
```

Integration tests cover multi-client session ownership (status to all clients; transcripts to the owner only).

## Manual Testing

### Start Daemon (standalone)

```bash
cd daemon
DEBUG=1 bun src/main.ts
```

### Connect with dictatectl

```bash
# In another terminal
cd daemon
bun src/cli/dictatectl.ts

# Send commands via stdin:
{"type":"start_listening"}
{"type":"stop_listening"}
```

### Test from Neovim

```vim
:checkhealth dictate    " Verify setup
:DictateStart           " Start dictation
:DictateStop            " Stop dictation
:DictateToggle          " Toggle on/off

" Check state
:lua print(require('dictate.job').get_state())
:lua print(require('dictate.job').is_audio_ok())
:lua print(require('dictate.job').is_ws_ok())
```

## Systemd

```bash
# Install service (creates lazy.nvim symlink by default)
./scripts/install-service.sh

# Or skip the lazy.nvim symlink
./scripts/install-service.sh --no-lazy

# Check status
systemctl --user status dictate.service

# View logs
journalctl --user -u dictate.service -f

# Restart after code changes
cd daemon && bun run build
systemctl --user restart dictate.service
```

The service auto-starts on login and runs continuously.

## Troubleshooting

### Socket Issues

```bash
# Check socket exists
ls -la $XDG_RUNTIME_DIR/dictate/

# Remove stale socket
rm $XDG_RUNTIME_DIR/dictate/dictate.sock

# Kill orphan daemon
pkill -f dictated
```

### Audio Issues (Linux)

```bash
# Test PipeWire directly
pw-cat --record --rate=24000 --channels=1 --format=s16 - | head -c 1000

# Check audio devices
pw-cli list-objects | grep -i node
```

### Audio Issues (macOS)

```bash
# List available audio devices
ffmpeg -f avfoundation -list_devices true -i ""

# Test audio capture (5 seconds)
ffmpeg -f avfoundation -i ":0" -t 5 test.wav

# Common issues:
# - "Permission denied": Grant microphone access in System Settings > Privacy & Security > Microphone
# - "No such device": Check device index with list_devices command
```

### Debug Logging

```bash
# Daemon debug
DEBUG=1 bun src/main.ts

# Neovim debug
:lua require('dictate').setup({ debug = true })
```
