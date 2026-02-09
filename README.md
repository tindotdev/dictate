# dictate

Voice-to-text for Linux. Speak → transcribe → clipboard.

![dictate demo](assets/demo.gif)


## Requirements

- Rust toolchain
- [Groq API key](https://console.groq.com/keys)
- Linux audio (PipeWire or PulseAudio)
- Clipboard: `wl-clipboard` (Wayland) or `xclip`/`xsel` (X11)


## Installation

```bash
git clone https://github.com/tindotdev/dictate.git && cd dictate
just install       # installs to ~/.cargo/bin/dictate
just add-secret    # configure GROQ_API_KEY
```


## Usage

```bash
dictate                              # record → clipboard
dictate --stdout                     # record → stdout (+ clipboard)
dictate --no-clipboard               # record → stdout only
dictate devices                      # list audio input devices
dictate --device <query>             # select specific device by name/index
dictate --language en                # language hint for better accuracy
dictate --format verbose_json        # structured JSON output
dictate --timestamps word            # word-level timestamps (requires verbose_json)
```


## Configuration

- `GROQ_API_KEY` (required) — Groq API key for Whisper transcription
- `GROQ_BASE_URL` (optional) — override API endpoint URL


## Global Activation

For desktop-wide activation, bind a launcher script to a global shortcut.

Configure your API key:

```bash
just add-secret
```

Install the launcher:

```bash
just install-launcher
```

Bind a global shortcut in your compositor:

COSMIC:
```
super + semicolon
  /home/you/.local/bin/dictate-launch
```

Sway:
```
bindsym $mod+d exec ~/.local/bin/dictate-launch
```

Hyprland:
```
bind = SUPER, D, exec, ~/.local/bin/dictate-launch
```


## Troubleshooting

Audio issues:
```bash
systemctl --user status pipewire  # check PipeWire
dictate devices                    # list devices
dictate --device "device name"     # test specific device
sudo usermod -aG audio $USER       # fix permissions (then log out/in)
```

Clipboard issues:
```bash
# Wayland
sudo dnf install wl-clipboard
echo "test" | wl-copy && wl-paste

# X11
sudo dnf install xclip
echo "test" | xclip -selection clipboard && xclip -o -selection clipboard
```

API errors:
- 401 — invalid/expired API key
- 429 — rate limited (retries automatically)
- 413 — recording too long (shouldn't happen with chunking)


## Architecture

```
microphone → cpal → resample (16kHz mono) → chunking → Groq Whisper → clipboard
```

- Audio capture — cpal with real-time resampling
- Ring buffer — lock-free SPSC for zero-allocation transfer
- Progressive chunking — overlapping chunks for long recordings
- Transcription — Groq Whisper API (OpenAI-compatible)
- Clipboard — platform-aware with error handling


## Privacy

Audio is sent to the [Groq API](https://groq.com) for transcription. Review [Groq's privacy policy](https://groq.com/privacy-policy/) and [terms of use](https://groq.com/terms-of-use/). No audio is stored locally.


## License

MIT
