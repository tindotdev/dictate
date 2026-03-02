# dictate

Voice-to-text for Linux and macOS. Speak -> transcribe -> clipboard.

![dictate demo](assets/demo.gif)

## Installation

Homebrew:

```bash
brew tap tindotdev/tap
brew install tindotdev/tap/dictate-cli
```

From source:

```bash
git clone https://github.com/tindotdev/dictate.git && cd dictate
just install
```

## Usage

```bash
dictate                        # record -> clipboard
dictate --stdout               # record -> stdout (+ clipboard)
dictate --no-clipboard         # record -> stdout only
dictate --language en          # language hint for accuracy
dictate --device <query>       # select device by name or index
dictate --save-last-audio      # save audio locally for retry
dictate retry                  # rerun Whisper + post-process on saved audio
dictate devices                # list audio input devices
```

### Retry the last recording

If a long dictation did not come out the way you wanted, save the audio once and
rerun transcription without speaking again:

```bash
dictate --save-last-audio -p
dictate retry
dictate retry --transcription-model whisper-large-v3
dictate retry --prompt "Keep the wording literal" -p
dictate retry --no-post-process
```

Notes:

- `dictate retry` reuses the last audio saved with `--save-last-audio`
- Retry inherits the saved recording's transcription and post-process settings by default
- Any flags passed to `dictate retry` override the saved settings for that run
- Direct `dictate` recordings use shorter network timeouts and fewer retries so interactive/hotkey use stays bounded; `dictate retry` keeps longer, more persistent budgets
- Use `dictate retry --no-post-process` to compare raw Whisper output against a previously cleaned-up run
- The saved audio stays available until it is replaced by a later `--save-last-audio` recording

### Output formats

```bash
dictate --format verbose_json  # structured JSON
dictate --timestamps word      # word-level timestamps (requires verbose_json)
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

Detailed methodology and latest results:

- `crates/dictate-core/src/postprocess/prompts/README.md`
- `crates/dictate-core/src/postprocess/prompts/RESULTS-latest.md`

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
dictate remember   # add correction (interactive)
dictate dictionary # list entries
```

Both are injected into Whisper's prompt parameter and stored at `~/.config/dictate/`.

## Configuration

Required:

```bash
export GROQ_API_KEY="your-api-key"  # console.groq.com/keys
```

Optional:

```bash
export GROQ_BASE_URL="..."       # override transcription endpoint
export GROQ_CHAT_BASE_URL="..."  # override post-process chat endpoint
```

Add to your shell profile for persistence. From source installs, `just add-secret` can help.

## Shell completions

Generate and install completions for your shell (fish, bash, zsh ):

```bash
dictate completions fish > ~/.config/fish/completions/dictate.fish
dictate completions bash > ~/.local/share/bash-completion/completions/dictate
dictate completions zsh > ~/.zfunc/_dictate  # then add ~/.zfunc to fpath
```

## Platform requirements

Linux:

- Audio: PipeWire or PulseAudio
- Clipboard: `wl-clipboard` (Wayland) or `xclip`/`xsel` (X11)
- Launcher notifications: `libnotify` (`notify-send`) and `glib2` (`gdbus`)

macOS:

- Grant microphone access to your terminal app
- Clipboard uses built-in `pbcopy` (no extra clipboard package required)

## Global shortcut

Install the canonical launcher files from this repo, then bind them to keyboard shortcuts:

```bash
just install-launchers
```

The desktop launcher is a toggle — press once to start recording, press again to stop. It runs
headlessly (no terminal window) and uses desktop notifications for feedback:

- **Recording** — persistent notification stays visible while recording
- **Transcribing** — replaces the recording notification on stop
- **Cancelling** — pressing the shortcut again during transcription requests cancellation
- **Done** — shows result for 3 seconds, then auto-dismisses

Auto-stops after 5 minutes by default. Override with `DICTATE_TIMEOUT=120` (seconds).
If transcription still hangs after stop, the launcher escalates after
`DICTATE_TRANSCRIBE_TIMEOUT=45` seconds by default.

Linux compositor examples:

