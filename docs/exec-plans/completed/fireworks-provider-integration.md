# Add Fireworks via Shared OpenAI-Compatible Transports

## Purpose

Allow `dictate` and `dictate retry` to use Fireworks AI for speech-to-text and post-processing while preserving Groq as the default and keeping the fail-safe behavior that never drops transcription text.

User-visible outcome after implementation:

- `dictate` can choose Groq or Fireworks independently for transcription and post-processing.
- `groq` remains the default, so existing users do not need to change flags or env vars.
- `fireworks` works as a first-class named provider with built-in defaults for endpoints and models.
- `openai-compatible` remains available as a generic escape hatch for arbitrary compatible endpoints.
- `dictate retry` preserves the resolved provider, endpoint, and model choices from the saved recording unless the user overrides them.

## Progress

- [x] Add shared OpenAI-compatible transcription and chat transports.
- [x] Add named Fireworks and generic OpenAI-compatible providers for both pipeline stages.
- [x] Resolve provider, endpoint, API key, and model independently per stage in the CLI.
- [x] Persist provider and raw model metadata in retry manifests with Groq-compatible v1 fallback.
- [x] Update tests and user-facing docs.

## Decision Log

- Decision: Use one shared OpenAI-compatible transport layer for audio transcription and chat completions.
  Rationale: Groq and Fireworks both expose OpenAI-shaped HTTP APIs, so shared request building, cancellation, retry handling, and error parsing remove duplicated provider stacks.

- Decision: Keep `groq` and `fireworks` as explicit named providers on top of that shared transport.
  Rationale: Provider identity still matters for default endpoints, default models, env vars, and retry replay.

- Decision: Treat `openai-compatible` as the generic fallback mode, not as a first-class hosted provider UX.
  Rationale: Generic mode is useful for arbitrary compatible backends, but it should require explicit endpoint and model configuration.

- Decision: Resolve each network stage into an explicit target object before constructing the pipeline.
  Rationale: The pipeline now receives resolved transcription and post-process targets instead of scattered strings or one shared API key.

- Decision: Keep semantic Whisper presets and map them to provider-specific wire model ids.
  Rationale: Users can continue selecting stable intent such as `large-v3` while Groq and Fireworks use different underlying model names.

## Context

- Core provider abstractions and resolved targets live in `crates/dictate-core/src/provider/mod.rs` and `crates/dictate-core/src/postprocess/mod.rs`.
- Shared OpenAI-compatible transports live in `crates/dictate-core/src/provider/openai_compatible.rs` and `crates/dictate-core/src/postprocess/openai_compatible.rs`.
- Runtime resolution and pipeline construction live in `crates/dictate-cli/src/commands/record.rs`.
- Retry-manifest persistence lives in `crates/dictate-core/src/saved_recording/store.rs`.
- User-facing flags and examples live in `crates/dictate-cli/src/args.rs`, `crates/dictate-cli/src/main.rs`, and `README.md`.

## Plan of Work

Implemented shared OpenAI-compatible transports for transcription and chat completions, rebuilt Groq as a thin preset, added Fireworks and generic OpenAI-compatible providers, moved provider/model/endpoint resolution into the CLI per stage, updated the pipeline to consume resolved targets, bumped saved recording manifests to version 2 with backward-compatible Groq defaults for version 1, and documented the new provider/model flags and env vars in the README.

## Validation

Run from the repo root:

- `just fmt`
- `just clippy`
- `just test`
- `just check`

Observed result on 2026-03-24: all four commands completed successfully after the integration landed.
