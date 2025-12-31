# Roadmap

## Milestone 1: Core Implementation (Complete)

Initial working implementation of real-time dictation.

- [x] TypeScript daemon with OpenAI Realtime API integration
- [x] PipeWire audio capture via `pw-cat`
- [x] JSONL IPC protocol between daemon and Neovim
- [x] Neovim plugin with ghost text preview
- [x] Text insertion on speech completion
- [x] Basic configuration options

## Milestone 2: Polish & Ergonomics

Make the plugin production-ready and pleasant to use.

### Configuration
- [ ] Config validation with helpful error messages
- [ ] `on_start` / `on_stop` callback hooks
- [ ] Buffer/filetype filtering (disable in help, terminal, etc.)

### User Feedback
- [ ] Statusline component: `require("dictate").statusline()`
- [ ] Improved notifications (shorter, clearer messages)
- [ ] Optional integration with `nvim-notify` or similar

### Developer Experience
- [ ] `:checkhealth dictate` for troubleshooting
- [ ] Better error messages when daemon fails to start
- [ ] Debug mode with verbose logging

## Milestone 3: Robustness

Handle edge cases and failures gracefully.

- [ ] Daemon auto-respawn on unexpected exit
- [ ] WebSocket reconnection with exponential backoff
- [ ] Graceful degradation when API key missing or invalid
- [ ] Memory/resource cleanup for long sessions
- [ ] Handle API rate limits and quota errors

## Milestone 4: Advanced Features

Power-user features for specific workflows.

- [ ] Push-to-talk mode (hold key to dictate)
- [ ] Confidence indicators (show logprobs/uncertainty)
- [ ] Markdown-aware transcription prompts
- [ ] Transcription history/log
- [ ] Insert-as-you-speak mode (live text insertion)

## Milestone 5: Ecosystem

Integration with the broader Neovim ecosystem.

- [ ] Telescope picker for transcription history
- [ ] Which-key integration
- [ ] Lualine/statusline preset
- [ ] Documentation in vimdoc format (`:help dictate`)

---

## Ideas Backlog

Features that may be explored in the future:

- Voice commands (e.g., "new line", "delete that")
- Multi-language support
- Custom vocabulary/terminology training
- Audio input device selection
- Recording/playback of audio segments
- Integration with other AI models (Whisper local, etc.)
