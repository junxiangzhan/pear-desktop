//! Port of `src/plugins/notifications`: a system notification per new song.
//! On Windows with `interactive` on, it is a toast with playback buttons in
//! the chosen `toastStyle` (`toast.rs`, from interactive.ts), and
//! `trayControls` makes a tray click show or hide it. Elsewhere, and with
//! `interactive` off, it is the basic notification. Unlike Electron, the
//! mode follows the setting without a restart.

#[cfg(windows)]
mod toast;

use std::sync::{Arc, Mutex};

use serde_json::Value;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use super::Plugin;
use crate::song_info::{SongInfo, SongInfoEvent, SongInfoProvider};

#[derive(Default)]
struct State {
    config: Value,
    current_url: Option<String>,
    /// The last song info, for the tray and the refresh after play/pause
    #[cfg_attr(not(windows), allow(dead_code))]
    saved_song: Option<SongInfo>,
    #[cfg_attr(not(windows), allow(dead_code))]
    current_seconds: f64,
}

pub struct Notifications {
    state: Arc<Mutex<State>>,
    #[cfg(windows)]
    toaster: Arc<toast::Toaster>,
}

impl Notifications {
    pub fn new() -> Self {
        Self {
            state: Arc::default(),
            #[cfg(windows)]
            toaster: Arc::default(),
        }
    }
}

/// The cover, saved where the notification can load it from disk.
async fn cover_file(app: &AppHandle, song: &SongInfo) -> Option<std::path::PathBuf> {
    let src = song.image_src.as_deref().filter(|s| !s.is_empty())?;
    let bytes = crate::net::client().get(src).send().await.ok()?.bytes().await.ok()?;
    let path = app.path().app_cache_dir().ok()?.join("notification-cover.jpg");
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

async fn notify(app: AppHandle, song: SongInfo) {
    let cover = cover_file(&app, &song).await;
    let title = if song.title.is_empty() { "Playing".to_owned() } else { song.title.clone() };
    let mut builder = app.notification().builder().title(title).body(song.artist.clone());
    if let Some(cover) = cover {
        builder = builder.icon(cover.to_string_lossy());
    }
    if let Err(error) = builder.show() {
        log::warn!("notifications: showing failed: {error}");
    }
}

#[cfg(windows)]
fn interactive(config: &Value) -> bool {
    config["interactive"].as_bool().unwrap_or(true)
}

/// The toast with the current settings; its buttons act like Electron's
/// protocol handler (`changeProtocolHandler` in interactive.ts).
#[cfg(windows)]
fn show_toast(app: &AppHandle, state: &Arc<Mutex<State>>, toaster: &Arc<toast::Toaster>, song: SongInfo) {
    let style = state.lock().unwrap().config["toastStyle"].as_u64().unwrap_or(1);
    let on_action: toast::OnAction = {
        let state = state.clone();
        let toaster = toaster.clone();
        Arc::new(move |app: &AppHandle, action: &str| {
            use crate::song_controls;
            match action {
                "previous" => song_controls::previous(app),
                "next" => song_controls::next(app),
                "play" => song_controls::play(app),
                "pause" => song_controls::pause(app),
                // A click on the toast itself
                _ => {
                    if let Some(window) = app.get_webview_window(crate::ipc::MAIN_WEBVIEW) {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                    return;
                }
            }
            let (refresh, song) = {
                let state = state.lock().unwrap();
                let unpause = state.config["unpauseNotification"].as_bool().unwrap_or(false);
                let refresh = state.config["refreshOnPlayPause"].as_bool().unwrap_or(false)
                    && (action == "pause" || (action == "play" && !unpause));
                let song = state.saved_song.clone().map(|song| SongInfo {
                    is_paused: Some(action == "pause"),
                    elapsed_seconds: Some(state.current_seconds),
                    ..song
                });
                (refresh, song)
            };
            if let (true, Some(song)) = (refresh, song) {
                let (app, state, toaster) = (app.clone(), state.clone(), toaster.clone());
                tauri::async_runtime::spawn(async move { show_toast(&app, &state, &toaster, song) });
            }
        })
    };
    let (app, toaster) = (app.clone(), toaster.clone());
    tauri::async_runtime::spawn(async move { toaster.show(&app, song, style, on_action).await });
}

/// `trayControls`: a tray click hides the toast or shows it again.
#[cfg(windows)]
fn sync_tray(state: &Arc<Mutex<State>>, toaster: &Arc<toast::Toaster>) {
    let config = state.lock().unwrap().config.clone();
    if !(interactive(&config) && config["trayControls"].as_bool().unwrap_or(true)) {
        crate::tray::set_click_override(None);
        return;
    }
    let (state, toaster) = (state.clone(), toaster.clone());
    crate::tray::set_click_override(Some(Arc::new(move |app: &AppHandle| {
        if toaster.is_shown() {
            let toaster = toaster.clone();
            tauri::async_runtime::spawn(async move { toaster.hide().await });
            return;
        }
        let song = {
            let state = state.lock().unwrap();
            state.saved_song.clone().map(|song| SongInfo {
                elapsed_seconds: Some(state.current_seconds),
                ..song
            })
        };
        if let Some(song) = song {
            show_toast(app, &state, &toaster, song);
        }
    })));
}

impl Plugin for Notifications {
    fn id(&self) -> &'static str {
        "notifications"
    }

    fn start(&self, app: &AppHandle, config: &Value) {
        self.state.lock().unwrap().config = config.clone();
        let state = self.state.clone();
        #[cfg(windows)]
        let toaster = self.toaster.clone();
        app.state::<SongInfoProvider>().register(self.id(), move |app, song, event| {
            let mut guard = state.lock().unwrap();
            if event == SongInfoEvent::TimeChanged {
                guard.current_seconds = song.elapsed_seconds.unwrap_or(0.0);
                return;
            }
            if song.artist.is_empty() && song.title.is_empty() {
                return;
            }
            guard.saved_song = Some(song.clone());
            let unpause = guard.config["unpauseNotification"].as_bool().unwrap_or(false);
            if song.is_paused.unwrap_or(false) || (song.url == guard.current_url && !unpause) {
                return;
            }
            guard.current_url = song.url.clone();
            #[cfg(windows)]
            if interactive(&guard.config) {
                drop(guard);
                show_toast(app, &state, &toaster, song.clone());
                return;
            }
            drop(guard);
            tauri::async_runtime::spawn(notify(app.clone(), song.clone()));
        });
        #[cfg(windows)]
        {
            super::request_player_listeners(app, self.id(), &["time-changed"]);
            sync_tray(&self.state, &self.toaster);
        }
    }

    fn stop(&self, _app: &AppHandle) {
        #[cfg(windows)]
        {
            crate::tray::set_click_override(None);
            let toaster = self.toaster.clone();
            tauri::async_runtime::spawn(async move { toaster.hide().await });
        }
    }

    fn on_config_change(&self, _app: &AppHandle, config: &Value) {
        self.state.lock().unwrap().config = config.clone();
        #[cfg(windows)]
        sync_tray(&self.state, &self.toaster);
    }
}
