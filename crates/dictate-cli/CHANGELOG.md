# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.8.0] - 2026-03-04

### Added

- Add cancellation-aware recording pipeline ([#19](https://github.com/tindotdev/dictate/pull/19))



### Added

- Add `dictate record --stop-after <duration>` as a built-in non-interactive stop path for headless/scripted recording flows

### Changed

- Split launcher stop vs cancel handling so launcher stop-recording uses `SIGUSR1` while `Ctrl+C`/`SIGINT` remain dedicated cancellation paths
- Keep desktop and Kitty launcher smoke tests aligned with the separate stop and cancel signal behavior
- Treat launcher-observed exit `130` as cancellation so launcher-driven transcription cancellation does not surface as a failure

## [1.7.0] - 2026-03-03

### Added

- Add saved-audio retry flow and canonical launcher smoke tests ([#15](https://github.com/tindotdev/dictate/pull/15))



### Added

- Add opt-in saved-audio retry flow with `--save-last-audio` and `dictate retry`

### Changed

- Use shorter request/retry budgets for direct recording while keeping `dictate retry` as the longer, more persistent reprocessing path
- Make repo-owned launcher assets the canonical source for desktop and Kitty integrations, with shared install/debug workflow
- Add repo-local launcher smoke tests for start/stop/retry behavior

## [1.6.0] - 2026-02-17

### Added

- Add shell completions support ([#8](https://github.com/tindotdev/dictate/pull/8))



## [1.4.0] - 2026-02-13

### Added

- Add vocabulary management and prompt hint merging ([#3](https://github.com/tindotdev/dictate//pull/3))
