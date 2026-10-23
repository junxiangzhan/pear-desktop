//! Port of `src/plugins/transparent-player/backend.ts`: a window material
//! (Mica/Acrylic/Tabbed on Windows, vibrancy on macOS) behind a translucent
//! background. The main window is created transparent when this plugin is
//! enabled at startup (it is `restartNeeded`), see `lib.rs`.

use serde_json::Value;
use tauri::{
    window::{Color, Effect, EffectsBuilder},
    AppHandle, Manager,
};

use super::Plugin;
use crate::ipc::MAIN_WEBVIEW;

pub struct TransparentPlayer;

fn effect_for(material: &str) -> Option<Effect> {
    match material {
        #[cfg(windows)]
        "mica" => Some(Effect::Mica),
        #[cfg(windows)]
        "acrylic" => Some(Effect::Acrylic),
        #[cfg(windows)]
        "tabbed" => Some(Effect::Tabbed),
        #[cfg(target_os = "macos")]
        "window" => Some(Effect::WindowBackground),
        #[cfg(target_os = "macos")]
        "fullscreen-ui" => Some(Effect::FullScreenUI),
        #[cfg(target_os = "macos")]
        "content" => Some(Effect::ContentBackground),
        #[cfg(target_os = "macos")]
        "under-window" => Some(Effect::UnderWindowBackground),
        #[cfg(target_os = "macos")]
        "under-page" => Some(Effect::UnderPageBackground),
        _ => None,
    }
}

/// Port of `setWindowTransparency`.
fn apply(app: &AppHandle, material: &str, opacity: f64) {
    let Some(window) = app.get_webview_window(MAIN_WEBVIEW) else { return };
    let effects = effect_for(material).map(|effect| EffectsBuilder::new().effect(effect).build());
    if let Err(error) = window.set_effects(effects) {
        log::warn!("transparent-player: setting '{material}' failed: {error}");
    }
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    let _ = window.set_background_color(Some(Color(0, 0, 0, alpha)));
}

impl Plugin for TransparentPlayer {
    fn id(&self) -> &'static str {
        "transparent-player"
    }

    fn start(&self, app: &AppHandle, config: &Value) {
        self.on_config_change(app, config);
    }

    fn on_config_change(&self, app: &AppHandle, config: &Value) {
        apply(
            app,
            config["type"].as_str().unwrap_or("none"),
            config["opacity"].as_f64().unwrap_or(1.0),
        );
    }

    fn stop(&self, app: &AppHandle) {
        apply(app, "none", 1.0);
    }
}
