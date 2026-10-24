//! App-level Android commands through `AndroidAppPlugin`
//! (gen/android/.../AndroidAppPlugin.kt):
//! - full screen (View › Toggle full screen): hides the status and navigation
//!   bars, including the gesture handle; swiping from an edge shows them for
//!   a moment. The choice is kept in `options.fullscreen`.
//! - restart: Tauri's `restart()` spawns the binary like a desktop app, which
//!   on Android only ends the process.
//! - prompt: a native dialog for `prompt::open`, which has no prompt
//!   windows on Android.
//! - audioExists / saveAudio: the downloader's files in the shared
//!   Music/Pear Desktop collection (MediaStore).

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

#[derive(serde::Deserialize)]
struct Answer<T> {
    value: T,
}

/// A plugin call that waits for Kotlin's `{ value }` answer.
async fn ask<T: serde::de::DeserializeOwned + Send + 'static>(
    app: &AppHandle,
    command: &'static str,
    args: serde_json::Value,
) -> Result<T, String> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AndroidApp>()
            .0
            .run_mobile_plugin::<Answer<T>>(command, args)
            .map(|answer| answer.value)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Shows a prompt (`custom-electron-prompt` options) and waits for the
/// answer: the value, or `null` when cancelled.
pub async fn prompt(app: &AppHandle, options: serde_json::Value) -> Result<serde_json::Value, String> {
    ask(app, "prompt", options).await
}

/// Whether the downloader already saved `name` into Music/Pear Desktop (and
/// `subfolder`).
pub async fn audio_exists(app: &AppHandle, name: &str, subfolder: Option<&str>) -> Result<bool, String> {
    ask(app, "audioExists", serde_json::json!({ "name": name, "subfolder": subfolder })).await
}

/// Copies a downloaded file into Music/Pear Desktop (and `subfolder`),
/// replacing a file of the same name; returns its content URI.
pub async fn save_audio(
    app: &AppHandle,
    path: &std::path::Path,
    name: &str,
    subfolder: Option<&str>,
    mime: &str,
) -> Result<String, String> {
    ask(
        app,
        "saveAudio",
        serde_json::json!({ "path": path, "name": name, "subfolder": subfolder, "mime": mime }),
    )
    .await
}
