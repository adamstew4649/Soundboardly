use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use anyhow::{Context as _, Result};

use crate::audio::{self, Controls, DeviceNames, Engine, EngineConfig};
use crate::clip::{self, Audio, ClipMeta, MAX_SPEED, MIN_SPEED};
use crate::download;
use crate::hotkeys::{self, Hotkeys};
use crate::library::Library;

type DownloadResult = Result<(String, std::path::PathBuf, u64)>;

pub struct App {
    ctx: egui::Context,
    library: Library,
    engine: Engine,
    controls: Arc<Controls>,
    hotkeys: Hotkeys,
    devices: DeviceNames,
    cache: HashMap<u64, Arc<Audio>>,
    url: String,
    downloading: usize,
    downloads: Receiver<DownloadResult>,
    downloads_tx: std::sync::mpsc::Sender<DownloadResult>,
    selected: Option<u64>,
    capturing: Option<u64>,
    status: String,
}

impl App {
    pub fn new(ctx: &egui::Context) -> Result<Self> {
        let root = directories::ProjectDirs::from("", "", "Soundboardly")
            .context("no user data directory")?
            .data_dir()
            .to_path_buf();
        let library = Library::open(root)?;
        let s = library.settings();
        let controls = audio::new_controls(s.mute_mic_during_clips, s.mic_volume.unwrap_or(1.0));
        let (engine, warnings) = start_engine(&library, controls.clone());
        let (downloads_tx, downloads) = channel();
        let mut app = App {
            ctx: ctx.clone(),
            library,
            engine,
            controls,
            hotkeys: Hotkeys::new()?,
            devices: audio::list_devices(),
            cache: HashMap::new(),
            url: String::new(),
            downloading: 0,
            downloads,
            downloads_tx,
            selected: None,
            capturing: None,
            status: warnings.join("; "),
        };
        let bindings: Vec<_> = app.library.clips().iter().map(|c| (c.id, c.hotkey.clone())).collect();
        for (id, hk) in bindings {
            if let Err(e) = app.hotkeys.bind(id, hk.as_deref()) {
                app.status = format!("{e:#}");
            }
        }
        Ok(app)
    }

    fn audio_for(&mut self, id: u64) -> Result<Arc<Audio>> {
        if let Some(a) = self.cache.get(&id) {
            return Ok(a.clone());
        }
        let meta = self.library.clips().iter().find(|c| c.id == id).context("unknown clip")?;
        let audio = Arc::new(clip::load_wav(&self.library.clip_path(meta))?);
        self.cache.insert(id, audio.clone());
        Ok(audio)
    }

    fn play(&mut self, id: u64) {
        let Some(meta) = self.library.clips().iter().find(|c| c.id == id).cloned() else { return };
        match self.audio_for(id) {
            Ok(audio) => self.engine.play(&audio, &meta),
            Err(e) => self.status = format!("{e:#}"),
        }
    }

    fn start_download(&mut self) {
        let url = self.url.trim().to_string();
        if url.is_empty() {
            return;
        }
        let id = self.library.next_id();
        let stem = self.library.clips_dir().join(format!("clip{id}"));
        let (tx, ctx) = (self.downloads_tx.clone(), self.ctx.clone());
        self.downloading += 1;
        self.status = "Downloading...".into();
        std::thread::spawn(move || {
            let res = download::fetch(&url, &stem).map(|d| (d.title, d.wav, id));
            let _ = tx.send(res);
            ctx.request_repaint();
        });
    }

    fn finish_downloads(&mut self) {
        while let Ok(res) = self.downloads.try_recv() {
            self.downloading -= 1;
            match res {
                Ok((title, wav, id)) => {
                    let file = wav.file_name().unwrap().to_string_lossy().into_owned();
                    self.library.add(ClipMeta { id, name: title, file, start: 0.0, end: None, speed: 1.0, volume: 1.0, hotkey: None });
                    self.selected = Some(id);
                    self.status = "Clip added".into();
                    self.save();
                }
                Err(e) => self.status = format!("{e:#}"),
            }
        }
    }

    fn save(&mut self) {
        if let Err(e) = self.library.save() {
            self.status = format!("could not save library: {e:#}");
        }
    }

    fn restart_engine(&mut self) {
        let (engine, warnings) = start_engine(&self.library, self.controls.clone());
        self.engine = engine;
        self.status = if warnings.is_empty() { "Devices applied".into() } else { warnings.join("; ") };
    }

    fn capture_hotkey(&mut self, ctx: &egui::Context) {
        let Some(id) = self.capturing else { return };
        let pressed = ctx.input(|i| {
            if i.key_pressed(egui::Key::Escape) {
                return Some(None);
            }
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => hotkeys::from_egui(*key, *modifiers).map(Some),
                _ => None,
            })
        });
        let Some(choice) = pressed else { return };
        self.capturing = None;
        match self.hotkeys.bind(id, choice.as_deref()) {
            Ok(()) => {
                if let Some(c) = self.library.clip_mut(id) {
                    c.hotkey = choice;
                }
                self.save();
            }
            Err(e) => self.status = format!("{e:#}"),
        }
    }
}

