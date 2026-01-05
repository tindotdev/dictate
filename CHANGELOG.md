# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-01-04

First public release candidate with full Linux and macOS support.

### Added

- **Desktop CLI** - New `dictate` command for one-shot dictation without Neovim
  - `--clipboard` flag (default) to copy transcript to system clipboard
  - `--stdout` flag to print transcript to stdout
  - `--no-clipboard` flag to disable clipboard
  - `--json` flag for JSONL output format (integration-friendly)
  - `--verbose` flag for debug output
- **macOS Support**
  - Audio capture via ffmpeg (avfoundation backend)
  - Clipboard support using built-in `pbcopy`
  - Full CI integration (lint, build, tests)
  - Comprehensive QA verification on macOS 14.6.1
- **Clipboard Backends** - Multi-platform clipboard support with graceful fallback chain
  - Linux Wayland: `wl-copy` (primary)
  - Linux X11: `xclip` (primary) → `xsel` (fallback)
  - macOS: `pbcopy` (built-in)
  - Universal: stdout fallback when no clipboard tool available
- **Auto-start Daemon** - `dictatectl` automatically starts `dictated` when socket is missing
- **Idle Exit** - Daemon exits after 60s idle with no clients (configurable via `DICTATE_IDLE_TIMEOUT_MS`)
- **Systemd-Optional Design** - Daemon can run without systemd on both platforms
- **No-sudo Installation** - Full support for `bunx -p @tindotdev/dictate dictate` workflow
- **Comprehensive Testing**
  - 305 tests passing (10 skipped)
  - CLI integration tests with signal handling (Ctrl+C)
  - Clipboard persistence tests across all backends
  - Package distribution tests
  - macOS-specific test suite

### Fixed

- **Clipboard Persistence** - `wl-copy` now persists after parent process exits
- **Duplicate Shebang** - Removed source shebangs that conflicted with tsup banner
- **Cross-platform Compatibility** - Improved Bun API usage and error handling
- **Flaky Tests** - Eliminated race conditions in CLI integration tests

### Changed

- **Published to npm** - Package available as `@tindotdev/dictate` v0.2.0
- **Platform Support** - macOS fully supported (was experimental)
- **CI/CD** - Added macOS CI job alongside Linux tests
- **Documentation** - Comprehensive QA checklists (P0 and P1) with sign-off

### Documentation

- Desktop CLI usage examples in README and runbook
- macOS installation and setup guide
- Troubleshooting for audio permissions and device selection
- Fallback chain documentation for clipboard backends

## [0.1.0] - 2025-12-XX

Initial development releases (not publicly announced).

### Added

- Core Neovim plugin with ghost text preview
- TypeScript daemon with OpenAI Realtime API integration
- Linux PipeWire audio capture via `pw-cat`
- Unix socket server architecture
- JSONL IPC protocol between daemon and Neovim
- Session ownership for multi-client isolation
- WebSocket reconnection with exponential backoff
- Audio supervisor with auto-restart
- Comprehensive test suite (194+ tests)
- systemd service support (optional)
- npm distribution support with `use_global_daemon` option

### Platform Support

- Linux (Wayland/X11) - Full support
- macOS - Experimental

---

## Release Links

- [0.2.0](https://github.com/tindotdev/dictate/releases/tag/v0.2.0) - 2026-01-04

[0.2.0]: https://github.com/tindotdev/dictate/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/tindotdev/dictate/releases/tag/v0.1.0
