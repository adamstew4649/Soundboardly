//! Fetches audio from a URL (YouTube, TikTok, X, ...) via yt-dlp, converted to WAV by ffmpeg.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

pub struct Downloaded {
    pub title: String,
    pub wav: PathBuf,
}

/// Blocking; run on a worker thread. `out_stem` is the path without extension.
pub fn fetch(url: &str, out_stem: &Path) -> Result<Downloaded> {
    // Reject option-like input so a pasted string can't inject yt-dlp flags.
    if url.starts_with('-') || !(url.starts_with("http://") || url.starts_with("https://")) {
        bail!("not a http(s) URL");
    }
    let wav = out_stem.with_extension("wav");
    let mut cmd = Command::new(tool("yt-dlp"));
    let ffmpeg = tool("ffmpeg");
    if ffmpeg.is_absolute() {
        cmd.arg("--ffmpeg-location").arg(&ffmpeg);
    }
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
    let output = cmd
        .args(["--no-playlist", "-x", "--audio-format", "wav", "--print", "title", "--no-simulate", "-o"])
        .arg(format!("{}.%(ext)s", out_stem.display()))
        .arg("--")
        .arg(url)
        .output()
        .context("could not run yt-dlp (is it installed next to the app or on PATH?)")?;
    if !output.status.success() {
        bail!("yt-dlp failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    if !wav.exists() {
        bail!("yt-dlp finished but produced no WAV (is ffmpeg available?)");
    }
    let title = String::from_utf8_lossy(&output.stdout).lines().next().unwrap_or("clip").trim().to_string();
    Ok(Downloaded { title, wav })
}

/// Prefer a binary shipped beside the executable, else fall back to PATH.
fn tool(name: &str) -> PathBuf {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(&exe)))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(exe))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_urls() {
        let p = Path::new("/tmp/x");
        assert!(fetch("--exec=evil", p).is_err());
        assert!(fetch("file:///etc/passwd", p).is_err());
    }
}
