//! Port of `src/providers/app-controls.ts`.

use std::path::PathBuf;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::{config::Config, ipc::Ipc};

/// Like `restart()`: relaunches the app.
pub fn restart(app: &AppHandle) {
    app.restart();
}

/// Like the `peard:reload` handler: loads the configured URL again (used by
/// the Retry button of the error page).
pub fn reload(app: &AppHandle) {
    let url = app.state::<Config>().get_str("url");
    let Some(window) = app.get_webview_window(crate::ipc::MAIN_WEBVIEW) else {
        return;
    };
    match url.parse() {
        Ok(url) => {
            let _ = window.navigate(url);
        }
        Err(_) => {
            let _ = window.reload();
        }
    }
}

pub fn register_ipc(ipc: &Ipc) {
    ipc.on("core", "peard:restart", |app, _| restart(app));
    ipc.on("core", "peard:reload", |app, _| reload(app));
    ipc.handle("core", "peard:get-downloads-folder", |app, _| async move {
        let dir = app.path().download_dir().map_err(|e| e.to_string())?;
        Ok(json!(dir))
    });
    ipc.handle("core", "peard:get-path", |_, args| async move {
        let path: PathBuf = args.iter().filter_map(Value::as_str).collect();
        Ok(json!(path))
    });
}

#[tauri::command]
pub fn app_restart(app: AppHandle) {
    restart(&app);
}
