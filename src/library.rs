//! Persistent clip library and app settings, stored as JSON in the user data dir.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::clip::ClipMeta;

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    pub mic_device: Option<String>,
    /// Virtual cable (e.g. "CABLE Input") that other apps record as their mic.
    pub cable_device: Option<String>,
    /// Where you hear the clips yourself. `None` = system default output.
    pub monitor_device: Option<String>,
    pub mute_mic_during_clips: bool,
    pub mic_volume: Option<f32>,
}

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    next_id: u64,
    clips: Vec<ClipMeta>,
    settings: Settings,
}

pub struct Library {
    root: PathBuf,
    stored: Stored,
}

impl Library {
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(root.join("clips"))?;
        let path = root.join("library.json");
        let stored = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Stored::default(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self { root, stored })
    }

    pub fn clips_dir(&self) -> PathBuf {
        self.root.join("clips")
    }

    pub fn clip_path(&self, clip: &ClipMeta) -> PathBuf {
        self.clips_dir().join(&clip.file)
    }

    pub fn clips(&self) -> &[ClipMeta] {
        &self.stored.clips
    }

    pub fn clip_mut(&mut self, id: u64) -> Option<&mut ClipMeta> {
        self.stored.clips.iter_mut().find(|c| c.id == id)
    }

    pub fn settings(&self) -> &Settings {
        &self.stored.settings
    }

    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.stored.settings
    }

    pub fn next_id(&mut self) -> u64 {
        self.stored.next_id += 1;
        self.stored.next_id
    }

    pub fn add(&mut self, clip: ClipMeta) {
        self.stored.clips.push(clip);
    }

    pub fn remove(&mut self, id: u64) {
        if let Some(i) = self.stored.clips.iter().position(|c| c.id == id) {
            let clip = self.stored.clips.remove(i);
            let _ = fs::remove_file(self.clip_path(&clip));
        }
    }

    /// Writes via a temp file so a crash can't leave a truncated library.
    pub fn save(&self) -> Result<()> {
        let path = self.root.join("library.json");
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&self.stored)?)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_and_removes() {
        let dir = std::env::temp_dir().join(format!("sbly-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut lib = Library::open(dir.clone()).unwrap();
        let id = lib.next_id();
        let clip = ClipMeta { id, name: "a".into(), file: "a.wav".into(), start: 0.0, end: None, speed: 1.0, volume: 1.0, hotkey: Some("ctrl+KeyA".into()) };
        fs::write(lib.clip_path(&clip), b"x").unwrap();
        lib.add(clip);
        lib.settings_mut().mute_mic_during_clips = true;
        lib.save().unwrap();

        let mut lib = Library::open(dir.clone()).unwrap();
        assert_eq!(lib.clips().len(), 1);
        assert!(lib.settings().mute_mic_during_clips);
        assert_eq!(lib.next_id(), id + 1);
        lib.remove(id);
        assert!(lib.clips().is_empty());
        assert!(!dir.join("clips/a.wav").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
