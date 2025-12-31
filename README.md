# say

Real-time speech-to-text dictation for Neovim using OpenAI's Realtime transcription API.

## Features

- `:DictateToggle` starts/stops dictation
- Live ghost text preview as you speak
- Text inserted at cursor on speech completion
- Uses OpenAI's gpt-4o-mini-transcribe model

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
```

### Build the Daemon

```bash
cd say/daemon
bun install
bun run build
```

### Neovim Plugin (lazy.nvim)

```lua
{
  dir = "~/path/to/say/nvim",
  keys = {
    { "<Leader>d", "<Cmd>DictateToggle<CR>", desc = "Toggle dictation" },
  },
  cmd = { "DictateToggle", "DictateStart", "DictateStop" },
  opts = {},
}
```

## Configuration

Set your OpenAI API key:

```bash
export OPENAI_API_KEY="sk-..."
```

Optional environment variables:

```bash
export OPENAI_STT_MODEL="gpt-4o-transcribe"  # Default: gpt-4o-mini-transcribe
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
})
```

## Usage

1. Start dictation: `:DictateToggle` (or your configured keymap)
2. Speak - ghost text appears at cursor
3. Pause - text is inserted when speech completes
4. Stop: `:DictateToggle` again

## API

```lua
local dictate = require("dictate")

dictate.is_running()  -- Returns true if daemon is active
dictate.get_state()   -- Returns: 'stopped'|'connecting'|'ready'|'recording'|'error'
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
┌─────────────┐    stdio JSONL    ┌──────────────┐    WebSocket    ┌─────────────┐
│   Neovim    │◄─────────────────►│   Daemon     │◄───────────────►│ OpenAI API  │
│  (Lua)      │                   │ (TypeScript) │                 │  Realtime   │
└─────────────┘                   └──────────────┘                 └─────────────┘
                                        ▲
                                        │ spawn
                                        ▼
                                  ┌──────────────┐
                                  │   pw-cat     │
                                  │  (PipeWire)  │
                                  └──────────────┘
```

## License

MIT
