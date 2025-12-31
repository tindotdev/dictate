# Roadmap

## Milestone 1: Core Implementation (Complete)

Initial working implementation of real-time dictation.

- [x] TypeScript daemon with OpenAI Realtime API integration
- [x] PipeWire audio capture via `pw-cat`
- [x] JSONL IPC protocol between daemon and Neovim
- [x] Neovim plugin with ghost text preview
- [x] Text insertion on speech completion
- [x] Basic configuration options

## Milestone 2: Polish & Ergonomics (Complete)

Make the plugin production-ready and pleasant to use.

### Configuration

- [x] Config validation with helpful error messages
- [x] `on_start` / `on_stop` callback hooks
- [x] Buffer/filetype filtering (disable in help, terminal, etc.)

### User Feedback

- [x] Statusline component: `require("dictate").statusline()`
- [x] Improved notifications (shorter, clearer messages)
- [x] Optional integration with `nvim-notify` or similar

### Developer Experience

- [x] `:checkhealth dictate` for troubleshooting
- [x] Better error messages when daemon fails to start
- [x] Debug mode with verbose logging

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
- Explore system-level programming language for the daemon (Rust, Go, Zig) **after everything is stable (no premature optimization)\***
- Custom vocabulary/terminology training
