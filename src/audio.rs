//! Real-time audio: mic + clips -> virtual cable, clips -> monitor (your speakers).
//!
//! The cable output is what other apps record as their microphone. Each output
//! stream owns its own set of voices; `play` starts a clip on both.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream, StreamConfig};
use crossbeam_queue::ArrayQueue;

use crate::clip::{Audio, ClipMeta, Voice};

pub struct DeviceNames {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

pub fn list_devices() -> DeviceNames {
    let host = cpal::default_host();
    let names = |devs: Option<Vec<Device>>| {
        devs.unwrap_or_default().iter().filter_map(|d| d.description().ok().map(|d| d.name().to_string())).collect()
    };
    DeviceNames {
        inputs: names(host.input_devices().ok().map(|i| i.collect())),
        outputs: names(host.output_devices().ok().map(|i| i.collect())),
    }
}

fn find(devs: impl Iterator<Item = Device>, name: &str) -> Option<Device> {
    devs.into_iter().find(|d| d.description().is_ok_and(|d| d.name() == name))
}

/// Shared with the UI thread; read lock-free by the audio callbacks.
pub struct Controls {
    pub mute_mic_during_clips: AtomicBool,
    /// Mic gain as f32 bits.
    mic_gain: AtomicU32,
}

impl Controls {
    pub fn set_mic_gain(&self, g: f32) {
        self.mic_gain.store(g.to_bits(), Ordering::Relaxed);
    }
    fn mic_gain(&self) -> f32 {
        f32::from_bits(self.mic_gain.load(Ordering::Relaxed))
    }
}

pub struct Engine {
    cable: Option<Sink>,
    monitor: Option<Sink>,
    _streams: Vec<Stream>,
}

struct Sink {
    new_voices: Arc<ArrayQueue<Voice>>,
    stop: Arc<AtomicBool>,
    rate: u32,
}

pub struct EngineConfig<'a> {
    pub mic: Option<&'a str>,
    pub cable: Option<&'a str>,
    pub monitor: Option<&'a str>,
}

impl Engine {
    /// Opens whatever devices are configured. Returns the engine plus a warning
    /// per device that could not be opened, so one bad device doesn't block the rest.
    pub fn start(cfg: EngineConfig, controls: Arc<Controls>) -> (Engine, Vec<String>) {
        let host = cpal::default_host();
        let mut streams = Vec::new();
        let mut warnings = Vec::new();

        let monitor_dev = match cfg.monitor {
            Some(n) => host.output_devices().ok().and_then(|d| find(d, n)),
            None => host.default_output_device(),
        };
        let cable_dev = cfg.cable.and_then(|n| host.output_devices().ok().and_then(|d| find(d, n)));
        if cfg.cable.is_some() && cable_dev.is_none() {
            warnings.push("Virtual cable device not found - is VB-Cable installed?".into());
        }
        let mic_dev = match cfg.mic {
            Some(n) => host.input_devices().ok().and_then(|d| find(d, n)),
            None => host.default_input_device(),
        };

        let mut cable = None;
        if let Some(dev) = cable_dev {
            // Mic samples hop from the input callback to the cable callback via a queue.
            let mic_queue = Arc::new(ArrayQueue::<f32>::new(48_000));
            match build_cable(&dev, mic_queue.clone(), controls.clone()) {
                Ok((stream, sink)) => {
                    streams.push(stream);
                    cable = Some(sink);
                    match mic_dev.as_ref().map(|d| build_mic(d, mic_queue, sink_rate(&cable))) {
                        Some(Ok(s)) => streams.push(s),
                        Some(Err(e)) => warnings.push(format!("Microphone: {e:#}")),
                        None => warnings.push("No microphone found".into()),
                    }
                }
                Err(e) => warnings.push(format!("Virtual cable: {e:#}")),
            }
        }

        let mut monitor = None;
        if let Some(dev) = monitor_dev {
            match build_monitor(&dev) {
                Ok((stream, sink)) => {
                    streams.push(stream);
                    monitor = Some(sink);
                }
                Err(e) => warnings.push(format!("Monitor output: {e:#}")),
            }
        }

        (Engine { cable, monitor, _streams: streams }, warnings)
    }

    /// Starts a clip on every open output.
    pub fn play(&self, audio: &Arc<Audio>, meta: &ClipMeta) {
        for sink in self.cable.iter().chain(&self.monitor) {
            // A full queue means the user is mashing keys faster than we render; drop the extra.
            let _ = sink.new_voices.push(Voice::new(audio.clone(), meta, sink.rate));
        }
    }

    pub fn stop_all(&self) {
        for sink in self.cable.iter().chain(&self.monitor) {
            sink.stop.store(true, Ordering::Relaxed);
        }
    }
}

pub fn new_controls(mute: bool, mic_gain: f32) -> Arc<Controls> {
    Arc::new(Controls { mute_mic_during_clips: AtomicBool::new(mute), mic_gain: AtomicU32::new(mic_gain.to_bits()) })
}

fn sink_rate(sink: &Option<Sink>) -> u32 {
    sink.as_ref().map_or(48_000, |s| s.rate)
}

