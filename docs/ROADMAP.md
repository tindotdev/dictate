# Roadmap

## Summary

**Status:** v1 features are **complete** ✅ - Public release preparation in progress.

**What's Done:**

- Full Linux + macOS support (Neovim + Desktop CLI)
- All core features working and tested (305 tests passing)
- Published to npm as @tindotdev/dictate v0.2.0
- Zero TODOs/FIXMEs in codebase
- Essential documentation (CHANGELOG, SECURITY, troubleshooting, privacy warnings) ✅

**What Remains for Public Launch:**

1. ~~Documentation (CHANGELOG, privacy/cost warnings, troubleshooting)~~ ✅ **Complete**
2. Open-source governance (CODE_OF_CONDUCT, CONTRIBUTING, issue templates)
3. Make repository public
4. Final QA on fresh installations

**Next Priority:** Focus on **Milestone 6 - Priority 2** (Open Source Governance)

---

## Definition of Done (v1) - ✅ COMPLETE

This project is considered **complete** when the items below are true (not when every nice-to-have is built):

### End-to-end (Neovim)

- [x] Linux: Live partial transcript (ghost text) works while speaking
- [x] Linux: Final transcript is inserted at cursor on completion
- [x] macOS: Live partial transcript (ghost text) works while speaking
- [x] macOS: Final transcript is inserted at cursor on completion

### End-to-end (Desktop)

- [x] Linux: One command can capture mic audio and produce text without Neovim (CLI)
- [x] Linux: Desktop mode can write the final transcript to the **system clipboard** (Wayland/X11) and/or stdout
- [x] macOS: One command can capture mic audio and produce text without Neovim (CLI)
- [x] macOS: Desktop mode can write the final transcript to the **system clipboard** and/or stdout

### Providers

- [x] OpenAI-only backend is supported (no provider swapping required for v1)
- [x] Partial + final transcripts are reliable (and match Neovim + desktop UX)

### Installation / "One Command"

- [x] No `sudo` required for install/run (OS mic permission prompts are expected on macOS)
- [x] Systemd is **optional** (Linux). The project works without a background service on both platforms.
- [x] Bun is the supported runtime (document `bunx` as the canonical install/run path)

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

## Milestone 5: Cross‑Platform v1 (Complete ✅)

Deliver the v1 "done line": Linux + macOS support, desktop/clipboard usage, OpenAI-only, and Bun-only.

### Desktop (not just Neovim)

- [x] Add a first-party user CLI (e.g. `dictate`) for "dictate once → clipboard/stdout"
- [x] Implement clipboard backends with clear fallback: `pbcopy` (macOS) → `wl-copy` (Wayland) → `xclip`/`xsel` (X11) → stdout
- [x] Add CLI flags: `--clipboard` (default), `--stdout`, `--no-clipboard`, `--json`, `--verbose`
- [x] Document desktop usage for Linux + macOS (examples in README + runbook)

### macOS Support

- [x] Add macOS audio capture using an external dependency (`ffmpeg` via Homebrew)
- [x] Default to the system microphone (no config required)
- [x] Document mic permission prompts + common failure modes (see runbook.md)
- [x] Add macOS CI job (lint + build + tests passing)
- [ ] Document how to select alternative macOS input device (nice-to-have)

### Provider (OpenAI)

- [x] OpenAI Realtime transcription integration works (partial + final transcripts)
- [x] Handles reconnect/backoff and basic auth errors
- [x] Document required env vars and recommended models

### No‑Systemd "Single Command" Path

- [x] Make `dictatectl` auto-start `dictated` when the socket is missing, then connect
- [x] Add daemon idle-exit (exit after 60s with 0 clients and not listening)
- [x] Update Neovim health/docs to treat systemd as optional (not required)

### Packaging / Distribution (Bun-only)

- [x] Provide a "single command" happy path that does not require systemd (daemon auto-starts)
- [x] Support a no-sudo install/run flow using `bunx` (e.g. `bunx -p @tindotdev/dictate dictate`)
- [x] Keep Linux systemd installer as an optional convenience (not required for v1)
- [x] Keep `dictatectl` bundled with the daemon package for v1 (no split)

### Testing & Quality

- [x] 305 tests passing (Linux + macOS)
- [x] P0 QA complete (both platforms)
- [x] P1 QA complete (both platforms)
- [x] Zero TODOs/FIXMEs in codebase
- [x] Published to npm as @tindotdev/dictate v0.2.0

## Milestone 6: Public Release (In Progress)

Prepare for public announcement and open-source community engagement.

### Priority 1: Essential Documentation (Complete ✅)

- [x] **CHANGELOG.md** - Document release history starting with v0.2.0
- [x] **Privacy & Cost Warning** - Add prominent note in README that audio is sent to OpenAI and incurs API costs
- [x] **Platform Support Table** - Update README table to show macOS as "Supported" (was "In Progress")
- [x] **Troubleshooting Guide** - Expand README troubleshooting section with common issues and solutions
- [x] **SECURITY.md** - Add vulnerability reporting policy and security contact

### Priority 2: Open Source Governance (Required for Public Release)

- [ ] **Make GitHub repository public**
- [ ] **CODE_OF_CONDUCT.md** - Adopt standard Contributor Covenant or similar
- [ ] **CONTRIBUTING.md** - Document how to contribute (setup, testing, PR process)
- [ ] **Issue Templates** - Add bug report and feature request templates
- [ ] **Support Policy** - Document how users can get help (GitHub Issues, Discussions, etc.)

### Priority 3: Automation & Publishing (Nice-to-have)

- [ ] **Automated npm publishing** - GitHub Action to publish on git tag
- [ ] **Release workflow** - Document or automate the release process
- [ ] **Version bumping** - Consider using changesets or similar for version management

### Priority 4: Final Quality Assurance (Before First Public Announcement)

- [x] Review and clean up all TODOs in codebase (✅ Zero TODOs found)
- [x] Verify all tests pass in CI (✅ 305 pass, 10 skip, 0 fail)
- [ ] **Fresh Linux installation test** - Verify `bunx -p @tindotdev/dictate dictate` works on clean Ubuntu/Fedora
- [ ] **Fresh macOS installation test** - Verify installation and mic permissions on clean macOS system
- [ ] **Documentation review** - Have someone unfamiliar with the project try to install and use it

### Infrastructure (Complete ✅)

- [x] Published daemon to npm as `@tindotdev/dictate`
- [x] Tested installation from npm registry
- [x] Added `use_global_daemon` option for published package
- [x] Add CI/CD pipeline (GitHub Actions) - lint, test-daemon, test-daemon-macos, test-lua
- [x] Update README with npm installation instructions
- [x] Ensure lazy.nvim install instructions use `subdir = "nvim"`
