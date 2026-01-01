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

## Milestone 3: Robustness (Complete)

Socket-based daemon architecture with fault tolerance.

- [x] Daemon runs as standalone service (systemd-supervised)
- [x] dictatectl bridge connects Neovim to daemon via Unix socket
- [x] WebSocket reconnection with exponential backoff
- [x] Audio supervisor with auto-restart on pw-cat crash
- [x] Graceful degradation when API key missing or invalid
- [x] 142 unit tests covering all components

## Milestone 4: Distribution & Packaging (Complete)

Make the project installable and usable from published sources.

- [x] Migrate from sandbox to production directory (`/github`)
- [x] Remove hardcoded paths from systemd service and scripts
- [x] Multi-source daemon discovery (local build → npm global → PATH → dev)
- [x] Publish daemon to npm as `@tindotdev/dictate`
- [x] Add `use_global_daemon` config option for daily-driving published package
- [x] Fix duplicate shebang issue in published binaries
- [x] Verified installation and testing with Linuxbrew Node
- [x] Health check reports global vs local daemon usage

## Milestone 5: Advanced Features

Power-user features for specific workflows.

- [ ] Push-to-talk mode (hold key to dictate)
- [ ] Confidence indicators (show logprobs/uncertainty)
- [ ] Markdown-aware transcription prompts
- [ ] Transcription history/log
- [ ] Insert-as-you-speak mode (live text insertion)

## Milestone 6: Ecosystem

Integration with the broader Neovim ecosystem.

- [ ] `noice.nvim` integration for loading spinner while recording
- [ ] Documentation in vimdoc format (`:help dictate`)

---

## Public-ready checklist

General open-source readiness tasks before announcing broadly.

### Infrastructure & Publishing

- [x] Published daemon to npm as `@tindotdev/dictate`
- [x] Tested installation from npm registry
- [x] Added `use_global_daemon` option for published package
- [ ] Make GitHub repository public
- [ ] Add CI/CD pipeline (GitHub Actions)
- [ ] Set up automated npm publishing on release

### Documentation

- [ ] Add CONTRIBUTING guide
- [ ] Add Code of Conduct
- [ ] Add SECURITY policy / vulnerability reporting guidance
- [ ] Add support/contact or issue triage guidance
- [ ] Add CHANGELOG / release notes
- [ ] Add Privacy & costs note in README (audio sent to OpenAI; usage is billable)
- [ ] Clarify platform support (Linux + PipeWire) in README
- [ ] Update README with npm installation instructions
- [ ] Add troubleshooting section to README
- [ ] Ensure lazy.nvim install instructions use `subdir = "nvim"`

### Quality Assurance

- [ ] Review and clean up all TODOs in codebase
- [ ] Verify all tests pass in CI
- [ ] Test installation flow on fresh Linux system

---

## Ideas Backlog

Features that may be explored in the future:

- Voice commands (e.g., "new line", "delete that")
- Explore system-level programming language for the daemon (Rust, Go, Zig) **after everything is stable (no premature optimization)\***
- Custom vocabulary/terminology training
