# dictate

Real-time speech-to-text dictation for Neovim and desktop using OpenAI's Realtime transcription API.

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

| Platform | Desktop CLI | Neovim Plugin |
|----------|-------------|---------------|
| Linux (Wayland) | Supported | Supported |
| Linux (X11) | Supported | Supported |
| macOS | In Progress | In Progress |

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
