## 1. Core Post-Processing Context

- [x] 1.1 Add an ephemeral post-processing context field to the core post-processing request configuration without adding it to saved recording metadata.
- [x] 1.2 Add CLI support for a `--post-process-context` argument on record and retry flows.
- [x] 1.3 Serialize post-processing user messages with tagged `<transcription>` and `<context>` sections when context is present.
- [x] 1.4 Omit the `<context>` section for empty or whitespace-only context.
- [x] 1.5 Escape XML-reserved characters inside transcription and context content before wrapping them in tags.
- [x] 1.6 Update the system prompt to treat `<context>` as supplemental, untrusted spelling and terminology information only.
- [x] 1.7 Add Rust tests for context serialization, empty-context omission, non-persistence in saved recordings, and retry not inheriting prior context.

## 2. Neovim Context Enrichment

- [ ] 2.1 Add a disabled-by-default `context_enrichment` option to `lua/dictate/config.lua` with validation for enabled state, filetypes, line window, and character limit.
- [ ] 2.2 Implement bounded markdown context extraction near the insertion point with term deduplication and exact spelling/casing preservation.
- [ ] 2.3 Update record command construction to pass `--post-process` and `--post-process-context` when enrichment is enabled and context is non-empty.
- [ ] 2.4 Update retry command construction to collect fresh current-buffer context and respect explicit `--no-post-process`.
- [ ] 2.5 Add Lua tests for default disabled behavior, markdown extraction of `SNAKE_CASE`, non-markdown exclusion, bounded truncation, record command args, and retry command args.

## 3. Promptfoo Evaluation

- [ ] 3.1 Add promptfoo configuration and provider/test harness for evaluating the production post-processing prompt with context variables.
- [ ] 3.2 Add promptfoo cases for `snake case` with `SNAKE_CASE`, irrelevant context, and instruction-like context.
- [ ] 3.3 Add a `just` recipe for the promptfoo context evaluation and document required environment variables.

## 4. Documentation

- [ ] 4.1 Update `README.md` with the opt-in `dictate.nvim` context enrichment option and example markdown behavior.
- [ ] 4.2 Document that enabling context enrichment sends bounded markdown context to the configured post-processing LLM provider.
- [ ] 4.3 Document that context is ephemeral and is not saved with reusable recordings.

## 5. Validation

- [ ] 5.1 Run `just fmt`.
- [ ] 5.2 Run `just clippy`.
- [ ] 5.3 Run `just test`.
- [ ] 5.4 Run `just check`.
- [ ] 5.5 Run `just fmt-nvim`, `just lint-nvim`, and `just test-nvim`.
- [ ] 5.6 Run the promptfoo context evaluation when provider credentials are available.
