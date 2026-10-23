//! Port of `src/plugins/picture-in-picture/main.ts`: shrinks the main window
//! into a small always-on-top player and back. Blocking F / handling Esc
//! while in PiP happens in the page (`inject/plugin-glue.ts`), since Rust
//! cannot intercept the webview's key events.
//!
//! Window calls run with no lock held: moving or resizing the window fires
//! its window events synchronously on the same thread, and the event handler
//! below reads this plugin's state.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use serde_json::{json, Value};
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize, WebviewWindow,
    WindowEvent,
};

use super::Plugin;
use crate::{
    config::Config,
    ipc::{send_to_page, Ipc, MAIN_WEBVIEW},
};

#[derive(Default, Clone, Copy)]
struct Saved {
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    fullscreen: bool,
    maximized: bool,
}

#[derive(Default)]
struct Shared {
    config: Mutex<Value>,
    saved: Mutex<Saved>,
    in_pip: AtomicBool,
    /// Set while this plugin itself moves/resizes the window
    adjusting: AtomicBool,
    /// Gates the window-event handler of the current run (it can't be removed)
    listening: Mutex<Arc<AtomicBool>>,
}

pub struct PictureInPicture {
    shared: Arc<Shared>,
}

impl PictureInPicture {
    pub fn new() -> Self {
        Self { shared: Arc::default() }
    }
}

/// `[a, b]` from the config, in logical pixels like Electron's DIPs.
fn pair(value: &Value, default: (f64, f64)) -> (f64, f64) {
    match (value[0].as_f64(), value[1].as_f64()) {
        (Some(a), Some(b)) => (a, b),
        _ => default,
    }
}

fn set_plugin_options(app: &AppHandle, options: Value) {
    let config = app.state::<Config>();
    let default = config.plugin_default("picture-in-picture");
    config.set_partial(app, "plugins.picture-in-picture", &options, Some(&default));
}

/// Puts the window at the PiP position and size from the config.
fn apply_pip_geometry(window: &WebviewWindow, config: &Value) {
    let (x, y) = if config["savePosition"].as_bool().unwrap_or(true) {
        pair(&config["pip-position"], (10.0, 10.0))
    } else {
        (10.0, 10.0)
    };
    let (w, h) = if config["saveSize"].as_bool().unwrap_or(false) {
        pair(&config["pip-size"], (450.0, 275.0))
    } else {
        (450.0, 275.0)
    };
    let _ = window.set_position(LogicalPosition::new(x, y));
    let _ = window.set_size(LogicalSize::new(w, h));
}

fn enter(app: &AppHandle, window: &WebviewWindow, shared: &Shared) {
    let config = shared.config.lock().unwrap().clone();
    let saved = Saved {
        fullscreen: window.is_fullscreen().unwrap_or(false),
        maximized: window.is_maximized().unwrap_or(false),
        position: window.outer_position().unwrap_or_default(),
        // Inner size: `set_size` (used to restore it) sets the inner size
        size: window.inner_size().unwrap_or_default(),
    };
    *shared.saved.lock().unwrap() = saved;

    if saved.fullscreen {
        let _ = window.set_fullscreen(false);
    }
    if saved.maximized {
        let _ = window.unmaximize();
    }
    let _ = window.set_maximizable(false);
    let _ = window.set_visible_on_all_workspaces(true);
    if config["alwaysOnTop"].as_bool().unwrap_or(true) {
        let _ = window.set_always_on_top(true);
    }
    apply_pip_geometry(window, &config);
    let _ = app;
}

fn leave(app: &AppHandle, window: &WebviewWindow, shared: &Shared) {
    let saved = *shared.saved.lock().unwrap();
    let _ = window.set_maximizable(true);
    let _ = window.set_visible_on_all_workspaces(false);
    let _ = window.set_always_on_top(app.state::<Config>().get_bool("options.alwaysOnTop"));
    let _ = window.set_position(saved.position);
    let _ = window.set_size(saved.size);
    if saved.fullscreen {
        let _ = window.set_fullscreen(true);
    }
    if saved.maximized {
        let _ = window.maximize();
    }
}

fn toggle(app: &AppHandle, window: &WebviewWindow, shared: &Shared) {
    // `in_pip` is only true while the window is the PiP player, so the moves
    // made while entering or leaving are not saved as the PiP position
    let in_pip = !shared.in_pip.load(Ordering::SeqCst);
    shared.adjusting.store(true, Ordering::SeqCst);
    if in_pip {
        enter(app, window, shared);
        shared.in_pip.store(true, Ordering::SeqCst);
    } else {
        shared.in_pip.store(false, Ordering::SeqCst);
        leave(app, window, shared);
    }
    shared.adjusting.store(false, Ordering::SeqCst);
    send_to_page(app, "peard:pip-toggle", &[json!(in_pip)]);
    set_plugin_options(app, json!({ "isInPiP": in_pip }));
}

impl Plugin for PictureInPicture {
    fn id(&self) -> &'static str {
        "picture-in-picture"
    }

    fn start(&self, app: &AppHandle, config: &Value) {
        *self.shared.config.lock().unwrap() = config.clone();
        self.shared.in_pip.store(false, Ordering::SeqCst);
        set_plugin_options(app, json!({ "isInPiP": false }));
        let Some(window) = app.get_webview_window(MAIN_WEBVIEW) else { return };

        let (shared, toggle_window) = (self.shared.clone(), window.clone());
        app.state::<Ipc>().on(self.id(), "plugin:toggle-picture-in-picture", move |app, _| {
            toggle(app, &toggle_window, &shared);
        });

        // Remember where the user moves/resizes the PiP window (logical px)
        let listening = Arc::new(AtomicBool::new(true));
        *self.shared.listening.lock().unwrap() = listening.clone();
        let (shared, app, events_window) = (self.shared.clone(), app.clone(), window.clone());
        window.on_window_event(move |event| {
            if !listening.load(Ordering::Relaxed)
                || !shared.in_pip.load(Ordering::SeqCst)
                || shared.adjusting.load(Ordering::SeqCst)
            {
                return;
            }
            // The PiP renderer clicks YTM's fullscreen button for its player
            // layout, and Tauri turns HTML fullscreen into window fullscreen.
            // Electron prevented that with `setFullScreenable(false)`; undo it.
            if matches!(event, WindowEvent::Resized(_)) && events_window.is_fullscreen().unwrap_or(false) {
                let config = shared.config.lock().unwrap().clone();
                shared.adjusting.store(true, Ordering::SeqCst);
                let _ = events_window.set_fullscreen(false);
                apply_pip_geometry(&events_window, &config);
                shared.adjusting.store(false, Ordering::SeqCst);
                return;
            }
            if shared.config.lock().unwrap()["useNativePiP"].as_bool().unwrap_or(true) {
                return;
            }
            let scale = events_window.scale_factor().unwrap_or(1.0);
            match event {
                WindowEvent::Moved(position) => {
                    let p = position.to_logical::<f64>(scale);
                    set_plugin_options(&app, json!({ "pip-position": [p.x.round(), p.y.round()] }));
                }
                WindowEvent::Resized(size) => {
                    let s = size.to_logical::<f64>(scale);
                    set_plugin_options(&app, json!({ "pip-size": [s.width.round(), s.height.round()] }));
                }
                _ => {}
            }
        });
    }

    fn stop(&self, _app: &AppHandle) {
        self.shared.listening.lock().unwrap().store(false, Ordering::Relaxed);
    }

    fn on_config_change(&self, _app: &AppHandle, config: &Value) {
        *self.shared.config.lock().unwrap() = config.clone();
    }
}
