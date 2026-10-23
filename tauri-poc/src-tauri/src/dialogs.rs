//! Message boxes from `src/index.ts`, on `tauri-plugin-dialog`.

use serde_json::{json, Value};
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tokio::sync::oneshot;

use crate::ipc::Ipc;

/// The "About" menu item (Electron's `role: 'about'`).
#[cfg_attr(mobile, allow(dead_code))] // no native menu on mobile
pub fn about(app: &AppHandle) {
    let info = app.package_info();
    app.dialog()
        .message(format!(
            "{} {}\nRunning on Tauri {}",
            info.name,
            info.version,
            tauri::VERSION
        ))
        .title("About")
        .kind(MessageDialogKind::Info)
        .show(|_| {});
}

#[cfg_attr(mobile, allow(dead_code))]
pub fn error(app: &AppHandle, title: &str, message: &str) {
    app.dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Error)
        .show(|_| {});
}

/// Port of `showNeedToRestartDialog`.
pub fn need_restart(app: &AppHandle, plugin_id: &str) {
    let app_for_answer = app.clone();
    app.dialog()
        .message(format!(
            "\"{plugin_id}\" needs to restart the app to take effect.\nRestart now?"
        ))
        .title("Restart needed")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Restart now".into(),
            "Later".into(),
        ))
        .show(move |restart| {
            if restart {
                app_for_answer.restart();
            }
        });
}

/// `dialog.showOpenDialog` for page-side menus (`inject/compat/electron.ts`):
/// title and filters, one file, answered like Electron.
pub fn register_ipc(ipc: &Ipc) {
    ipc.handle("core", "pear:show-open-dialog", |app, args| async move {
        let options = args.into_iter().next().unwrap_or(Value::Null);
        let mut dialog = app.dialog().file();
        if let Some(title) = options["title"].as_str() {
            dialog = dialog.set_title(title);
        }
        for filter in options["filters"].as_array().into_iter().flatten() {
            let extensions: Vec<&str> = filter["extensions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            dialog = dialog.add_filter(filter["name"].as_str().unwrap_or_default(), &extensions);
        }
        let (tx, rx) = oneshot::channel();
        dialog.pick_file(move |path| {
            let _ = tx.send(path);
        });
        let path = rx.await.ok().flatten().and_then(|path| path.into_path().ok());
        Ok(match path {
            Some(path) => json!({ "canceled": false, "filePaths": [path] }),
            None => json!({ "canceled": true, "filePaths": [] }),
        })
    });
}
