//! Replacement for `custom-electron-prompt`: a small window
//! (`placeholder/prompt.html`) that shows one prompt and returns its value,
//! or `null` when closed or cancelled. Supports the types the plugins use:
//! input, select, counter, keybind and multiInput.
// Mobile has no prompt windows; only the commands' signatures remain there
#![cfg_attr(mobile, allow(dead_code, unused_imports))]

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use serde_json::Value;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tokio::sync::oneshot;

use crate::ipc::MAIN_WEBVIEW;

#[derive(Default)]
pub struct Prompts {
    next_id: AtomicU64,
    pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
}

impl Prompts {
    fn resolve(&self, label: &str, value: Value) {
        if let Some(sender) = self.pending.lock().unwrap().remove(label) {
            let _ = sender.send(value);
        }
    }
}

/// Default sizes close to `custom-electron-prompt`'s, grown per field.
fn default_size(options: &Value) -> (f64, f64) {
    let fields = |key: &str| options[key].as_array().map_or(1, Vec::len) as f64;
    let height = match options["type"].as_str().unwrap_or("input") {
        "keybind" => 110.0 + 62.0 * fields("keybindOptions"),
        "multiInput" => 110.0 + 70.0 * fields("multiInputOptions"),
        _ => 170.0,
    };
    (
        options["width"].as_f64().unwrap_or(400.0),
        options["height"].as_f64().unwrap_or(height),
    )
}

/// Android and iOS have the main webview only.
#[cfg(mobile)]
pub async fn open(_app: &AppHandle, _options: Value) -> Result<Value, String> {
    Err("prompt windows are not available on mobile".into())
}

/// Opens a prompt and waits for its result (`null` if dismissed).
#[cfg(desktop)]
pub async fn open(app: &AppHandle, options: Value) -> Result<Value, String> {
    let prompts = app.state::<Prompts>();
    let label = format!("prompt-{}", prompts.next_id.fetch_add(1, Ordering::Relaxed));
    let (sender, receiver) = oneshot::channel();
    prompts.pending.lock().unwrap().insert(label.clone(), sender);

    let (width, height) = default_size(&options);
    let title = options["title"].as_str().unwrap_or("Pear Desktop").to_owned();
    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("prompt.html".into()));
    let mut builder = crate::window::shared_webview_options(builder)
        .title(title)
        .inner_size(width, height)
        .resizable(options["resizable"].as_bool().unwrap_or(false))
        .minimizable(false)
        .maximizable(false)
        .always_on_top(options["alwaysOnTop"].as_bool().unwrap_or(true))
        .skip_taskbar(true)
        .center()
        .initialization_script(format!("window.__PROMPT_OPTIONS__ = {};", options));
    if let Some(main) = app.get_webview_window(MAIN_WEBVIEW) {
        builder = builder.parent(&main).map_err(|e| e.to_string())?;
    }
    let window = builder.build().map_err(|e| {
        prompts.pending.lock().unwrap().remove(&label);
        format!("opening the prompt window failed: {e}")
    })?;
    log::info!("prompt {label} opened");

    // Closing the window counts as cancel
    let app_for_close = app.clone();
    let closed_label = label.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            app_for_close.state::<Prompts>().resolve(&closed_label, Value::Null);
        }
    });

    Ok(receiver.await.unwrap_or(Value::Null))
}

#[tauri::command]
pub async fn prompt_open(app: AppHandle, options: Value) -> Result<Value, String> {
    open(&app, options).await
}

/// Called by `prompt.html` with the result (`null` for cancel).
#[tauri::command]
pub fn prompt_submit(app: AppHandle, window: tauri::WebviewWindow, value: Value) {
    app.state::<Prompts>().resolve(window.label(), value);
    let _ = window.close();
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn size_grows_with_fields() {
        let keybind = json!({ "type": "keybind", "keybindOptions": [{}, {}] });
        assert_eq!(default_size(&keybind), (400.0, 234.0));
        assert_eq!(default_size(&json!({ "type": "input", "width": 500 })), (500.0, 170.0));
    }
}
