//! Message boxes from `src/index.ts`, on `tauri-plugin-dialog`.

use serde_json::{json, Value};
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tokio::sync::oneshot;

use crate::ipc::Ipc;

/// The "About" menu item (Electron's `role: 'about'`). Says it is not an
/// official Pear Desktop release, so problems aren't reported there.
pub fn about(app: &AppHandle) {
    let info = app.package_info();
    app.dialog()
        .message(format!(
            "{} {}\nUnofficial experimental Tauri port of Pear Desktop\nRunning on Tauri {}",
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
                crate::app_controls::restart(&app_for_answer);
            }
        });
}

/// `showOpenDialog` for page-side menus (`src/platform/dialog.ts`):
/// title, filters and `defaultPath`, one file or one folder
/// (`openDirectory`), answered like Electron.
pub fn register_ipc(ipc: &Ipc) {
    ipc.handle("core", "pear:show-open-dialog", |app, args| async move {
        let options = args.into_iter().next().unwrap_or(Value::Null);
        let mut dialog = app.dialog().file();
        if let Some(title) = options["title"].as_str() {
            dialog = dialog.set_title(title);
        }
        if let Some(path) = options["defaultPath"].as_str().filter(|path| !path.is_empty()) {
            dialog = dialog.set_directory(path);
        }
        let folder = options["properties"]
            .as_array()
            .is_some_and(|properties| properties.iter().any(|p| p == "openDirectory"));
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
        let answer = move |path| {
            let _ = tx.send(path);
        };
        #[cfg(desktop)]
        if folder {
            dialog.pick_folder(answer);
        } else {
            dialog.pick_file(answer);
        }
        // No folder picker on mobile; the downloader hides its folder items
        #[cfg(mobile)]
        {
            let _ = folder;
            dialog.pick_file(answer);
        }
        let path = rx.await.ok().flatten().and_then(|path| path.into_path().ok());
        Ok(match path {
            Some(path) => json!({ "canceled": false, "filePaths": [path] }),
            None => json!({ "canceled": true, "filePaths": [] }),
        })
    });
}
