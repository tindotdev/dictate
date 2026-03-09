# Add Neovim Integration

## Purpose

Add an in-repo `dictate.nvim` plugin that controls `dictate-cli` directly from Neovim, with lazy.nvim-friendly setup and a machine-readable stderr event stream for reliable phase tracking.

## Progress

- [x] Add `--json-events` to `dictate record` and `dictate retry`
- [x] Implement `dictate.nvim` with start/stop/toggle, buffer insertion, and health checks
- [x] Add Lua tests, formatting, linting, and CI coverage
- [x] Run the full validation stack and fix follow-up issues

## Decision Log

- Decision: Use `dictate-cli` directly instead of reintroducing a daemon.
  Rationale: The Rust CLI already supports stop/cancel via `SIGUSR1` and `SIGINT`; keeping one process contract is simpler and matches the existing launcher design.

- Decision: Emit JSONL events on stderr behind `--json-events`.
  Rationale: Neovim needs stable phase information without parsing human-readable stderr output.

- Decision: Default Neovim insertion to buffer text, not clipboard.
  Rationale: Editor integration should not clobber the user’s clipboard by default.

- Decision: Keep Lua validation lightweight with `stylua`, `selene`, fixture-based Neovim tests, and a minimal `tests/run.lua` harness.
  Rationale: A simple in-repo harness reduced tooling friction while still covering the plugin behavior and signal handling contract.

## Context

- Rust integration lives in `crates/dictate-cli/src/main.rs` and `crates/dictate-cli/src/commands/record.rs`.
- Neovim plugin code lives under `lua/dictate/`.
- Neovim tests live under `tests/` and use fake `dictate` fixtures that respond to Unix signals.
- User-facing docs live in `README.md`.

## Plan of Work

Implemented the Rust event mode, wired the Lua plugin to it, added health checks and fixture-based Neovim integration tests, and folded Lua formatting/linting/test commands into `just` and CI.

## Validation

Run from the repo root:

- `just fmt`
- `just clippy`
- `just test`
- `just check`
- `just fmt-nvim`
- `just lint-nvim`
- `just test-nvim`
- `just nvim-dev-real`