- Sway: `bindsym $mod+d exec dictate-launch -p`
- Hyprland: `bind = SUPER, D, exec, dictate-launch -p`
- COSMIC: `super + semicolon -> dictate-launch -p`

All flags after `dictate-launch` are passed through to `dictate` (e.g. `-p` for post-processing,
`--language en`, `--device "USB Mic"`).

Kitty adapter:

- Install with `just install-launcher-kitty` or `just install-launchers`
- Bind in Kitty with:

```text
map kitty_mod+d launch --type=background dictate-kitty
map kitty_mod+shift+d launch --type=background dictate-kitty retry
```

The canonical launcher sources are:

- [contrib/launchers/dictate-launch](/mnt/68ce8b89-5b49-4f3f-857c-8c9edca5b28e/code/github/tindotdev/dictate/contrib/launchers/dictate-launch)
- [contrib/launchers/dictate-kitty](/mnt/68ce8b89-5b49-4f3f-857c-8c9edca5b28e/code/github/tindotdev/dictate/contrib/launchers/dictate-kitty)
- [contrib/launchers/dictate-launch-common.sh](/mnt/68ce8b89-5b49-4f3f-857c-8c9edca5b28e/code/github/tindotdev/dictate/contrib/launchers/dictate-launch-common.sh)

Repo-side launcher validation:

- `just test-launchers` runs start/stop/retry smoke tests against fake `dictate`, notification, and Kitty binaries
- `just lint-launchers` runs `shellcheck`

Compatibility wrappers remain at:

- [contrib/dictate-launch](/mnt/68ce8b89-5b49-4f3f-857c-8c9edca5b28e/code/github/tindotdev/dictate/contrib/dictate-launch)
- [contrib/dictate-kitty](/mnt/68ce8b89-5b49-4f3f-857c-8c9edca5b28e/code/github/tindotdev/dictate/contrib/dictate-kitty)

Debugging helpers:

- `DICTATE_STATE_DIR=/tmp/dictate-test` overrides where pid/state/output files are written
- `DICTATE_LAUNCH_LOG=/tmp/dictate-launch.log` appends launcher events and child exec lines
- `DICTATE_LAUNCH_TRACE=1` enables Bash xtrace; if `DICTATE_LAUNCH_LOG` is set, trace output goes there
- `DICTATE_BIN=/path/to/dictate` and `DICTATE_TRANSCRIPTION_MODEL=...` let you test alternate binaries/settings

On macOS outside Kitty, create a system shortcut (Shortcuts or Automator) that runs `dictate` in your terminal.

## Architecture

```text
microphone -> cpal -> resample (16kHz mono) -> chunking -> Groq Whisper -> optional LLM cleanup -> clipboard/stdout
```

- Audio capture: cpal with real-time resampling
- Ring buffer: lock-free SPSC for zero-allocation transfer
- Progressive chunking: overlapping chunks for long recordings
- Transcription: Groq Whisper API (OpenAI-compatible)
- Post-processing: optional Groq chat cleanup with fail-safe fallback
- Clipboard: platform-aware with fallback to stderr

## Troubleshooting

Audio:

- Linux: check `systemctl --user status pipewire`, then run `dictate devices`
- Linux: if needed, add your user to the `audio` group: `sudo usermod -aG audio $USER` (re-login required)
- macOS: verify microphone permission in System Settings -> Privacy & Security -> Microphone

Clipboard:

- Linux (Wayland): `echo "test" | wl-copy && wl-paste`
- Linux (X11): verify `xclip` or `xsel` is installed
- macOS: `echo "test" | pbcopy && pbpaste`

API errors:

- `401`: invalid API key
- `429`: rate limited (retries automatically)
- `413`: recording too long
- `dictate retry`: first create a reusable recording with `dictate --save-last-audio`

## Privacy

Audio is sent to [Groq](https://groq.com) for transcription. By default, audio is
not stored locally. If you pass `--save-last-audio`, dictate stores one reusable
local recording until it is replaced by a later saved recording. See Groq's
[privacy policy](https://groq.com/privacy-policy/) and
[terms of use](https://groq.com/terms-of-use/).

## Acknowledgments

Audio pipeline design inspired by [whis](https://github.com/frankdierolf/whis).

## License

MIT
