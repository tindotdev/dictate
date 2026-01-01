# dictate

Real-time speech-to-text dictation for Neovim using OpenAI's Realtime transcription API.

## Features

- `:DictateToggle` starts/stops dictation
- Live ghost text preview as you speak
- Text inserted at cursor on speech completion
- Uses OpenAI's gpt-4o-transcribe model

## Requirements

- Neovim 0.10+
- Node.js 20+ or Bun
- PipeWire with `pw-cat` (`pipewire-utils` package on Fedora)
- OpenAI API key with Realtime API access

## Installation

### System Dependencies

```bash
# Fedora
sudo dnf install pipewire-utils

# Ubuntu/Debian
sudo apt install pipewire

# Install Bun (required for daemon)
curl -fsSL https://bun.sh/install | bash
```

### Method A: Global npm Install (Recommended)

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

### Method B: Clone and Build (For Development)

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

  -- Advanced: Force using global daemon (for developers with local builds)
  use_global_daemon = false,    -- Skip local paths, use only npm-installed daemon
})
```

**Advanced Options:**

- `use_global_daemon`: Set to `true` to force using the globally installed daemon even if a local build exists. Useful for testing the published npm package during development.

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
