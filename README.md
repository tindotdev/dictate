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
microphone → cpal → resample (16kHz mono) → chunking → Groq Whisper → clipboard
```

- **Audio capture** — cpal with real-time resampling
- **Ring buffer** — lock-free SPSC for zero-allocation transfer
- **Progressive chunking** — overlapping chunks for long recordings
- **Transcription** — Groq Whisper API (OpenAI-compatible)
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
