# Soundboardly

A fast soundboard for Windows 11, written in Rust. Like Soundpad, plus:

- **Grab clips from links** – paste a YouTube, TikTok or X (Twitter) URL (anything yt-dlp supports); the audio is downloaded and converted to WAV.
- **Edit clips** – trim to the part you need, change speed (0.25×–4×, pitch follows speed) and volume. Edits are non-destructive.
- **Global hotkeys** – play any clip from any app.
- **Play into your microphone** – clips and your voice are mixed and sent to a virtual cable, so Discord, games, etc. hear both.
- **Mute-me-while-clips-play toggle** – off by default.

## Install

1. Download `Soundboardly-Setup-x.y.z.exe` from the repository's **Releases** page (or from the latest CI run's artifacts) and run it. It bundles `yt-dlp` and `ffmpeg`, so nothing else is needed for downloading clips.
2. Install **[VB-Cable](https://vb-audio.com/Cable/)** (free) if you want clips to come out of your microphone. It is a separate driver install and needs a reboot. Soundboardly doesn't bundle it because VB-Audio's licence doesn't allow redistribution.

## Set up the microphone output

In Soundboardly's right-hand panel:

| Setting | Choose |
|---|---|
| Microphone | Your real mic |
| Virtual cable | `CABLE Input (VB-Audio Virtual Cable)` |
| Monitor | The headphones/speakers you listen on |

Click **Apply devices**. Then, in Discord / your game / OBS, set the **input (microphone)** device to `CABLE Output (VB-Audio Virtual Cable)`.

Tips: turn off noise suppression and echo cancellation in the receiving app if clips sound filtered. Without a virtual cable selected you'll only hear clips on your Monitor device.

## Using it

1. Paste a link into the URL box and press **Download clip**.
2. Click a clip to select it, then adjust start/end, speed and volume on the right. **Preview** plays it.
3. Click **set hotkey** next to a clip and press the combination (Esc clears it).
4. **Stop all** cuts off everything currently playing.

Clips and settings are stored in `%APPDATA%\Soundboardly`.

## Building from source

Requires Rust (stable). On Windows:

```
cargo run --release
```

For downloads, put `yt-dlp.exe` and `ffmpeg.exe` next to the built exe, or on your `PATH`.

The installer is built by GitHub Actions (`.github/workflows/build.yml`) with [Inno Setup](https://jrsoftware.org/isinfo.php). Push a tag like `v0.1.0` to publish it as a release; every other push uploads it as a build artifact. Locally: `cargo test` runs the unit tests on any OS.

## Known limitations

- Audio devices must support 32-bit float samples (the Windows shared-mode default).
- Trimming uses sliders, not a waveform view.
- Only WAV clips from downloads for now; no local-file import yet.
- Respect the terms of the sites you download from and the rights of clip owners.
