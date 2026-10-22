//! Global hotkeys: string <-> key combination, plus registration bookkeeping.

use std::collections::HashMap;
use std::str::FromStr;

use anyhow::{Result, anyhow};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    /// hotkey id -> clip id
    bound: HashMap<u32, u64>,
    hotkeys: HashMap<u64, HotKey>,
}

impl Hotkeys {
    pub fn new() -> Result<Self> {
        Ok(Self { manager: GlobalHotKeyManager::new()?, bound: HashMap::new(), hotkeys: HashMap::new() })
    }

    /// Replaces any existing binding for `clip`. `None` just clears it.
    pub fn bind(&mut self, clip: u64, hotkey: Option<&str>) -> Result<()> {
        if let Some(old) = self.hotkeys.remove(&clip) {
            let _ = self.manager.unregister(old);
            self.bound.remove(&old.id());
        }
        let Some(text) = hotkey else { return Ok(()) };
        let hk = HotKey::from_str(text).map_err(|e| anyhow!("bad hotkey {text:?}: {e}"))?;
        if let Some(other) = self.bound.get(&hk.id()) {
            return Err(anyhow!("{text} is already used by another clip (#{other})"));
        }
        self.manager.register(hk).map_err(|e| anyhow!("could not register {text}: {e}"))?;
        self.bound.insert(hk.id(), clip);
        self.hotkeys.insert(clip, hk);
        Ok(())
    }

    /// Clip ids whose hotkeys were pressed since the last call.
    pub fn pressed(&self) -> Vec<u64> {
        GlobalHotKeyEvent::receiver()
            .try_iter()
            .filter(|e| e.state == HotKeyState::Pressed)
            .filter_map(|e| self.bound.get(&e.id).copied())
            .collect()
    }
}

/// Builds a hotkey string from an egui key press, or `None` for keys we can't bind.
pub fn from_egui(key: egui::Key, m: egui::Modifiers) -> Option<String> {
    let name = key.name();
    let code = if name.len() == 1 && name.as_bytes()[0].is_ascii_alphabetic() {
        format!("Key{name}")
    } else if name.len() == 1 && name.as_bytes()[0].is_ascii_digit() {
        format!("Digit{name}")
    } else {
        name.to_string()
    };
    let code = Code::from_str(&code).ok()?;
    let mut mods = Modifiers::empty();
    mods.set(Modifiers::CONTROL, m.ctrl);
    mods.set(Modifiers::ALT, m.alt);
    mods.set(Modifiers::SHIFT, m.shift);
    mods.set(Modifiers::META, m.mac_cmd);
    Some(HotKey::new(Some(mods).filter(|m| !m.is_empty()), code).into_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egui_key_roundtrips_through_hotkey_parser() {
        let m = egui::Modifiers { ctrl: true, shift: true, ..Default::default() };
        for key in [egui::Key::A, egui::Key::Num5, egui::Key::F9, egui::Key::Space] {
            let s = from_egui(key, m).unwrap();
            HotKey::from_str(&s).unwrap_or_else(|e| panic!("{s}: {e}"));
        }
    }
}
