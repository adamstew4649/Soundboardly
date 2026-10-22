//! Clip model and the sample-level playback voice.
//!
//! Edits (trim, speed, volume) are non-destructive: the decoded audio is kept
//! as-is and the edit is applied while a `Voice` renders it.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const MIN_SPEED: f32 = 0.25;
pub const MAX_SPEED: f32 = 4.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClipMeta {
    pub id: u64,
    pub name: String,
    /// Mono WAV file name inside the library's clip directory.
    pub file: String,
    /// Trim start/end in seconds of the source audio. `end == None` means "to the end".
    pub start: f32,
    pub end: Option<f32>,
    /// Playback speed multiplier. Pitch follows speed, as with a tape.
    pub speed: f32,
    pub volume: f32,
    /// Global hotkey in `global-hotkey` string form, e.g. `ctrl+alt+KeyA`.
    pub hotkey: Option<String>,
}

/// Decoded mono audio shared between voices.
#[derive(Debug)]
pub struct Audio {
    pub samples: Vec<f32>,
    pub rate: u32,
}

impl Audio {
    pub fn duration(&self) -> f32 {
        self.samples.len() as f32 / self.rate as f32
    }
}

/// Reads a WAV file of any channel count and downmixes it to mono f32.
pub fn load_wav(path: &Path) -> Result<Audio> {
    let mut reader = hound::WavReader::open(path).with_context(|| format!("open {}", path.display()))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
        }
    };
    let samples = interleaved
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    Ok(Audio { samples, rate: spec.sample_rate })
}

/// Plays one clip, resampled to the output rate with linear interpolation.
pub struct Voice {
    audio: Arc<Audio>,
    pos: f64,
    end: f64,
    step: f64,
    volume: f32,
}

impl Voice {
    pub fn new(audio: Arc<Audio>, meta: &ClipMeta, out_rate: u32) -> Self {
        let rate = audio.rate as f64;
        let len = audio.samples.len() as f64;
        let pos = (meta.start.max(0.0) as f64 * rate).min(len);
        let end = meta.end.map_or(len, |e| (e as f64 * rate).clamp(pos, len));
        let speed = meta.speed.clamp(MIN_SPEED, MAX_SPEED) as f64;
        Self { audio, pos, end, step: speed * rate / out_rate as f64, volume: meta.volume }
    }

    pub fn finished(&self) -> bool {
        self.pos >= self.end
    }

    /// Adds this voice's next samples into `out` (one mono sample per slot).
    pub fn mix_into(&mut self, out: &mut [f32]) {
        let s = &self.audio.samples;
        for slot in out {
            if self.finished() {
                break;
            }
            let i = self.pos as usize;
            let frac = (self.pos - i as f64) as f32;
            let a = s[i];
            let b = if (i + 1) < self.end as usize { s[i + 1] } else { a };
            *slot += (a + (b - a) * frac) * self.volume;
            self.pos += self.step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(start: f32, end: Option<f32>, speed: f32) -> ClipMeta {
        ClipMeta { id: 1, name: "t".into(), file: "t.wav".into(), start, end, speed, volume: 1.0, hotkey: None }
    }

    fn render(audio: Arc<Audio>, m: &ClipMeta, out_rate: u32) -> Vec<f32> {
        let mut v = Voice::new(audio, m, out_rate);
        let mut out = Vec::new();
        while !v.finished() {
            let mut buf = [0.0; 64];
            v.mix_into(&mut buf);
            out.extend_from_slice(&buf);
        }
        out
    }

    fn ramp(n: usize) -> Arc<Audio> {
        Arc::new(Audio { samples: (0..n).map(|i| i as f32).collect(), rate: 100 })
    }

    fn played_len(out: &[f32]) -> usize {
        // Trailing zeros are block padding from the last 64-sample buffer.
        out.iter().rposition(|&x| x != 0.0).map_or(0, |i| i + 1)
    }

    #[test]
    fn double_speed_halves_length() {
        let out = render(ramp(1000), &meta(0.0, None, 2.0), 100);
        assert!((499..=501).contains(&played_len(&out)), "{}", played_len(&out));
    }

    #[test]
    fn trim_selects_range() {
        let out = render(ramp(1000), &meta(2.0, Some(4.0), 1.0), 100);
        assert_eq!(out[0], 200.0);
        assert!((199..=201).contains(&played_len(&out)));
        assert!(out[..199].iter().all(|&x| x >= 200.0 && x < 400.0));
    }

    #[test]
    fn resamples_to_output_rate() {
        let out = render(ramp(1000), &meta(0.0, None, 1.0), 200);
        assert!((1998..=2001).contains(&played_len(&out)));
    }

    #[test]
    fn bad_trim_is_clamped() {
        let v = Voice::new(ramp(100), &meta(50.0, Some(1.0), 1.0), 100);
        assert!(v.finished());
    }
}
