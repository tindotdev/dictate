# Roadmap

## Definition of Done (v1)

This project is considered **complete** when the items below are true (not when every nice-to-have is built):

### End-to-end (Neovim)

- [x] Linux: Live partial transcript (ghost text) works while speaking
- [x] Linux: Final transcript is inserted at cursor on completion
- [ ] macOS: Live partial transcript (ghost text) works while speaking
- [ ] macOS: Final transcript is inserted at cursor on completion

### End-to-end (Desktop)

- [ ] Linux: One command can capture mic audio and produce text without Neovim (CLI)
- [ ] Linux: Desktop mode can write the final transcript to the **system clipboard** (Wayland/X11) and/or stdout
- [ ] macOS: One command can capture mic audio and produce text without Neovim (CLI)
- [ ] macOS: Desktop mode can write the final transcript to the **system clipboard** and/or stdout

### Providers

- [x] OpenAI-only backend is supported (no provider swapping required for v1)
- [ ] Partial + final transcripts are reliable (and match Neovim + desktop UX)

### Installation / “One Command”

- [ ] No `sudo` required for install/run (OS mic permission prompts are expected on macOS)
- [ ] Systemd is **optional** (Linux). The project works without a background service on both platforms.
- [ ] Bun is the supported runtime (document `bunx` as the canonical install/run path)

---

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
- [x] Multi-client session ownership (only initiating client receives transcripts)
- [x] 194 unit/integration tests covering all components

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

## Milestone 5: Cross‑Platform v1 (In Progress)

Deliver the v1 “done line”: Linux + macOS support, desktop/clipboard usage, OpenAI-only, and Bun-only.

### Desktop (not just Neovim)

- [ ] Add a first-party user CLI (e.g. `dictate`) for “dictate once → clipboard/stdout”
- [ ] Implement clipboard backends with clear fallback: `pbcopy` (macOS) → `wl-copy` (Wayland) → `xclip`/`xsel` (X11) → stdout
- [ ] Add CLI flags: `--clipboard` (default), `--stdout`, `--no-clipboard`
- [ ] Document desktop usage for Linux + macOS (examples + troubleshooting)

### macOS Support

- [ ] Add macOS audio capture using an external dependency (`ffmpeg` via Homebrew)
- [ ] Default to the system microphone (no config required)
- [ ] Support selecting the macOS input device (and document how to list devices)
- [ ] Document mic permission prompts + common failure modes
- [ ] Add macOS CI job (at minimum: lint + unit tests)

### Provider (OpenAI)

- [x] OpenAI Realtime transcription integration works (partial + final transcripts)
- [x] Handles reconnect/backoff and basic auth errors
- [x] Document required env vars and recommended models

### No‑Systemd “Single Command” Path

- [ ] Make `dictatectl` auto-start `dictated` when the socket is missing, then connect
- [ ] Add daemon idle-exit (e.g. exit after N seconds with 0 clients and not listening)
- [ ] Update Neovim health/docs to treat systemd as optional (not required)

### Packaging / Distribution (Bun-only)

- [ ] Provide a “single command” happy path that does not require systemd (daemon can be started automatically)
- [ ] Support a no-sudo install/run flow using `bunx` (e.g. `bunx -p @tindotdev/dictate dictate`)
- [ ] Keep Linux systemd installer as an optional convenience (not required for v1)
- [x] Keep `dictatectl` bundled with the daemon package for v1 (no split)

## Public-ready checklist

General open-source readiness tasks before announcing broadly.

### Infrastructure & Publishing

- [x] Published daemon to npm as `@tindotdev/dictate`
- [x] Tested installation from npm registry
- [x] Added `use_global_daemon` option for published package
- [ ] Make GitHub repository public
- [x] Add CI/CD pipeline (GitHub Actions)
- [ ] Set up automated npm publishing on release

### Documentation

- [ ] Add CONTRIBUTING guide
- [ ] Add Code of Conduct
- [ ] Add SECURITY policy / vulnerability reporting guidance
- [ ] Add support/contact or issue triage guidance
- [ ] Add CHANGELOG / release notes
- [ ] Add Privacy & costs note in README (audio sent to OpenAI; usage is billable)
- [ ] Clarify platform support (Linux + macOS) in README
- [x] Update README with npm installation instructions
- [ ] Add troubleshooting section to README
- [x] Ensure lazy.nvim install instructions use `subdir = "nvim"`

### Quality Assurance

- [ ] Review and clean up all TODOs in codebase
- [ ] Verify all tests pass in CI
- [ ] Test installation flow on fresh Linux system
- [ ] Test installation flow on fresh macOS system
