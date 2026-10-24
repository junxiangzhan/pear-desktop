//! Android full screen (View › Toggle full screen): hides the status and
//! navigation bars, including the gesture handle, through `SystemBarsPlugin`
//! (gen/android/.../SystemBarsPlugin.kt). Swiping from an edge shows them
//! for a moment. The choice is kept in `options.fullscreen`.

use tauri::{
    plugin::{Builder, PluginHandle, TauriPlugin},
    AppHandle, Manager, Wry,
};

use crate::config::Config;

struct SystemBars(PluginHandle<Wry>);

pub fn init() -> TauriPlugin<Wry> {
    Builder::new("pear-system-bars")
        .setup(|app, api| {
            let handle = api.register_android_plugin("com.github.pear_devs.tauri_poc", "SystemBarsPlugin")?;
            app.manage(SystemBars(handle));
            Ok(())
        })
        .build()
}

/// Applies the saved choice at startup.
pub fn register(app: &AppHandle) {
    if app.state::<Config>().get_bool("options.fullscreen") {
        set_fullscreen(app, true);
    }
}

pub fn set_fullscreen(app: &AppHandle, fullscreen: bool) {
    // Plugin calls block until Kotlin answers on the UI thread
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let args = serde_json::json!({ "fullscreen": fullscreen });
        if let Err(error) = app.state::<SystemBars>().0.run_mobile_plugin::<()>("setFullscreen", args) {
            log::warn!("system bars: setting full screen failed: {error}");
        }
    });
}
