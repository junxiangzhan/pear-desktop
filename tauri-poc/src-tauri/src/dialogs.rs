//! Message boxes from `src/index.ts`, on `tauri-plugin-dialog`.

use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

/// The "About" menu item (Electron's `role: 'about'`).
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