fn out_config(dev: &Device) -> Result<StreamConfig> {
    let c = dev.default_output_config().context("no output config")?;
    if c.sample_format() != cpal::SampleFormat::F32 {
        return Err(anyhow!("device does not offer f32 samples ({})", c.sample_format()));
    }
    Ok(c.into())
}

/// Drains `new_voices` and sums all live voices into `mono`.
fn render_voices(voices: &mut Vec<Voice>, new_voices: &ArrayQueue<Voice>, stop: &AtomicBool, mono: &mut [f32]) {
    if stop.swap(false, Ordering::Relaxed) {
        voices.clear();
        while new_voices.pop().is_some() {}
    }
    while let Some(v) = new_voices.pop() {
        voices.push(v);
    }
    for v in voices.iter_mut() {
        v.mix_into(mono);
    }
    voices.retain(|v| !v.finished());
}

fn spread(mono: &[f32], out: &mut [f32], channels: usize) {
    for (frame, &s) in out.chunks_mut(channels).zip(mono) {
        frame.fill(s.clamp(-1.0, 1.0));
    }
}

fn err_fn(e: cpal::Error) {
    eprintln!("audio stream error: {e}");
}

fn build_cable(dev: &Device, mic: Arc<ArrayQueue<f32>>, controls: Arc<Controls>) -> Result<(Stream, Sink)> {
    let config = out_config(dev)?;
    let channels = config.channels as usize;
    let new_voices = Arc::new(ArrayQueue::new(32));
    let stop = Arc::new(AtomicBool::new(false));
    let (nv, st) = (new_voices.clone(), stop.clone());
    let mut voices: Vec<Voice> = Vec::new();
    let mut mono = vec![0.0f32; 8192];

    let stream = dev.build_output_stream(
        config,
        move |out: &mut [f32], _| {
            let frames = out.len() / channels;
            if mono.len() < frames {
                mono.resize(frames, 0.0);
            }
            let mono = &mut mono[..frames];
            mono.fill(0.0);
            let clips_playing = !voices.is_empty() || !nv.is_empty();
            render_voices(&mut voices, &nv, &st, mono);

            // Skip stale mic audio if the mic clock runs ahead of the cable's, to bound latency.
            while mic.len() > frames + 2400 {
                mic.pop();
            }
            // Always drain the mic queue so latency doesn't build up while muted.
            let muted = clips_playing && controls.mute_mic_during_clips.load(Ordering::Relaxed);
            let gain = controls.mic_gain();
            for slot in mono.iter_mut() {
                let m = mic.pop().unwrap_or(0.0);
                if !muted {
                    *slot += m * gain;
                }
            }
            spread(mono, out, channels);
        },
        err_fn,
        None,
    )?;
    stream.play()?;
    Ok((stream, Sink { new_voices, stop, rate: config.sample_rate }))
}

fn build_monitor(dev: &Device) -> Result<(Stream, Sink)> {
    let config = out_config(dev)?;
    let channels = config.channels as usize;
    let new_voices = Arc::new(ArrayQueue::new(32));
    let stop = Arc::new(AtomicBool::new(false));
    let (nv, st) = (new_voices.clone(), stop.clone());
    let mut voices: Vec<Voice> = Vec::new();
    let mut mono = vec![0.0f32; 8192];

    let stream = dev.build_output_stream(
        config,
        move |out: &mut [f32], _| {
            let frames = out.len() / channels;
            if mono.len() < frames {
                mono.resize(frames, 0.0);
            }
            let mono = &mut mono[..frames];
            mono.fill(0.0);
            render_voices(&mut voices, &nv, &st, mono);
            spread(mono, out, channels);
        },
        err_fn,
        None,
    )?;
    stream.play()?;
    Ok((stream, Sink { new_voices, stop, rate: config.sample_rate }))
}

/// Captures the mic as mono f32 and resamples it (linear) to the cable's rate.
fn build_mic(dev: &Device, queue: Arc<ArrayQueue<f32>>, out_rate: u32) -> Result<Stream> {
    let c = dev.default_input_config().context("no input config")?;
    if c.sample_format() != cpal::SampleFormat::F32 {
        return Err(anyhow!("device does not offer f32 samples ({})", c.sample_format()));
    }
    let config: StreamConfig = c.into();
    let channels = config.channels as usize;
    let step = config.sample_rate as f64 / out_rate as f64;
    let (mut pos, mut cur) = (0.0f64, 0.0f32);

    let stream = dev.build_input_stream(
        config,
        move |data: &[f32], _| {
            for frame in data.chunks(channels) {
                let prev = cur;
                cur = frame.iter().sum::<f32>() / channels as f32;
                // Emit an output sample for every `step` input samples consumed.
                pos += 1.0;
                while pos >= step {
                    pos -= step;
                    let t = (1.0 - pos).clamp(0.0, 1.0) as f32;
                    let _ = queue.push(prev + (cur - prev) * t);
                }
            }
        },
        err_fn,
        None,
    )?;
    stream.play()?;
    Ok(stream)
}
