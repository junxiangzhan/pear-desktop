//! Port of `src/tray.ts`: shown while `options.tray` is on, with playback
//! controls; the icon and tooltip follow the song info.

use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Wry,
};

use crate::{
    app_controls,
    config::Config,
    ipc::MAIN_WEBVIEW,
    menu::label,
    song_controls,
    song_info::{SongInfo, SongInfoProvider},
};

const TRAY_ID: &str = "main";

pub type ClickHandler = std::sync::Arc<dyn Fn(&AppHandle) + Send + Sync>;

/// `setTrayOnClick` / `setTrayOnDoubleClick` for notifications' tray
/// controls: while set, a click runs it and a double click shows or hides
/// the window.
static CLICK_OVERRIDE: std::sync::Mutex<Option<ClickHandler>> = std::sync::Mutex::new(None);

#[cfg_attr(not(windows), allow(dead_code))]
pub fn set_click_override(handler: Option<ClickHandler>) {
    *CLICK_OVERRIDE.lock().unwrap() = handler;
}

fn icon(paused: bool) -> Image<'static> {
    let bytes: &'static [u8] = if paused {
        include_bytes!("../../assets/tray-paused.png")
    } else {
        include_bytes!("../../assets/tray.png")
    };
    Image::from_bytes(bytes).expect("bundled tray icons are valid PNGs")
}

fn tooltip(app: &AppHandle, song: Option<&SongInfo>) -> String {
    match song {
        Some(song) => label(app, "main.tray.tooltip.with-song-info", "Pear Tauri: {artist} - {title}")
            .replace("{artist}", &song.artist)
            .replace("{title}", &song.title),
        None => label(app, "main.tray.tooltip.default", "Pear Tauri"),
    }
}

fn toggle_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WEBVIEW) else { return };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn create(app: &AppHandle) -> tauri::Result<()> {
    let items = [
        MenuItem::with_id(app, "tray:play-pause", label(app, "main.tray.play-pause", "Play/Pause"), true, None::<&str>)?,
        MenuItem::with_id(app, "tray:next", label(app, "main.tray.next", "Next"), true, None::<&str>)?,
        MenuItem::with_id(app, "tray:previous", label(app, "main.tray.previous", "Previous"), true, None::<&str>)?,
        MenuItem::with_id(app, "tray:show", label(app, "main.tray.show", "Show"), true, None::<&str>)?,
        MenuItem::with_id(app, "tray:restart", label(app, "main.tray.restart", "Restart"), true, None::<&str>)?,
        MenuItem::with_id(app, "tray:quit", label(app, "main.tray.quit", "Quit"), true, None::<&str>)?,
    ];
    let separator = PredefinedMenuItem::separator(app)?;
    let menu: Menu<Wry> = Menu::with_items(
        app,
        &[&items[0], &items[1], &items[2], &separator, &items[3], &items[4], &items[5]],
    )?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(false))
        .tooltip(tooltip(app, None))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray:play-pause" => song_controls::play_pause(app),
            "tray:next" => song_controls::next(app),
            "tray:previous" => song_controls::previous(app),
            "tray:show" => {
                if let Some(window) = app.get_webview_window(MAIN_WEBVIEW) {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "tray:restart" => app_controls::restart(app),
            "tray:quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            let click_override = CLICK_OVERRIDE.lock().unwrap().clone();
            match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } => {
                    if let Some(handler) = click_override {
                        handler(app);
                    } else if app.state::<Config>().get_bool("options.trayClickPlayPause") {
                        song_controls::play_pause(app);
                    } else {
                        toggle_window(app);
                    }
                }
                TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } if click_override.is_some() => {
                    toggle_window(app);
                }
                _ => {}
            }
        })
        .build(app)?;
    Ok(())
}

/// Shows or removes the tray to match `options.tray` (Electron needed a
/// restart for this; here it applies immediately).
pub fn sync(app: &AppHandle) {
    let wanted = app.state::<Config>().get_bool("options.tray");
    let exists = app.tray_by_id(TRAY_ID).is_some();
    if wanted && !exists {
        match create(app) {
            Ok(()) => log::info!("tray: shown"),
            Err(error) => log::error!("creating the tray failed: {error}"),
        }
    } else if !wanted && exists {
        app.remove_tray_by_id(TRAY_ID);
        log::info!("tray: removed");
    }
}

/// Recreates the tray so it picks up translated labels.
pub fn relabel(app: &AppHandle) {
    if app.remove_tray_by_id(TRAY_ID).is_some() {
        sync(app);
    }
}

/// Like the `registerCallback` in `setUpTray`.
///
/// Song-info callbacks run on async worker threads. Tray setters called from
/// there block until the main thread runs them, which can deadlock, so the
/// update is queued on the main thread without waiting.
pub fn register(app: &AppHandle) {
    app.state::<SongInfoProvider>().register("core", |app, song, _| {
        let paused = song.is_paused.unwrap_or(false);
        let tooltip = tooltip(app, Some(song));
        let handle = app.clone();
        let queued = app.run_on_main_thread(move || {
            let Some(tray) = handle.tray_by_id(TRAY_ID) else { return };
            let _ = tray.set_icon(Some(icon(paused)));
            let _ = tray.set_tooltip(Some(tooltip));
        });
        if let Err(error) = queued {
            log::warn!("updating the tray failed: {error}");
        }
    });
}
