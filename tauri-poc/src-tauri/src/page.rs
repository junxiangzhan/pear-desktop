//! Everything Rust sends to the page goes through here: events (`pear:ipc`,
//! `pear:config`), scripts and navigation. On Android the page runs in
//! GeckoView (see `gecko_bridge`), elsewhere in the main webview.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::ipc::MAIN_WEBVIEW;

/// Whether the page runs in GeckoView: on Android, unless MainActivity was
/// switched back to the WebView (`PEAR_ENGINE=webview`, set by Kotlin before
/// Rust starts).
pub fn in_gecko() -> bool {
    cfg!(target_os = "android") && std::env::var("PEAR_ENGINE").map_or(true, |engine| engine != "webview")
}

/// Like `emit_to(MAIN_WEBVIEW, event, payload)`.
pub fn emit(app: &AppHandle, event: &str, payload: impl Serialize + Clone) {
    #[cfg(target_os = "android")]
    if in_gecko() {
        return crate::gecko_bridge::emit(app, event, payload);
    }
    if let Err(error) = app.emit_to(MAIN_WEBVIEW, event, payload) {
        log::warn!("emitting '{event}' to the page failed: {error}");
    }
}

/// Runs a script in the page's main world.
pub fn eval(app: &AppHandle, script: &str) {
    #[cfg(target_os = "android")]
    if in_gecko() {
        return crate::gecko_bridge::eval(app, script);
    }
    if let Some(window) = app.get_webview_window(MAIN_WEBVIEW) {
        let _ = window.eval(script);
    }
}

/// Loads `url` in the page.
pub fn navigate(app: &AppHandle, url: &str) {
    if in_gecko() {
        return eval(app, &format!("location.assign({})", serde_json::json!(url)));
    }
    let Some(window) = app.get_webview_window(MAIN_WEBVIEW) else {
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

pub fn reload(app: &AppHandle) {
    if in_gecko() {
        return eval(app, "location.reload()");
    }
    if let Some(window) = app.get_webview_window(MAIN_WEBVIEW) {
        let _ = window.reload();
    }
}