fn start_engine(library: &Library, controls: Arc<Controls>) -> (Engine, Vec<String>) {
    let s = library.settings();
    Engine::start(
        EngineConfig { mic: s.mic_device.as_deref(), cable: s.cable_device.as_deref(), monitor: s.monitor_device.as_deref() },
        controls,
    )
}

fn device_combo(ui: &mut egui::Ui, label: &str, current: &mut Option<String>, names: &[String], default_label: &str) -> bool {
    let before = current.clone();
    egui::ComboBox::from_label(label)
        .selected_text(current.as_deref().unwrap_or(default_label))
        .width(260.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(current, None, default_label);
            for n in names {
                ui.selectable_value(current, Some(n.clone()), n);
            }
        });
    *current != before
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        for id in self.hotkeys.pressed() {
            self.play(id);
        }
        self.finish_downloads();
        self.capture_hotkey(ctx);
        // Global hotkeys arrive on a channel, not as window events, so poll.
        ctx.request_repaint_after(std::time::Duration::from_millis(30));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("URL:");
                let r = ui.add(egui::TextEdit::singleline(&mut self.url).desired_width(420.0).hint_text("YouTube / TikTok / X link"));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.button("Download clip").clicked() || enter) && !self.url.trim().is_empty() {
                    self.start_download();
                    self.url.clear();
                }
                if self.downloading > 0 {
                    ui.spinner();
                }
            });
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.label(&self.status);
        });

        egui::Panel::right("side").min_size(280.0).show(ui, |ui| {
            ui.heading("Output");
            let mut changed = false;
            let mut settings = std::mem::take(self.library.settings_mut());
            changed |= device_combo(ui, "Microphone", &mut settings.mic_device, &self.devices.inputs, "System default");
            changed |= device_combo(ui, "Virtual cable", &mut settings.cable_device, &self.devices.outputs, "(none)");
            changed |= device_combo(ui, "Monitor", &mut settings.monitor_device, &self.devices.outputs, "System default");
            let mut mute = settings.mute_mic_during_clips;
            if ui.checkbox(&mut mute, "Mute my mic while clips play").changed() {
                settings.mute_mic_during_clips = mute;
                self.controls.mute_mic_during_clips.store(mute, std::sync::atomic::Ordering::Relaxed);
                changed = true;
            }
            let mut gain = settings.mic_volume.unwrap_or(1.0);
            if ui.add(egui::Slider::new(&mut gain, 0.0..=2.0).text("Mic volume")).changed() {
                settings.mic_volume = Some(gain);
                self.controls.set_mic_gain(gain);
                changed = true;
            }
            *self.library.settings_mut() = settings;
            if changed {
                self.save();
            }
            if ui.button("Apply devices").clicked() {
                self.restart_engine();
            }
            ui.small("Pick e.g. \"CABLE Input (VB-Audio Virtual Cable)\" as Virtual cable, then choose \"CABLE Output\" as the microphone in Discord, games, etc.");
            ui.separator();

            ui.heading("Edit clip");
            let Some(id) = self.selected else {
                ui.label("Select a clip.");
                return;
            };
            let duration = self.audio_for(id).map(|a| a.duration()).unwrap_or(0.0);
            let mut play = false;
            let mut changed = false;
            if let Some(c) = self.library.clip_mut(id) {
                ui.text_edit_singleline(&mut c.name);
                let mut end = c.end.unwrap_or(duration);
                changed |= ui.add(egui::Slider::new(&mut c.start, 0.0..=duration).text("Start (s)")).changed();
                changed |= ui.add(egui::Slider::new(&mut end, 0.0..=duration).text("End (s)")).changed();
                c.end = (end < duration).then_some(end.max(c.start));
                c.start = c.start.min(end);
                changed |= ui.add(egui::Slider::new(&mut c.speed, MIN_SPEED..=MAX_SPEED).logarithmic(true).text("Speed")).changed();
                changed |= ui.add(egui::Slider::new(&mut c.volume, 0.0..=2.0).text("Volume")).changed();
                play = ui.button("Preview").clicked();
            }
            if changed {
                self.save();
            }
            if play {
                self.play(id);
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            if self.library.clips().is_empty() {
                ui.label("No clips yet - paste a link above.");
            }
            let mut play = None;
            let mut delete = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for c in self.library.clips() {
                    ui.horizontal(|ui| {
                        if ui.button("▶").clicked() {
                            play = Some(c.id);
                        }
                        if ui.selectable_label(self.selected == Some(c.id), &c.name).clicked() {
                            self.selected = Some(c.id);
                        }
                        let label = if self.capturing == Some(c.id) {
                            "press keys (Esc clears)".to_string()
                        } else {
                            c.hotkey.clone().unwrap_or_else(|| "set hotkey".into())
                        };
                        if ui.small_button(label).clicked() {
                            self.capturing = Some(c.id);
                        }
                        if ui.small_button("🗑").clicked() {
                            delete = Some(c.id);
                        }
                    });
                }
            });
            if let Some(id) = play {
                self.play(id);
            }
            if let Some(id) = delete {
                let _ = self.hotkeys.bind(id, None);
                self.library.remove(id);
                self.cache.remove(&id);
                if self.selected == Some(id) {
                    self.selected = None;
                }
                self.save();
            }
            if ui.button("Stop all").clicked() {
                self.engine.stop_all();
            }
        });
    }
}
