# voxkey

Latch dictation for Linux/Wayland (KDE): hold **AltGr** ~1s to start, speak hands-free,
then **tap** AltGr to stop — [Whisper](https://github.com/openai/whisper)
(via [whisper.cpp](https://github.com/ggerganov/whisper.cpp), CPU) transcribes and pastes
into the focused app.

## Build

```bash
sudo dnf install cmake clang   # build whisper.cpp (via whisper-rs)
cargo build --release
mkdir -p ~/.local/bin && cp target/release/voxkey ~/.local/bin/
```

## One-time setup

System packages and permissions:

```bash
sudo dnf install alsa-lib-devel wl-clipboard   # Fedora (ALSA headers for cpal + clipboard)
sudo usermod -aG input "$USER"                 # then log out/in
# udev rule for /dev/uinput (if not already present):
#   echo 'KERNEL=="uinput", MODE="0660", GROUP="input", OPTIONS+="static_node=uinput"' \
#     | sudo tee /etc/udev/rules.d/99-uinput.rules
#   sudo udevadm control --reload-rules && sudo udevadm trigger
```

> `alsa-lib-devel` is a **build** dependency (cpal's ALSA backend). The `input`
> group + `/dev/uinput` rule + `wl-clipboard` are runtime requirements.

Download a model (once, ~181 MB for `small`):

```bash
scripts/fetch-model.sh small
```

Config (optional):

```bash
mkdir -p ~/.config/voxkey && cp config.example.toml ~/.config/voxkey/config.toml
```

## Run

```bash
voxkey            # or: systemctl --user enable --now voxkey  (after copying the unit)
```

Hold AltGr for ~1s to start, speak, then tap AltGr to stop. The tray icon shows idle /
recording / transcribing, blinks in the last `warn_seconds` before the `max_seconds` cap,
and shows the elapsed time on hover. At the cap, recording auto-stops with a desktop
notification. The menu offers Pause and Quit.

## How it works

`evdev` reads AltGr press/release from `/dev/input/event*`; `cpal` records the mic;
the audio is resampled to 16 kHz and transcribed by **whisper.cpp** (via the
`whisper-rs` crate); the text is pasted with `Shift+Insert` (clipboard saved and
restored). The transcription engine is behind a `Transcriber` trait.

## Notes

- whisper.cpp runs on **CPU** with quantized models for speed. For more accuracy
  at some cost, point `model_path` at a larger or less-quantized `ggml-*.bin`.
