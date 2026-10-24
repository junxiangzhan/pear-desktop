//! App-level Android commands through `AndroidAppPlugin`
//! (gen/android/.../AndroidAppPlugin.kt):
//! - full screen (View › Toggle full screen): hides the status and navigation
//!   bars, including the gesture handle; swiping from an edge shows them for
//!   a moment. The choice is kept in `options.fullscreen`.
//! - restart: Tauri's `restart()` spawns the binary like a desktop app, which
//!   on Android only ends the process.

use tauri::{
    plugin::{Builder, PluginHandle, TauriPlugin},
    AppHandle, Manager, Wry,
};

use crate::config::Config;

struct AndroidApp(PluginHandle<Wry>);

pub fn init() -> TauriPlugin<Wry> {
    Builder::new("pear-android-app")
        .setup(|app, api| {
            let handle = api.register_android_plugin("com.github.pear_devs.tauri_poc", "AndroidAppPlugin")?;
            app.manage(AndroidApp(handle));
            Ok(())
        })
        .build()
}

/// Applies the saved full screen choice at startup.
pub fn register(app: &AppHandle) {
    if app.state::<Config>().get_bool("options.fullscreen") {
        set_fullscreen(app, true);
    }
}

/// Plugin calls block until Kotlin answers on the UI thread, so these run on
/// a blocking thread (they are also called from the main thread).
fn call(app: &AppHandle, command: &'static str, args: serde_json::Value) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = app.state::<AndroidApp>().0.run_mobile_plugin::<()>(command, args) {
            log::warn!("android app: {command} failed: {error}");
        }
    });
}

pub fn set_fullscreen(app: &AppHandle, fullscreen: bool) {
    call(app, "setFullscreen", serde_json::json!({ "fullscreen": fullscreen }));
}

pub fn restart(app: &AppHandle) {
    call(app, "restart", serde_json::json!({}));
}
