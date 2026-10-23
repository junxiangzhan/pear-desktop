//! Main window behaviour from `src/index.ts#createMainWindow`: title,
//! close-to-tray, always-on-top, start at login, custom window title.

use std::sync::Mutex;

use serde_json::json;
use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};

use crate::{config::Config, ipc::MAIN_WEBVIEW, menu::label, prompt};

/// Options every webview must share. WebView2 runs all webviews of a profile
/// in one browser environment and refuses to create one whose environment
/// options differ (HRESULT 0x8007139F), so the main window and prompt windows
/// go through this.
pub fn shared_webview_options<'a, R: tauri::Runtime, M: Manager<R>>(
    builder: tauri::WebviewWindowBuilder<'a, R, M>,
) -> tauri::WebviewWindowBuilder<'a, R, M> {
    let builder = builder.use_https_scheme(true);
    // Diagnostic switch: expose the Chrome DevTools Protocol on this port.
    // Setting browser args replaces wry's defaults, so they are repeated here.
    #[cfg(windows)]
    if let Ok(port) = std::env::var("PEAR_POC_DEBUG_PORT") {
        return builder.additional_browser_args(&format!(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port={port}"
        ));
    }
    builder
}

/// The last document title, so a cleared custom title can fall back to it.
#[derive(Default)]
pub struct PageTitle(Mutex<String>);

/// The page title, unless `options.customWindowTitle` overrides it
/// (Electron's `page-title-updated` handler). `page_title` updates the
/// remembered document title when given.
pub fn apply_title(window: &WebviewWindow, page_title: Option<String>) {
    let app = window.app_handle();
    let mut remembered = app.state::<PageTitle>().0.lock().unwrap().clone();
    if let Some(page_title) = page_title {
        remembered = page_title.clone();
        *app.state::<PageTitle>().0.lock().unwrap() = page_title;
    }
    let custom = app.state::<Config>().get_str("options.customWindowTitle");
    let title = match (custom.is_empty(), remembered.is_empty()) {
        (false, _) => custom,
        (true, false) => remembered,
        (true, true) => "Pear Desktop".to_owned(),
    };
    let _ = window.set_title(&title);
}

/// Hide instead of close while the tray is on (the `close` handler in
/// src/index.ts), so playback keeps going from the tray.
pub fn install_close_to_tray(window: &WebviewWindow) {
    let app = window.app_handle().clone();
    let window_for_event = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            let to_tray = app.state::<Config>().get_bool("options.tray");
            log::info!("main window close requested (to tray: {to_tray})");
            if to_tray {
                api.prevent_close();
                if let Err(error) = window_for_event.hide() {
                    log::error!("hiding the window failed: {error}");
                }
            }
        }
    });
}

pub fn set_start_at_login(app: &AppHandle, enabled: bool) {
    #[cfg(desktop)]
    {
        use tauri_plugin_autostart::ManagerExt;
        let launcher = app.autolaunch();
        let result = if enabled { launcher.enable() } else { launcher.disable() };
        if let Err(error) = result {
            log::error!("updating start at login failed: {error}");
        }
    }
    #[cfg(not(desktop))]
    let _ = (app, enabled);
}

/// The "Custom window title" menu item.
pub fn prompt_custom_title(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let key = "main.menu.options.submenu.visual-tweaks.submenu.custom-window-title";
        let current = app.state::<Config>().get_str("options.customWindowTitle");
        let options = json!({
            "title": label(&app, &format!("{key}.label"), "Custom window title"),
            "label": label(&app, &format!("{key}.prompt.label"), "Window title"),
            "value": current,
            "type": "input",
            "inputAttrs": { "type": "text", "placeholder": label(&app, &format!("{key}.prompt.placeholder"), "") },
        });
        let value = match prompt::open(&app, options).await {
            Ok(value) => value,
            Err(error) => {
                log::error!("custom window title prompt failed: {error}");
                return;
            }
        };
        let Some(title) = value.as_str() else { return }; // cancelled
        app.state::<Config>().set(&app, "options.customWindowTitle", json!(title));
        if let Some(window) = app.get_webview_window(MAIN_WEBVIEW) {
            apply_title(&window, None);
        }
    });
}

#[tauri::command]
pub fn window_set_always_on_top(app: AppHandle, flag: bool) -> Result<(), String> {
    let window = app
        .get_webview_window(MAIN_WEBVIEW)
        .ok_or("main window is gone")?;
    window.set_always_on_top(flag).map_err(|e| e.to_string())
}
