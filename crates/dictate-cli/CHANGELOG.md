# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
