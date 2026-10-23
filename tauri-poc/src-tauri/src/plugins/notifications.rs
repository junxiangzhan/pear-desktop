//! Port of the basic mode of `src/plugins/notifications`: a system
//! notification per new song. The interactive Windows toasts (with playback
//! buttons) are not ported yet; with `interactive` on, this falls back to the
//! basic notification.

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
}

pub struct Notifications {
    state: Arc<Mutex<State>>,
}

impl Notifications {
    pub fn new() -> Self {
        Self { state: Arc::default() }
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

impl Plugin for Notifications {
    fn id(&self) -> &'static str {
        "notifications"
    }

    fn start(&self, app: &AppHandle, config: &Value) {
        if config["interactive"].as_bool().unwrap_or(false) {
            log::info!("notifications: interactive toasts are not ported yet, using basic notifications");
        }
        self.state.lock().unwrap().config = config.clone();
        let state = self.state.clone();
        app.state::<SongInfoProvider>().register(self.id(), move |app, song, event| {
            if event == SongInfoEvent::TimeChanged || song.is_paused.unwrap_or(false) {
                return;
            }
            let mut state = state.lock().unwrap();
            let unpause = state.config["unpauseNotification"].as_bool().unwrap_or(false);
            if song.url == state.current_url && !unpause {
                return;
            }
            state.current_url = song.url.clone();
            tauri::async_runtime::spawn(notify(app.clone(), song.clone()));
        });
    }

    fn on_config_change(&self, _app: &AppHandle, config: &Value) {
        self.state.lock().unwrap().config = config.clone();
    }
}
