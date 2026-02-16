# dictate

Voice-to-text for Linux. Speak → transcribe → clipboard.

![dictate demo](assets/demo.gif)

## Installation

**Homebrew:**

```bash
brew tap tindotdev/tap
brew install tindotdev/tap/dictate-cli
```

**From source:**

```bash
git clone https://github.com/tindotdev/dictate.git && cd dictate
just install
```

## Usage

```bash
dictate                        # record → clipboard
dictate --stdout               # record → stdout (+ clipboard)
dictate --no-clipboard         # record → stdout only
dictate --language en          # language hint for accuracy
dictate --device <query>       # select device by name or index
dictate devices                # list audio input devices
```

### Output formats

```bash
dictate --format verbose_json        # structured JSON
dictate --timestamps word            # word-level timestamps (requires verbose_json)
```

When `--post-process` is enabled with `--format json` or `--format verbose_json`, output includes:

- `post_processed` (boolean)
- `post_process_status` (`applied`, `failed_fallback`, `skipped_verbose_json`, `skipped_empty_text`, `not_configured`)

### Post-processing (LLM cleanup)

Optional post-processing cleans raw Whisper output (filler words, punctuation, capitalization).

```bash
dictate -p
dictate -p --post-process-model openai/gpt-oss-120b
```

Notes:

- Default post-processing model: `openai/gpt-oss-20b`
- Fail-safe behavior: if post-processing fails, raw transcription text is still returned
- `--format verbose_json` skips post-processing to avoid mismatches between top-level `text` and timestamped `segments`/`words`
- `--post-process-base-url` is available for OpenAI-compatible chat endpoints, but this branch has only been validated against Groq API endpoints

Quality is tracked with golden-case evaluations (`just eval-prompt`, `just eval-matrix`):

- 14 golden scenarios (filler removal, technical terms, punctuation, mixed and edge cases)
- Best tested matrix configuration: `openai/gpt-oss-20b` + `cleanup_v2.txt` = `14/14 (100%)`
- Current built-in runtime configuration: `openai/gpt-oss-20b` + `cleanup.txt` = `13/14 (93%)`

Detailed methodology and results:

- `crates/dictate-core/src/postprocess/prompts/README.md`
- `crates/dictate-core/src/postprocess/prompts/RESULTS-2026-02-17.md`

### Vocabulary

Custom terms improve transcription accuracy for technical jargon, names, and abbreviations.

```bash
dictate vocab add AWS OpenAI
dictate vocab remove AWS
dictate vocab list
```

### Dictionary

Corrections for commonly misheard words. Interactive editor.

```bash
dictate remember                     # add correction (interactive)
dictate dictionary                   # list entries
```

Both are injected into Whisper's prompt parameter. Stored at `~/.config/dictate/`.

## Configuration

```bash
export GROQ_API_KEY="your-api-key"  # console.groq.com/keys
export GROQ_BASE_URL="..."          # optional: override endpoint
export GROQ_CHAT_BASE_URL="..."     # optional: override post-process chat endpoint
```

Add to shell profile for persistence. From source: `just add-secret`.

## Requirements

- Linux audio (PipeWire or PulseAudio)
- Clipboard: `wl-clipboard` (Wayland) or `xclip`/`xsel` (X11)

## Global shortcut

Bind `dictate` to a key in your compositor for desktop-wide activation.

**Sway:** `bindsym $mod+d exec foot -T "dictate" -- dictate`

**Hyprland:** `bind = SUPER, D, exec, foot -T "dictate" -- dictate`

**COSMIC:** `super + semicolon → foot -T "dictate" -- dictate`

Replace `foot` with your terminal of choice.

## Architecture

```
microphone → cpal → resample (16kHz mono) → chunking → Groq Whisper → optional LLM cleanup → clipboard/stdout
```

- **Audio capture** — cpal with real-time resampling
- **Ring buffer** — lock-free SPSC for zero-allocation transfer
- **Progressive chunking** — overlapping chunks for long recordings
- **Transcription** — Groq Whisper API (OpenAI-compatible)
- **Post-processing** — optional Groq chat cleanup with fail-safe fallback
- **Clipboard** — platform-aware with fallback to stderr

## Troubleshooting

**Audio:** Check PipeWire status with `systemctl --user status pipewire`. List devices with `dictate devices`. Fix permissions with `sudo usermod -aG audio $USER` (requires re-login).

**Clipboard:** Install `wl-clipboard` (Wayland) or `xclip` (X11). Verify with `echo "test" | wl-copy && wl-paste`.

**API errors:** 401 = invalid key. 429 = rate limited (retries automatically). 413 = recording too long.

## Privacy

Audio is sent to [Groq](https://groq.com) for transcription. No audio is stored locally. See Groq's [privacy policy](https://groq.com/privacy-policy/) and [terms of use](https://groq.com/terms-of-use/).

## Acknowledgments

Audio pipeline design inspired by [whis](https://github.com/frankdierolf/whis).

## License

MIT
