//! What the core menu items do when clicked (the `click` handlers of
//! `src/menu.ts`).

use super::*;

#[cfg(desktop)]
fn zoom(app: &AppHandle, change: impl FnOnce(f64) -> f64) {
    let state = app.state::<MenuState>();
    let mut zoom = state.zoom.lock().unwrap();
    if *zoom == 0.0 {
        *zoom = 1.0;
    }
    *zoom = change(*zoom).clamp(0.25, 5.0);
    if let Some(window) = app.get_webview_window(MAIN_WEBVIEW) {
        let _ = window.set_zoom(*zoom);
    }
}

/// Runs a menu item by its id, after its check state was updated (natively
/// by a click on the menu bar, or by `click_by_command_id`).
pub(super) fn dispatch(app: &AppHandle, id: &str) {
    let id = id.to_owned();
    let checked = settle_check(app, &id);
    let config = app.state::<Config>();
    // The window items (always on top, devtools, full screen) are desktop-only
    #[cfg(desktop)]
    let window = app.get_webview_window(MAIN_WEBVIEW);

    if let Some(command_id) = id.strip_prefix("cmd:") {
        let command_id: u64 = command_id.parse().unwrap_or_default();
        ipc::send_to_page(app, "peard:menu-event", &[json!(command_id), json!(checked)]);
        return;
    }
    if let Some(plugin) = id.strip_prefix("plugin:") {
        if always_on(plugin) {
            return;
        }
        // `config.plugins.enable/disable` → setMenuOptions
        let mut value = config.get(&format!("plugins.{plugin}"));
        if !value.is_object() {
            value = json!({});
        }
        value["enabled"] = json!(checked.unwrap_or(false));
        set_menu_option(app, &format!("plugins.{plugin}"), value);
        return;
    }
    if let Some(option) = id.strip_prefix("opt:") {
        let checked = checked.unwrap_or(false);
        set_menu_option(app, &format!("options.{option}"), json!(checked));
        match option {
            #[cfg(desktop)]
            "alwaysOnTop" => {
                if let Some(window) = &window {
                    let _ = window.set_always_on_top(checked);
                }
            }
            "startAtLogin" => crate::window::set_start_at_login(app, checked),
            #[cfg(target_os = "android")]
            "fullscreen" => crate::android_app::set_fullscreen(app, checked),
            "trayClickPlayPause" => {}
            _ => {}
        }
        return;
    }
    if let Some(rest) = id.strip_prefix("radio:") {
        let (group, value) = rest.split_once(':').unwrap_or((rest, ""));
        match group {
            "startingPage" => config.set(app, "options.startingPage", json!(value)),
            "likeButtons" => config.set(app, "options.likeButtons", json!(value)),
            "tray" => {
                set_menu_option(app, "options.tray", json!(value != "disabled"));
                set_menu_option(app, "options.appVisible", json!(value != "hide"));
                #[cfg(desktop)]
                crate::tray::sync(app);
            }
            "language" => {
                config.set(app, "options.language", json!(value));
                // The renderer reads the language at load
                page::reload(app);
            }
            _ => {}
        }
        return;
    }

    match id.as_str() {
        "act:customWindowTitle" => crate::window::prompt_custom_title(app),
        "act:setProxy" => crate::proxy::prompt_proxy(app),
        "act:editConfig" => {
            if let Err(error) = crate::config::config_edit(app.clone()) {
                log::error!("opening config.json failed: {error}");
            }
        }
        #[cfg(desktop)]
        "act:toggleDevTools" => {
            if let Some(window) = &window {
                if window.is_devtools_open() {
                    window.close_devtools();
                } else {
                    window.open_devtools();
                }
            }
        }
        "act:reload" | "act:forceReload" => page::reload(app),
        #[cfg(desktop)]
        "act:zoomIn" => zoom(app, |z| z + 0.1),
        #[cfg(desktop)]
        "act:zoomOut" => zoom(app, |z| z - 0.1),
        #[cfg(desktop)]
        "act:resetZoom" => zoom(app, |_| 1.0),
        #[cfg(desktop)]
        "act:toggleFullscreen" => {
            if let Some(window) = &window {
                let fullscreen = window.is_fullscreen().unwrap_or(false);
                let _ = window.set_fullscreen(!fullscreen);
            }
        }
        "act:goBack" => page::eval(app, "history.back()"),
        "act:goForward" => page::eval(app, "history.forward()"),
        "act:copyUrl" => {
            // Off the main thread: on Android the URL may come from Kotlin
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Some(url) = page::url(&app) {
                    use tauri_plugin_clipboard_manager::ClipboardExt;
                    let _ = app.clipboard().write_text(url);
                }
            });
        }
        "act:restart" => app_controls::restart(app),
        "act:quit" => app.exit(0),
        "act:about" => crate::dialogs::about(app),
        // The tray's own menu handler takes these (tray.rs); Tauri sends
        // every menu event here too
        id if id.starts_with("tray:") => {}
        _ => log::warn!("unhandled menu item '{id}'"),
    }
}
