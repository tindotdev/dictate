# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.9.2] - 2026-05-10

### Miscellaneous

- Update Cargo.lock dependencies


## [1.9.1] - 2026-04-29

### Miscellaneous

- Update Cargo.lock dependencies


## [1.9.0] - 2026-03-24

### Added

- Add --json-events flag for JSONL progress events on stderr
- Add multi-provider support for transcription and post-processing ([#30](https://github.com/tindotdev/dictate/pull/30))


## [1.8.1] - 2026-03-05

### Changed

- Replace dictionary commands with vocab editor ([#22](https://github.com/tindotdev/dictate/pull/22))

### Miscellaneous

- Automate root changelog with release-plz


### Added

- Add `dictate record --stop-after <duration>` as a built-in non-interactive stop path for headless/scripted recording flows

### Changed

- Split launcher stop vs cancel handling so launcher stop-recording uses `SIGUSR1` while `Ctrl+C`/`SIGINT` remain dedicated cancellation paths
- Keep desktop and Kitty launcher smoke tests aligned with the separate stop and cancel signal behavior
- Treat launcher-observed exit `130` as cancellation so launcher-driven transcription cancellation does not surface as a failure

## [1.8.0] - 2026-03-04

### Added

- Add cancellation-aware recording pipeline

## [1.7.0] - 2026-03-03

### Added

- Add opt-in saved-audio retry flow with `--save-last-audio` and `dictate retry`

### Changed

- Use shorter request/retry budgets for direct recording while keeping `dictate retry` as the longer, more persistent reprocessing path
- Make repo-owned launcher assets the canonical source for desktop and Kitty integrations, with shared install/debug workflow
- Add repo-local launcher smoke tests for start/stop/retry behavior

## [1.6.0] - 2026-02-17

### Added

- Add shell completions support
- Add launcher toggle flow with persistent notifications and auto-timeout

## [1.5.0] - 2026-02-17

### Added

- Add post-processing cleanup and evaluation tooling for improving raw Whisper output quality

### Changed

- Rework prompt candidate evaluation to auto-discover test inputs at runtime

### Fixed

- Remove stale prompt includes and ignore generated prompt candidate artifacts

## [1.4.0] - 2026-02-13

### Added

- Add vocabulary management and prompt hint merging

## [1.3.0] - 2026-02-12

### Added

- **Dictionary feature** — persistent dictionary for Whisper prompt injection to improve transcription accuracy
  - Maps commonly mis-transcribed words to their preferred spellings
  - Entries are automatically injected into Whisper's `prompt` parameter during transcription
  - Token budget management ensures dictionary entries fit within Whisper's prompt limits
  - Persistent storage in `~/.local/share/dictate/dictionary.json` (Linux) or `~/Library/Application Support/dictate/dictionary.json` (macOS)
- **`dictate remember` command** — interactive CLI for adding dictionary entries
  - Prompts for "Heard" (mis-transcribed word) and "Preferred" (correct spelling)
  - Confirms before overwriting existing entries
  - Validates input (non-empty, trimmed)
- **`dictate dictionary` command** — manage and view dictionary entries
  - `dictate dictionary list` — display all entries in a formatted table
  - `dictate dictionary clear` — remove all entries (with confirmation)
- **Token counting utilities** — estimate token count for Whisper prompt budget calculations

### Fixed

- Prompt budget calculation now accounts for joiner cost (comma + space) between dictionary entries
- Stabilized `DictionaryStore::open()` test to handle concurrent test execution

### Changed

- Dictionary entries are automatically included in all transcription requests when available
- Dictionary format uses deterministic ordering (`BTreeMap`) for consistent serialization
- **CLI flags** — removed deprecated `--model`; use `--transcription-model`

## [1.1.0] - 2026-02-10

### Added

- **macOS support** — full cross-platform support for macOS alongside Linux
  - Clipboard integration via `pbcopy` (built-in to macOS)
  - Platform-specific audio permission error messages (CoreAudio vs ALSA/PipeWire)
  - CoreAudio error detection for permission-denied scenarios
  - macOS CI integration (clippy + tests on `macos-latest`)
- **Platform-aware error handling** — `permission_denied_help()` returns macOS or Linux-specific troubleshooting guidance

### Changed

- **Platform support** — macOS upgraded from "not yet supported" to full support

## [1.0.0] - 2026-02-09

Complete rewrite from TypeScript/Bun to Rust.

### Added

- **Rust CLI** — one-shot `dictate` command: record → transcribe → clipboard
- **Groq Whisper transcription** — replaces OpenAI Realtime API with Groq's OpenAI-compatible Whisper endpoint
- **Native audio capture** — cpal-based recording with real-time resampling to 16kHz mono
- **Progressive chunking** — overlapping chunks for long recordings with accurate transcription
- **Explicit clipboard behavior** — Wayland-first (`wl-copy`), X11 fallback (`xclip`/`xsel`), never silently loses text
- **Actionable error messages** — permission-denied errors include troubleshooting steps for PipeWire/ALSA
- **Device selection** — `dictate devices` to list inputs, `--device <query>` to select
- **Timestamp support** — `--timestamps word,segment` with `--format verbose_json`
- **Model selection** — `--transcription-model whisper-large-v3-turbo` (default) or `--transcription-model whisper-large-v3`

### Removed

- **TypeScript daemon** — `daemon/` directory and all Bun-based code
- **Lua/Neovim plugin** — `lua/`, `plugin/`, `tests/` (will be rewritten for v1.2)
- **Legacy config files** — `package.json`, `lefthook.yml`, `renovate.json`, `selene.toml`, `stylua.toml`
- **Legacy CI jobs** — Biome, Selene, Stylua, Bun test jobs removed; only Rust CI remains
- **OpenAI Realtime API** — replaced by Groq Whisper

### Changed

- **Version scheme** — `1.0.0` marks the Rust rewrite as a stable major release (follows `0.1.0` → `0.2.0` → `0.3.0` TypeScript lineage)
- **CI pipeline** — simplified to single Rust job (`cargo fmt`, `cargo clippy`, `cargo test`)
- **README** — complete rewrite for Rust CLI usage
- **CONTRIBUTING.md** — updated for Rust toolchain and `just` commands

### Platform Support

- Linux (Wayland + X11) — full support
- macOS — not yet supported (planned)

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

- [1.8.0](https://github.com/tindotdev/dictate/releases/tag/v1.8.0) - 2026-03-04
- [1.7.0](https://github.com/tindotdev/dictate/releases/tag/v1.7.0) - 2026-03-03
- [1.6.0](https://github.com/tindotdev/dictate/releases/tag/v1.6.0) - 2026-02-17
- [1.5.0](https://github.com/tindotdev/dictate/releases/tag/v1.5.0) - 2026-02-17
- [1.4.0](https://github.com/tindotdev/dictate/releases/tag/v1.4.0) - 2026-02-13
- [1.3.0](https://github.com/tindotdev/dictate/releases/tag/v1.3.0) - 2026-02-12
- [1.1.0](https://github.com/tindotdev/dictate/releases/tag/v1.1.0) - 2026-02-10
- [1.0.0](https://github.com/tindotdev/dictate/releases/tag/v1.0.0) - 2026-02-09
- [0.2.0](https://github.com/tindotdev/dictate/releases/tag/v0.2.0) - 2026-01-04

[1.8.0]: https://github.com/tindotdev/dictate/compare/v1.7.0...v1.8.0
[1.7.0]: https://github.com/tindotdev/dictate/compare/v1.6.0...v1.7.0
[1.6.0]: https://github.com/tindotdev/dictate/compare/v1.5.0...v1.6.0
[1.5.0]: https://github.com/tindotdev/dictate/compare/v1.4.0...v1.5.0
[1.4.0]: https://github.com/tindotdev/dictate/compare/v1.3.0...v1.4.0
[1.3.0]: https://github.com/tindotdev/dictate/compare/v1.2.0...v1.3.0
[1.1.0]: https://github.com/tindotdev/dictate/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/tindotdev/dictate/compare/v0.3.0...v1.0.0
[0.2.0]: https://github.com/tindotdev/dictate/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/tindotdev/dictate/releases/tag/v0.1.0
