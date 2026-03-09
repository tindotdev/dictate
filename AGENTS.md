# Dictate Development Guide

`dictate` is a cross-platform (Linux Wayland/X11 and macOS) voice-to-text CLI.
It records microphone audio, transcribes with Groq Whisper, and outputs text to the clipboard and/or stdout.

This guide applies to the repository root and all subdirectories unless a deeper `AGENTS.md` overrides it.

## ExecPlans

- Use an ExecPlan for cross-cutting features, significant refactors, or supported integration contract changes.
- Do not use one for small, local fixes or docs-only changes.
- Follow `PLANS.md` for when to use one and how to keep it concise.

## Project Scope

- Features: vocabulary management, dictionary/corrections, output formats (JSON, timestamps), post-processing.
- Pure Rust workspace under `crates/`.
  - `crates/dictate-core`: audio, transcription, clipboard, post-processing.
  - `crates/dictate-cli`: CLI entrypoint and user-facing commands.

## Development Workflow

- Use `just` as the standard task interface:
  - `just fmt`
  - `just clippy`
  - `just test`
  - `just check`
  - `just run --stdout`
- When adding dependencies, use `cargo add` rather than editing `Cargo.toml` by hand.
- Definition of done (unless the user asks otherwise): run `just fmt`, `just clippy`, `just test`, and `just check` before handoff.
- If any check cannot run, report the reason in the handoff.

## Rust Conventions

- Follow clippy guidance proactively to reduce fix-up churn:
  - Collapse nested `if` statements when equivalent.
  - Inline `format!` arguments when possible (`format!("{name}")` style).
  - Prefer method references over redundant closures when readable.
- Prefer exhaustive `match` statements over wildcard arms unless a wildcard is required for forward compatibility.
- Avoid introducing one-off helper functions that are only referenced once unless they materially improve readability.

## Architecture Rules

- Keep clear crate boundaries; avoid dependency leakage.
- `dictate-core` owns audio operations; presentation layers should stay thin.
- Implementation details (for example `cpal`) must not leak into consumer crates.
- Prefer explicit abstractions over shortcuts.

## Clipboard Behavior (Cross-Platform)

- macOS: use `pbcopy`.
- Wayland: require `wl-copy`; missing binary is a hard error.
- X11: try `xclip`, then `xsel`; if both are missing, hard error.
- Never lose transcription text. If clipboard write fails, still surface text (for example via stderr).

## Error Handling

- Use `thiserror` for custom error types.
- User-actionable failures should have clear, helpful messages.
- Retry transient failures with backoff.
- Fail fast on permanent errors.

## Git Workflow

- Conventional commits: `feat:`, `fix:`, `docs:`, `chore:`, `refactor:`, `test:`, `perf:`, `ci:`.
- Prefer one logical change per commit unless the user requests a different commit strategy.

## Workspace Structure (Illustrative)

Use this as a high-level map. Verify current files in the working tree before relying on exact paths.

```text
crates/
|- dictate-core/     # Audio + transcription + clipboard + post-processing
`- dictate-cli/      # CLI entrypoint and user-facing commands
```

## Code Style

- Public APIs require `///` doc comments.
- Module naming:
  - Agent nouns for modules exporting a primary struct (`recorder.rs`, `chunker.rs`, `resampler.rs`).
  - Descriptive nouns for utility/collection modules (`error.rs`, `devices.rs`).

## Testing & Validation

- Focus coverage on:
  - Chunker overlap, boundary behavior, `flush()` correctness.
  - Resampler rate conversion and format handling.
  - Error propagation and user-facing message quality.
  - Clipboard platform detection and fallback behavior.
  - Vocabulary operations and hint generation.
  - Dictionary corrections, persistence, and editor flows.
  - Post-processing transforms and pipeline integration.
- In tests, prefer asserting whole values/objects with `assert_eq!` when practical, rather than field-by-field assertions.

### Post-Processing Evaluation

- `just eval-prompt`: single prompt across all golden cases.
- `just eval-matrix`: multiple models/prompts across all golden cases.
- Golden cases: `crates/dictate-core/src/postprocess/prompts/golden_cases.json`.
- Prompt candidates: `crates/dictate-core/src/postprocess/prompts/candidates/`.
- Scoring: Levenshtein similarity + ROUGE-1 F1.
- Evaluation requires `GROQ_API_KEY`.

## Feature Work Process

- If behavior or CLI surface changes are user-visible, update `README.md`. Release notes are generated from Conventional Commits into the root `CHANGELOG.md` via `release-plz`, so keep commit messages release-note-ready instead of hand-editing changelog entries.
