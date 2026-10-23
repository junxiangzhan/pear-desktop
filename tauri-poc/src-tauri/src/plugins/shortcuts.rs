//! Ports of the global-shortcut parts of `shortcuts` (media keys, user
//! global/local shortcuts) and `precise-volume` (volume up/down keys).
//! MPRIS (Linux) is not ported yet.
//!
//! Local shortcuts were `electron-localshortcut`, which registers global
//! shortcuts while the window is focused; done the same way here.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use super::Plugin;
use crate::{ipc::{send_to_page, MAIN_WEBVIEW}, song_controls};

/// Electron accelerator names that `global-hotkey` spells differently.
pub fn normalize_accelerator(accelerator: &str) -> String {
    accelerator
        .split('+')
        .map(|part| match part.trim() {
            "MediaNextTrack" => "MediaTrackNext".to_owned(),
            "MediaPreviousTrack" => "MediaTrackPrevious".to_owned(),
            "VolumeUp" => "AudioVolumeUp".to_owned(),
            "VolumeDown" => "AudioVolumeDown".to_owned(),
            "VolumeMute" => "AudioVolumeMute".to_owned(),
            "Esc" => "Escape".to_owned(),
            "Plus" => "Equal".to_owned(),
            num if num.len() == 4 && num.starts_with("num") => format!("Numpad{}", &num[3..]),
            other => other.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Shortcuts a plugin registered, so `stop` can release them.
#[derive(Default)]
struct Registered(Mutex<Vec<String>>);

impl Registered {
    fn register(&self, app: &AppHandle, accelerator: &str, action: impl Fn(&AppHandle) + Send + Sync + 'static) {
        let normalized = normalize_accelerator(accelerator);
        let result = app.global_shortcut().on_shortcut(normalized.as_str(), move |app, _, event| {
            if event.state() == ShortcutState::Pressed {
                action(app);
            }
        });
        match result {
            Ok(()) => self.0.lock().unwrap().push(normalized),
            Err(error) => log::warn!("registering shortcut '{accelerator}' failed: {error}"),
        }
    }

    fn release(&self, app: &AppHandle) {
        for accelerator in self.0.lock().unwrap().drain(..) {
            let _ = app.global_shortcut().unregister(accelerator.as_str());
        }
    }
}

fn control(action: &str) -> Option<fn(&AppHandle)> {
    match action {
        "previous" => Some(song_controls::previous),
        "playPause" => Some(song_controls::play_pause),
        "next" => Some(song_controls::next),
        _ => None,
    }
}

pub struct Shortcuts {
    global: Arc<Registered>,
    local: Arc<Registered>,
    /// Gates the focus listener of the current run (listeners can't be removed)
    active: Mutex<Arc<AtomicBool>>,
}

impl Shortcuts {
    pub fn new() -> Self {
        Self {
            global: Arc::default(),
            local: Arc::default(),
            active: Mutex::new(Arc::new(AtomicBool::new(false))),
        }
    }
}

fn register_mapping(registered: &Registered, app: &AppHandle, mapping: &Value) {
    let Some(mapping) = mapping.as_object() else { return };
    for (action, accelerator) in mapping {
        let Some(accelerator) = accelerator.as_str().filter(|a| !a.is_empty()) else { continue };
        match control(action) {
            Some(callback) => registered.register(app, accelerator, callback),
            None => log::warn!("shortcuts: invalid action '{action}'"),
        }
    }
}

impl Plugin for Shortcuts {
    fn id(&self) -> &'static str {
        "shortcuts"
    }

    fn start(&self, app: &AppHandle, config: &Value) {
        if config["overrideMediaKeys"].as_bool().unwrap_or(false) {
            self.global.register(app, "MediaPlayPause", song_controls::play_pause);
            self.global.register(app, "MediaNextTrack", song_controls::next);
            self.global.register(app, "MediaPreviousTrack", song_controls::previous);
        }
        register_mapping(&self.global, app, &config["global"]);

        let active = Arc::new(AtomicBool::new(true));
        *self.active.lock().unwrap() = active.clone();
        let Some(window) = app.get_webview_window(MAIN_WEBVIEW) else { return };
        if window.is_focused().unwrap_or(false) {
            register_mapping(&self.local, app, &config["local"]);
        }
        let (app, local, mapping) = (app.clone(), self.local.clone(), config["local"].clone());
        window.on_window_event(move |event| {
            if !active.load(Ordering::Relaxed) {
                return;
            }
            if let WindowEvent::Focused(focused) = event {
                local.release(&app);
                if *focused {
                    register_mapping(&local, &app, &mapping);
                }
            }
        });
    }

    fn stop(&self, app: &AppHandle) {
        self.active.lock().unwrap().store(false, Ordering::Relaxed);
        self.global.release(app);
        self.local.release(app);
    }
}

pub struct PreciseVolume {
    registered: Registered,
}

impl PreciseVolume {
    pub fn new() -> Self {
        Self { registered: Registered::default() }
    }
}

impl Plugin for PreciseVolume {
    fn id(&self) -> &'static str {
        "precise-volume"
    }

    fn start(&self, app: &AppHandle, config: &Value) {
        let shortcuts = &config["globalShortcuts"];
        if let Some(up) = shortcuts["volumeUp"].as_str().filter(|s| !s.is_empty()) {
            self.registered.register(app, up, |app| send_to_page(app, "changeVolume", &[json!(true)]));
        }
        if let Some(down) = shortcuts["volumeDown"].as_str().filter(|s| !s.is_empty()) {
            self.registered.register(app, down, |app| send_to_page(app, "changeVolume", &[json!(false)]));
        }
    }

    fn stop(&self, app: &AppHandle) {
        self.registered.release(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_electron_key_names() {
        assert_eq!(normalize_accelerator("MediaNextTrack"), "MediaTrackNext");
        assert_eq!(normalize_accelerator("Ctrl+Shift+VolumeUp"), "Ctrl+Shift+AudioVolumeUp");
        assert_eq!(normalize_accelerator("Alt+num5"), "Alt+Numpad5");
        assert_eq!(normalize_accelerator("CmdOrCtrl+P"), "CmdOrCtrl+P");
    }
}
