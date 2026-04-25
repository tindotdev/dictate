# Add Neovim Context Enrichment

## Purpose

Let `dictate.nvim` enrich post-processing with bounded, opt-in markdown context near the insertion point, while keeping editor context ephemeral and untrusted.

User-visible outcome after implementation:

- Context enrichment stays disabled by default.
- Enabled markdown buffers can pass nearby terms to post-processing.
- Context-assisted post-processing can preserve exact spelling and casing.
- Retry uses fresh current-buffer context.
- Editor context is not saved in recording metadata.

## Progress

- [x] Add ephemeral post-processing context support.
- [x] Add opt-in Neovim context enrichment settings.
- [x] Extract bounded markdown terms near insertion points.
- [x] Pass context for record and retry flows.
- [x] Keep context out of saved recordings.
- [x] Add promptfoo coverage for context behavior.
- [x] Update docs and validation coverage.

## Decision Log

- Decision: Add `--post-process-context` for supplemental context.
  Rationale: Context belongs to LLM post-processing, not ASR prompting.

- Decision: Keep context ephemeral.
  Rationale: Nearby editor text can be private or stale.

- Decision: Require explicit Neovim opt-in.
  Rationale: Enrichment sends buffer-derived text to an LLM provider.

- Decision: Extract conservative markdown terms.
  Rationale: Identifier-like terms reduce privacy and over-correction risk.

- Decision: Evaluate prompt behavior with promptfoo.
  Rationale: The main risk is model behavior under context.

## Requirements

### Opt-in Neovim Context Enrichment

The Neovim plugin SHALL keep context enrichment disabled by default and SHALL only collect or send buffer context when the user enables the feature.

- **WHEN** `dictate.nvim` is set up with default options in a markdown buffer
- **THEN** `DictateStart` does not add a post-processing context argument to the `dictate record` command

- **WHEN** context enrichment is enabled in a markdown buffer and relevant context is available near the cursor
- **THEN** `DictateStart` passes that context to `dictate record` using the post-processing context argument

- **WHEN** context enrichment is enabled and the plugin sends a non-empty context payload
- **THEN** the plugin ensures the `dictate record` or `dictate retry` command enables post-processing unless the user explicitly disabled post-processing for retry

### Markdown Context Extraction

The Neovim plugin SHALL extract bounded, relevant markdown context near the insertion point while preserving exact spelling and casing.

- **WHEN** the markdown buffer contains `This is SNAKE_CASE.` on the line before the insertion point
- **THEN** the context payload sent to `dictate` is `SNAKE_CASE`

- **WHEN** context enrichment is enabled in a buffer whose filetype is not configured for enrichment
- **THEN** the plugin does not send a post-processing context argument

- **WHEN** the configured markdown context window contains more extractable terms than the configured character limit allows
- **THEN** the plugin truncates the payload at a term boundary within the configured limit

### Retry Uses Current Editor Context

The Neovim plugin SHALL collect fresh context for `DictateRetry` from the current insertion target instead of relying on context from the original recording.

- **WHEN** context enrichment is enabled, `DictateRetry` is invoked in a markdown buffer, and relevant context is available near the cursor
- **THEN** the retry command includes the current context payload

- **WHEN** context enrichment is enabled but retry args explicitly include `--no-post-process`
- **THEN** the retry command does not include a post-processing context argument

## Context

- Core CLI support lives in `crates/dictate-cli`.
- Core post-processing behavior lives in `crates/dictate-core`.
- Neovim plugin behavior lives under `lua/dictate/`.
- Prompt evaluation tooling covers context-assisted correction, irrelevant context, and instruction-like context.
- User-facing documentation lives in `README.md`.

## Plan of Work

Implemented an ephemeral context path from Neovim through the CLI into post-processing, serialized context with tagged transcription content, kept retry context fresh, bounded markdown extraction, added tests and promptfoo cases, and documented opt-in behavior and privacy scope.

## Validation

Run from the repo root:

- `just fmt`
- `just clippy`
- `just test`
- `just check`
- `just fmt-nvim`
- `just lint-nvim`
- `just test-nvim`
- promptfoo context evaluation when provider credentials are available
