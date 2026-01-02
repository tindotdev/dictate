# Runbook

Quick reference for development and testing commands.

## Prerequisites

```bash
# Verify PipeWire
pw-cat --version

# Verify bun
bun --version

# Set API key
export OPENAI_API_KEY="sk-..."
```

## Desktop CLI Usage

The `dictate` CLI is a one-shot dictation tool that copies transcripts to clipboard.

### Basic Usage

```bash
# Start dictation, copy to clipboard when done
dictate

# Print to stdout instead of clipboard
dictate --stdout

# Both clipboard and stdout
dictate --stdout

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
bunx -p @tindotdev/dictate dictate --stdout --verbose
```

### Integration Examples

```bash
# Append to a file
dictate --stdout --no-clipboard >> notes.txt

# Pipe to another command
dictate --stdout --no-clipboard | tr '[:lower:]' '[:upper:]'

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
bun test              # Run all 194 tests
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

**Test coverage (24 tests):**
- 4.1: Multiple clients connecting simultaneously
- 4.2: Status broadcast to all clients, transcripts to owner only
- 4.3: Client disconnect handling (owner disconnect stops session)
- 4.4: Duplicate command handling
- 4.5: Session ownership (SESSION_BUSY errors, ownership lifecycle)

**Architecture:**
```
TestClient 1 ──┐
TestClient 2 ──┼──▶ SocketServer ──▶ StateMachine
TestClient N ──┘    (real)          (real)
                       │                │
                       ▼                ▼
            MockAudioSupervisor    NetworkSupervisor
            (EventEmitter)         (local WS mock)
```

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
pkill -f "bun.*main"
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
# - "Permission denied": Grant microphone access in System Preferences > Privacy
# - "No such device": Check device index with list_devices command
```

### Debug Logging

```bash
# Daemon debug
DEBUG=1 bun src/main.ts

# Neovim debug
:lua require('dictate').setup({ debug = true })
```
