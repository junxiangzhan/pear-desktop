//! Gecko settings that only take effect when Gecko starts (Android).
//!
//! GeckoView reads its prefs once, from the config file Kotlin writes before
//! it starts Gecko (`GeckoPage.writeConfig`). What depends on the app config
//! is decided here and kept in `gecko-startup.json` next to `config.json`,
//! for Kotlin to read at the next start; a change asks for a restart, like a
//! `restartNeeded` plugin.
//!
//! The GPU process: off by default, because Android kills it soon after the
//! app goes to the background (seen on a POCO F7 Pro: within 30 s), taking
//! the media decoders along. Gecko's WebGL needs it on Android, so it is on
//! while something uses WebGL: the visualizer's butterchurn type.

use std::{fs, path::Path, sync::Mutex};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::config::Config;

const STARTUP_FILE: &str = "gecko-startup.json";
/// What Gecko started with: GeckoPage writes it into the app's files dir
const GECKO_CONFIG: &str = "files/geckoview-config.yaml";
const GPU_PROCESS_ON: &str = "layers.gpu-process.enabled: true";

/// The plugin whose option needs WebGL, named in the restart dialog
const WEBGL_PLUGIN: &str = "visualizer";

/// The last decided value, so only a change of it asks for a restart
static WANTED: Mutex<Option<bool>> = Mutex::new(None);

/// Whether Gecko should run its GPU process (for WebGL): the visualizer on
/// with butterchurn, its default type (the default is assumed before the
/// page has sent the plugins' defaults)
fn wants_gpu_process(config: &Config) -> bool {
    let visualizer = config.plugin_config(WEBGL_PLUGIN);
    visualizer["enabled"].as_bool().unwrap_or(false)
        && visualizer["type"].as_str().unwrap_or("butterchurn") == "butterchurn"
}

/// Whether the running Gecko started with its GPU process
fn running_gpu_process(dir: &Path) -> bool {
    fs::read_to_string(dir.join(GECKO_CONFIG))
        .is_ok_and(|text| text.lines().any(|line| line.trim() == GPU_PROCESS_ON))
}

/// Brings `gecko-startup.json` up to date with the config. After a change
/// (`at_start` false) that the running Gecko does not match, offers to
/// restart.
pub fn sync(app: &AppHandle, at_start: bool) {
    let config = app.state::<Config>();
    let Some(dir) = config.dir() else { return };
    let wanted = wants_gpu_process(&config);

    let changed = {
        let mut last = WANTED.lock().unwrap();
        let changed = last.is_some_and(|last| last != wanted);
        *last = Some(wanted);
        changed
    };

    let path = dir.join(STARTUP_FILE);
    let startup = json!({ "gpuProcess": wanted });
    let stored = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    if stored.as_ref() != Some(&startup) {
        if let Err(error) = fs::write(&path, startup.to_string()) {
            log::warn!("writing {} failed: {error}", path.display());
        }
    }

    if !at_start && changed && wanted != running_gpu_process(dir) {
        crate::dialogs::need_restart(app, WEBGL_PLUGIN);
    }
}
