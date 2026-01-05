# dictate

Real-time speech-to-text dictation for Neovim and desktop using OpenAI's Realtime transcription API.

## ⚠️ Privacy & Costs

**Important:** This tool sends your audio to OpenAI's servers for transcription.

- **Audio is transmitted** to OpenAI's Realtime API in real-time
- **Usage is billable** - You will incur OpenAI API costs based on your usage
- Review [OpenAI's pricing](https://openai.com/api/pricing/) for Realtime API costs
- Review [OpenAI's privacy policy](https://openai.com/policies/privacy-policy/) for data handling

Only use this tool with audio content you're comfortable sending to OpenAI.

## Quick Start (bunx)

The fastest way to try dictate without installing:

```bash
# Set your OpenAI API key
export OPENAI_API_KEY="sk-..."

# Run dictate (auto-starts daemon)
bunx -p @tindotdev/dictate dictate
```

Speak into your microphone, press Ctrl+C when done, and the transcript is copied to your clipboard.

## Features

- `:DictateToggle` starts/stops dictation
- Live ghost text preview as you speak
- Text inserted at cursor on speech completion
- Uses OpenAI's gpt-4o-transcribe model

## Requirements

### All Platforms

- Bun runtime (required; Node.js is not supported)
- OpenAI API key with Realtime API access

### Linux

- PipeWire with `pw-cat` (`pipewire-utils` package on Fedora)
- Clipboard: `wl-copy` (Wayland) or `xclip`/`xsel` (X11)
- Neovim 0.10+ (for Neovim integration)

### macOS

- ffmpeg (`brew install ffmpeg`)
- pbcopy (built-in)
- Neovim 0.10+ (for Neovim integration)

## Platform Support

| Platform        | Desktop CLI | Neovim Plugin |
| --------------- | ----------- | ------------- |
| Linux (Wayland) | Supported   | Supported     |
| Linux (X11)     | Supported   | Supported     |
| macOS           | Supported   | Supported     |

## Backend Support

- OpenAI only (Google Cloud is out of scope)

## Installation

### System Dependencies

**Linux (Fedora):**

```bash
sudo dnf install pipewire-utils wl-clipboard  # Wayland
# OR
sudo dnf install pipewire-utils xclip         # X11
```

**Linux (Ubuntu/Debian):**

```bash
sudo apt install pipewire wl-clipboard  # Wayland
# OR
sudo apt install pipewire xclip         # X11
```

**macOS:**

```bash
brew install ffmpeg
```

**All Platforms - Install Bun:**

```bash
curl -fsSL https://bun.sh/install | bash
```

### Desktop CLI (standalone usage)

```bash
# Option A: Run without installing (recommended for trying it out)
bunx -p @tindotdev/dictate dictate

# Option B: Install globally
npm install -g @tindotdev/dictate
dictate  # run directly
```

### Neovim Plugin - Method A: Global npm Install (Recommended)

Install the daemon globally and add the plugin:

```bash
npm install -g @tindotdev/dictate
```

Then add to your lazy.nvim config:

```lua
{
  "tindotdev/dictate",
  subdir = "nvim",
  keys = {
    { "<Leader>d", "<Cmd>DictateToggle<CR>", desc = "Toggle dictation" },
  },
  cmd = { "DictateToggle", "DictateStart", "DictateStop" },
  config = function()
    require("dictate").setup()
  end,
}
```

The plugin will automatically find `dictatectl` in your PATH. Start the daemon:

```bash
# Run manually
dictated &

# Or set up systemd service (optional)
curl -fsSL https://raw.githubusercontent.com/tindotdev/dictate/main/scripts/install-service.sh | bash
```

### Neovim Plugin - Method B: Clone and Build (For Development)

Clone the repository and build locally:

```bash
git clone https://github.com/tindotdev/dictate.git
cd dictate/daemon
bun install
bun run build
```

Add to lazy.nvim pointing to your local clone:

```lua
{
  dir = "~/path/to/dictate",
  subdir = "nvim",
  keys = {
    { "<Leader>d", "<Cmd>DictateToggle<CR>", desc = "Toggle dictation" },
  },
  config = function()
    require("dictate").setup()
  end,
}
```

The plugin will automatically use your local build. Set up the systemd service:

```bash
cd ~/path/to/dictate
./scripts/install-service.sh
```

## Configuration

Set your OpenAI API key:

```bash
export OPENAI_API_KEY="sk-..."
```

Optional environment variables:

```bash
export OPENAI_STT_MODEL="gpt-4o-mini-transcribe"  # Default: gpt-4o-transcribe
export OPENAI_STT_PROMPT="technical terms like Neovim, TypeScript, PipeWire"
export DEBUG=1  # Enable debug logging
```

Plugin options:

```lua
require("dictate").setup({
  daemon_cmd = nil,             -- Auto-detect (or specify explicit path)
  keymap = nil,                 -- Optional: set a keymap (prefer lazy.nvim keys)
  ghost_hl = 'Comment',         -- Highlight group for ghost text
  insert_trailing_space = true, -- Add space after inserted text

  -- Advanced: Force using global npm daemon instead of local build
  use_global_daemon = false,    -- false=prefer local build, true=use npm package
})
```

**Advanced Options:**

- `use_global_daemon`: Controls which daemon binary is used:
  - `false` (default): Auto-detect, preferring local build at `plugin_dir/../daemon/dist`
  - `true`: Force using globally installed `@tindotdev/dictate` from npm

  Note: This only affects the daemon binary. The Neovim plugin Lua code always uses whatever is loaded by lazy.nvim (local `dir` or GitHub repo).

## Usage

1. Start dictation: `:DictateToggle` (or your configured keymap)
2. Speak - ghost text appears at cursor
3. Pause - text is inserted when speech completes
4. Stop: `:DictateToggle` again

## API

```lua
local dictate = require("dictate")

dictate.is_running()   -- Returns true if dictatectl process is active
dictate.get_state()    -- Returns: 'stopped'|'connecting'|'connected'|'idle'|'listening'|'error'
dictate.is_active()    -- Returns true if actively listening
dictate.is_audio_ok()  -- Returns true if audio capture is working
dictate.is_ws_ok()     -- Returns true if WebSocket is connected
```

## Commands

- `:DictateToggle` - Toggle dictation on/off
- `:DictateStart` - Start dictation
- `:DictateStop` - Stop dictation

## Troubleshooting

### Common Issues

#### "No audio capture" or microphone not working

**Linux:**

```bash
# Check if PipeWire is running
pw-cat --version

# Test audio capture directly
pw-cat --record --rate=24000 --channels=1 --format=s16 - | head -c 1000

# List available audio devices
pw-cli list-objects | grep -i node
```

**macOS:**

```bash
# Check if ffmpeg is installed
ffmpeg -version

# List available audio devices
ffmpeg -f avfoundation -list_devices true -i ""

# Test audio capture (5 seconds)
ffmpeg -f avfoundation -i ":0" -t 5 test.wav

# Grant microphone permission in System Preferences > Privacy & Security > Microphone
```

#### "Clipboard unavailable" warnings

**Linux Wayland:**

```bash
# Install wl-clipboard
sudo dnf install wl-clipboard    # Fedora
sudo apt install wl-clipboard    # Ubuntu/Debian

# Test clipboard
echo "test" | wl-copy && wl-paste
```

**Linux X11:**

```bash
# Install xclip or xsel
sudo dnf install xclip           # Fedora
sudo apt install xclip           # Ubuntu/Debian

# Test clipboard
echo "test" | xclip -selection clipboard && xclip -o -selection clipboard
```

**Fallback:** If no clipboard tool is available, use `--stdout --no-clipboard` to print transcript to stdout.

#### Daemon won't start or "Connection refused"

```bash
# Check if daemon is running
ps aux | grep dictated

# Check for stale socket (Linux)
ls -la $XDG_RUNTIME_DIR/dictate/

# Check for stale socket (macOS)
ls -la ~/.local/state/dictate/

# Remove stale socket and restart
rm $XDG_RUNTIME_DIR/dictate/dictate.sock  # Linux
rm ~/.local/state/dictate/dictate.sock    # macOS

# Kill any orphaned daemon
pkill -f dictated
```

#### Neovim: "dictatectl not found"

```bash
# Check if daemon is installed globally
which dictatectl

# If using local build, ensure it's built
cd daemon && bun run build

# Use :checkhealth to diagnose
:checkhealth dictate
```

#### API errors or "Authentication failed"

```bash
# Verify API key is set
echo $OPENAI_API_KEY

# Check API key validity at https://platform.openai.com/api-keys

# Enable debug logging
DEBUG=1 dictate --verbose
```

### Getting Help

- Check `:checkhealth dictate` in Neovim for detailed diagnostics
- See [runbook.md](docs/runbook.md) for detailed testing commands
- Enable debug mode: `DEBUG=1` environment variable
- [Report issues](https://github.com/tindotdev/dictate/issues) on GitHub

## Support

Need help or want to report an issue? Here's how to get support:

### Before Opening an Issue

1. **Check existing issues** - Search [existing issues](https://github.com/tindotdev/dictate/issues) to see if your problem has already been reported
2. **Run health check** - In Neovim, run `:checkhealth dictate` for diagnostic information
3. **Check troubleshooting** - Review the [Troubleshooting](#troubleshooting) section above
4. **Enable debug mode** - Run with `DEBUG=1` environment variable for verbose logging

### Reporting Bugs

Found a bug? [Open a bug report](https://github.com/tindotdev/dictate/issues/new/choose) with:

- Description of the issue
- Steps to reproduce
- Expected vs actual behavior
- Your environment (OS, Bun version, installation method)
- Output from `:checkhealth dictate` (if using Neovim)
- Any error messages or logs

### Feature Requests

Have an idea for an improvement? [Open a feature request](https://github.com/tindotdev/dictate/issues/new/choose) describing:

- The problem you're trying to solve
- Your proposed solution
- Why this would benefit other users

### Contributing

Interested in contributing code or documentation? See [CONTRIBUTING.md](CONTRIBUTING.md) for:

- Development setup instructions
- Testing guidelines
- Pull request process

### Security Issues

**Do not report security vulnerabilities in public issues.** See [SECURITY.md](SECURITY.md) for how to report security issues privately.

## Development

```bash
# Run daemon in development mode
cd daemon
export OPENAI_API_KEY="..."
bun dev

# Run tests
bun test

# Build for production
bun run build
```

## Architecture

```
┌─────────────┐  stdio   ┌────────────┐  Unix Socket  ┌──────────────┐  WebSocket  ┌─────────────┐
│   Neovim    │◄────────►│ dictatectl │◄─────────────►│    Daemon    │◄───────────►│ OpenAI API  │
│   (Lua)     │  JSONL   │  (bridge)  │               │ (TypeScript) │             │  Realtime   │
└─────────────┘          └────────────┘               └──────────────┘             └─────────────┘
                                                             ▲
                                                             │ supervises
                                                             ▼
                                                      ┌──────────────┐
                                                      │    pw-cat    │
                                                      │  (PipeWire)  │
                                                      └──────────────┘
```

The daemon runs as a standalone service (optionally via systemd) and survives Neovim restarts.
Multiple Neovim instances can connect to the same daemon.

## License

MIT
