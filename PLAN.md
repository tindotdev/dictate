# Implementation Plan: "say" - Neovim Dictation Plugin

## Overview

Build a real-time speech-to-text dictation system for Neovim using OpenAI's Realtime transcription API.

**Target UX**: `:DictateToggle` starts/stops dictation. As you speak, text appears at the cursor (live streaming). On pause/stop, the segment finalizes.

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

## Repository Structure

```
say/
  package.json              # Bun workspace root
  daemon/
    package.json
    tsconfig.json
    src/
      main.ts               # Entry point, orchestration
      protocol.ts           # JSONL message schemas (Zod)
      pipewire.ts           # Audio capture via pw-cat
      realtime.ts           # WebSocket to OpenAI
      config.ts             # Environment/config loading
    dist/                   # Built output
  nvim/
    plugin/
      dictate.lua           # Auto-load commands
    lua/dictate/
      init.lua              # Main module, setup()
      job.lua               # Daemon lifecycle
      ui.lua                # Ghost text rendering
      config.lua            # User configuration
```

## Implementation Phases

### Phase 1: Project Scaffold

1. Create directory structure
2. Initialize Bun workspace with `package.json`
3. Create `daemon/package.json` with dependencies:
   - Runtime: `ws`, `zod`
   - Dev: `typescript`, `tsx`, `tsup`, `vitest`, `@types/node`, `@types/ws`
4. Create `daemon/tsconfig.json`
5. Create `daemon/tsup.config.ts` for building

### Phase 2: Daemon - Protocol Layer

**File: `daemon/src/protocol.ts`**
- Define JSONL schemas with Zod:
  - Client → Daemon: `start`, `stop`, `config`
  - Daemon → Client: `status`, `delta`, `final`, `speech_started`, `speech_stopped`, `error`, `debug`
- `emit()` helper to write to stdout

**File: `daemon/src/config.ts`**
- Load from environment: `OPENAI_API_KEY`, `OPENAI_STT_MODEL`, `OPENAI_STT_PROMPT`, `DEBUG`
- Validate with Zod
- Defaults: model=`gpt-4o-mini-transcribe`, VAD threshold=0.5, prefix=300ms, silence=500ms

### Phase 3: Daemon - Audio Capture

**File: `daemon/src/pipewire.ts`**
- Spawn `pw-cat --record --rate=24000 --channels=1 --format=s16 -`
- Buffer stdout into 20ms frames (960 bytes)
- Emit `chunk` events with raw PCM buffers
- Handle process lifecycle (error, close)

### Phase 4: Daemon - OpenAI Realtime

**File: `daemon/src/realtime.ts`**
- Connect to `wss://api.openai.com/v1/realtime?model=gpt-realtime`
- Send `session.update` with transcription config on open
- Handle incoming events:
  - `input_audio_buffer.speech_started` → emit `speech_started`
  - `conversation.item.input_audio_transcription.delta` → emit `delta`
  - `conversation.item.input_audio_transcription.completed` → emit `final`
- `sendAudio(base64)` method for streaming

### Phase 5: Daemon - Main Orchestration

**File: `daemon/src/main.ts`**
- Wire audio chunks → base64 → WebSocket
- Wire WebSocket events → JSONL stdout
- Read stdin for `start`/`stop` commands
- Track accumulated text per `item_id`
- Graceful shutdown on SIGTERM/SIGINT

### Phase 6: Neovim Plugin

**Note:** The Lua module is named `dictate` (not `say`) to avoid conflict with plenary.nvim's `say` module.

**File: `nvim/lua/dictate/config.lua`**
- Default options: `daemon_cmd`, `keymap`, `ghost_hl`, `insert_trailing_space`
- Auto-detect daemon path (dist or dev)

**File: `nvim/lua/dictate/job.lua`**
- `jobstart()` daemon process
- Parse JSONL from stdout
- Dispatch to `ui.lua` handlers
- `send()` JSONL to stdin
- State tracking: stopped/connecting/ready/recording

**File: `nvim/lua/dictate/ui.lua`**
- On `speech_started`: capture cursor position
- On `delta`: show ghost text (inline `virt_text`) at cursor
- On `final`: clear ghost, insert text at captured position, advance cursor

**File: `nvim/lua/dictate/init.lua`**
- `setup(opts)` function
- Register `:DictateToggle`, `:DictateStart`, `:DictateStop` commands
- Optional keymap (`<Leader>d`)

**File: `nvim/plugin/dictate.lua`**
- Guard against double-load
- Provide `:DictateSetup` fallback command

### Phase 7: Testing

**Daemon unit tests** (`vitest`):
- Protocol schema validation
- Audio chunking logic

**Plugin tests** (`plenary.nvim`):
- Ghost text creation/removal
- Final text insertion
- JSONL parsing

## Key Technical Details

| Component | Detail |
|-----------|--------|
| Audio format | PCM s16 mono 24kHz |
| Chunk size | 20ms (960 bytes) |
| WebSocket URL | `wss://api.openai.com/v1/realtime?model=gpt-realtime` |
| Session type | `transcription` |
| VAD | server_vad, threshold 0.5, prefix 300ms, silence 500ms |
| IPC | stdio JSONL (newline-delimited JSON) |

## Files to Create

1. `say/package.json` - workspace root
2. `say/daemon/package.json` - daemon deps
3. `say/daemon/tsconfig.json` - TS config
4. `say/daemon/tsup.config.ts` - build config
5. `say/daemon/src/protocol.ts` - JSONL schemas
6. `say/daemon/src/config.ts` - env config
7. `say/daemon/src/pipewire.ts` - audio capture
8. `say/daemon/src/realtime.ts` - WebSocket client
9. `say/daemon/src/main.ts` - orchestration
10. `say/nvim/lua/dictate/config.lua` - plugin config
11. `say/nvim/lua/dictate/job.lua` - daemon management
12. `say/nvim/lua/dictate/ui.lua` - ghost text rendering
13. `say/nvim/lua/dictate/init.lua` - plugin entry
14. `say/nvim/plugin/dictate.lua` - auto-load

## Milestone 1 Scope (This Implementation)

Focus on **ghost text + final commit** approach:
- Show live transcription as virtual text overlay
- Insert final text only on speech completion
- Single undo operation per utterance
- Avoid complex buffer manipulation during streaming

## Future Milestones (Not in scope)

- Milestone 2: Insert-as-you-speak with extmarks
- Milestone 3: Logprobs confidence UI
- Milestone 4: Statusline, push-to-talk, markdown-aware

## Environment Requirements

```bash
# System
sudo dnf install pipewire-utils  # for pw-cat

# Runtime
export OPENAI_API_KEY="sk-..."
export OPENAI_STT_MODEL="gpt-4o-mini-transcribe"  # optional
export OPENAI_STT_PROMPT="..."  # optional

# Neovim setup (LazyVim example)
{ dir = "~/path/to/say/nvim", config = function() require("dictate").setup() end }
```
