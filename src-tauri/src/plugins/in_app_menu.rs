//! Port of `src/plugins/in-app-menu/main.ts`: the title bar the renderer
//! draws gets the app menu as Electron `Menu` JSON (built by `menu.rs`) and
//! drives the frameless window. Whether the window is frameless is decided at
//! startup (`restartNeeded`), see `lib.rs`. The backtick toggle and window
//! dragging are handled in the page (`src/inject/plugin-glue.ts`).
//!
//! On Android it is the only menu; there is no window to manage, so the
//! window channels answer as for a window that is never maximized.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use serde_json::Value;
#[cfg(desktop)]
use serde_json::json;
use tauri::{AppHandle, Manager};
#[cfg(desktop)]
use tauri::WindowEvent;

use super::Plugin;
#[cfg(desktop)]
use crate::ipc::{send_to_page, MAIN_WEBVIEW};
use crate::{
    ipc::{arg, Ipc},
    menu,
};

pub struct InAppMenu {
    /// Gates the window-event handler of the current run (it can't be removed)
    listening: Mutex<Arc<AtomicBool>>,
}

impl InAppMenu {
    pub fn new() -> Self {
        Self { listening: Mutex::new(Arc::new(AtomicBool::new(false))) }
    }
}

/// Whether in-app-menu draws the menu: its stored setting, or its default
/// (on everywhere but macOS and Linux, like `defaultInAppMenuConfig`).
#[cfg(desktop)]
pub fn enabled_at_startup(config: &crate::config::Config) -> bool {
    if crate::config::unavailable_plugins().contains("in-app-menu") {
        return false;
    }
    config
        .get("plugins.in-app-menu.enabled")
        .as_bool()
        .unwrap_or(cfg!(not(any(target_os = "macos", target_os = "linux"))))
}

#[cfg(desktop)]
fn window_call(app: &AppHandle, action: impl FnOnce(&tauri::WebviewWindow) -> tauri::Result<()>) -> Result<Value, String> {
    let window = app.get_webview_window(MAIN_WEBVIEW).ok_or("main window is gone")?;
    action(&window).map_err(|e| e.to_string())?;
    Ok(Value::Null)
}

impl Plugin for InAppMenu {
    fn id(&self) -> &'static str {
        "in-app-menu"
    }

    fn start(&self, app: &AppHandle, _config: &Value) {
        let ipc = app.state::<Ipc>();
        let id = self.id();
        ipc.handle(id, "get-menu", |app, _| async move { Ok(menu::electron_menu(&app)) });
        ipc.handle(id, "get-menu-by-id", |app, args| async move {
            let command_id: u64 = arg(&args, 0)?;
            Ok(menu::electron_item(&app, command_id))
        });
        ipc.handle(id, "peard:menu-event", |app, args| async move {
            let command_id: u64 = arg(&args, 0)?;
            // Menu items touch the window; run them on the main thread
            let handle = app.clone();
            app.run_on_main_thread(move || menu::click_by_command_id(&handle, command_id))
                .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        });
        // The menu has no item icons on Tauri
        ipc.handle(id, "image-path-to-data-url", |_, _| async move { Ok(Value::Null) });

        let listening = Arc::new(AtomicBool::new(true));
        *self.listening.lock().unwrap() = listening.clone();
        #[cfg(desktop)]
        Self::handle_window(app, &ipc, listening);
        #[cfg(mobile)]
        {
            let _ = listening;
            ipc.handle(id, "window-is-maximized", |_, _| async move { Ok(Value::Bool(false)) });
        }
    }

    fn stop(&self, _app: &AppHandle) {
        self.listening.lock().unwrap().store(false, Ordering::Relaxed);
    }
}

#[cfg(desktop)]
impl InAppMenu {
    /// The frameless window's controls and its maximize state.
    fn handle_window(app: &AppHandle, ipc: &Ipc, listening: Arc<AtomicBool>) {
        let id = "in-app-menu";
        ipc.handle(id, "window-is-maximized", |app, _| async move {
            let window = app.get_webview_window(MAIN_WEBVIEW).ok_or("main window is gone")?;
            Ok(json!(window.is_maximized().unwrap_or(false)))
        });
        ipc.handle(id, "window-close", |app, _| async move { window_call(&app, |w| w.close()) });
        ipc.handle(id, "window-minimize", |app, _| async move { window_call(&app, |w| w.minimize()) });
        ipc.handle(id, "window-maximize", |app, _| async move { window_call(&app, |w| w.maximize()) });
        ipc.handle(id, "window-unmaximize", |app, _| async move { window_call(&app, |w| w.unmaximize()) });

        let Some(window) = app.get_webview_window(MAIN_WEBVIEW) else { return };
        let maximized = Arc::new(AtomicBool::new(window.is_maximized().unwrap_or(false)));
        let (app, events_window) = (app.clone(), window.clone());
        window.on_window_event(move |event| {
            if !listening.load(Ordering::Relaxed) {
                return;
            }
            match event {
                WindowEvent::CloseRequested { .. } => {
                    send_to_page(&app, "close-all-in-app-menu-panel", &[]);
                }
                WindowEvent::Resized(_) => {
                    let now = events_window.is_maximized().unwrap_or(false);
                    if maximized.swap(now, Ordering::Relaxed) != now {
                        let channel = if now { "window-maximize" } else { "window-unmaximize" };
                        send_to_page(&app, channel, &[]);
                    }
                }
                _ => {}
            }
        });
    }
}
