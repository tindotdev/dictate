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

## Build

```bash
cd daemon
bun install
bun run build
```

## Unit Tests

```bash
cd daemon
bun test              # Run all 142 tests
bun test --watch      # Watch mode
bun test backoff      # Run specific test file
```

## Manual Testing

### Start Daemon (standalone)

```bash
cd daemon
DEBUG=1 bun src/main.ts
```

### Connect with sayctl

```bash
# In another terminal
cd daemon
bun src/cli/sayctl.ts

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

## Systemd (Optional)

```bash
# Install service
./scripts/install-service.sh

# Enable socket activation
systemctl --user enable say.socket
systemctl --user start say.socket

# Check status
systemctl --user status say.socket
systemctl --user status say.service

# View logs
journalctl --user -u say.service -f
```

## Troubleshooting

### Socket Issues

```bash
# Check socket exists
ls -la $XDG_RUNTIME_DIR/say/

# Remove stale socket
rm $XDG_RUNTIME_DIR/say/say.sock

# Kill orphan daemon
pkill -f "bun.*main"
```

### Audio Issues

```bash
# Test PipeWire directly
pw-cat --record --rate=24000 --channels=1 --format=s16 - | head -c 1000

# Check audio devices
pw-cli list-objects | grep -i node
```

### Debug Logging

```bash
# Daemon debug
DEBUG=1 bun src/main.ts

# Neovim debug
:lua require('dictate').setup({ debug = true })
```
