# Roadmap

dictate is usable today on Linux + macOS (desktop CLI + Neovim plugin). This roadmap focuses on what’s next, not a historical checklist.

## Before Making The Repo Public

- Do a fresh install smoke test:
  - Linux: `bunx -p @tindotdev/dictate dictate`
  - macOS: `brew install ffmpeg` then `bunx -p @tindotdev/dictate dictate`
- Have a newcomer follow `README.md` end-to-end and note any ambiguity.
- Sanity-check history for secrets (see `git filter-repo --analyze`).

## Near Term (Maintenance / Quality)

- Document macOS input device selection for `ffmpeg` (and how to list devices).
- Improve error UX for common failures (missing `ffmpeg`, missing `pw-cat`, auth failure).
- Keep compatibility with Neovim stable releases (0.10+).

## Longer Term (Nice-to-have)

- Windows support (audio capture + clipboard; likely requires a different backend).
- Better configurability:
  - configurable model / prompt in a clearer way
  - per-project/per-buffer enable/disable ergonomics

## Docs

- User docs live in `README.md`.
- Dev/troubleshooting notes live in `docs/runbook.md`.
