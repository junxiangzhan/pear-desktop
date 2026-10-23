//! Ports of the `lumiastream` and `tuna-obs` backends: both POST the current
//! song to a local streaming tool on every song-info update.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use super::Plugin;
use crate::{
    ipc::{send_to_page, Ipc},
    song_info::{SongInfo, SongInfoProvider},
};

fn millis(seconds: Option<f64>) -> Option<u64> {
    seconds.filter(|s| *s > 0.0).map(|s| (s * 1000.0).round() as u64)
}

/// Both plugins ask the page to report playback time.
fn request_time_updates(app: &AppHandle, owner: &str) {
    app.state::<Ipc>().on(owner, "peard:player-api-loaded", |app, _| {
        send_to_page(app, "peard:setup-time-changed-listener", &[]);
    });
}

fn has_song(song: &SongInfo) -> bool {
    !song.title.is_empty() || !song.artist.is_empty()
}

pub struct LumiaStream;

impl Plugin for LumiaStream {
    fn id(&self) -> &'static str {
        "lumiastream"
    }

    fn start(&self, app: &AppHandle, _config: &Value) {
        request_time_updates(app, self.id());
        app.state::<SongInfoProvider>().register(self.id(), |_, song, _| {
            if !has_song(song) {
                return;
            }
            // The original's `previousStatePaused` is a constant `null`, so it
            // always reports `switchSong`; kept as is.
            let data = json!({
                "origin": "youtubemusic",
                "eventType": "switchSong",
                "duration": millis(Some(song.song_duration)),
                "progress": millis(song.elapsed_seconds),
                "url": song.url,
                "videoId": song.video_id,
                "playlistId": song.playlist_id,
                "cover": song.image_src,
                "cover_url": song.image_src,
                "album_url": song.image_src,
                "title": song.title,
                "artists": [song.artist],
                "status": if song.is_paused.unwrap_or(false) { "stopped" } else { "playing" },
                "isPaused": song.is_paused,
                "album": song.album,
                "views": song.views,
            });
            tauri::async_runtime::spawn(async move {
                let result = crate::net::client()
                    .post("http://127.0.0.1:39231/api/media")
                    .json(&json!({ "token": "lsmedia_ytmsI7812", "data": data }))
                    .send()
                    .await;
                if let Err(error) = result {
                    log::debug!("lumiastream: webserver at port 39231 unreachable: {error}");
                }
            });
        });
    }
}

pub struct TunaObs {
    /// While OBS is unreachable, probe with OPTIONS instead of posting
    lite_mode: Arc<AtomicBool>,
}

impl TunaObs {
    pub fn new() -> Self {
        Self { lite_mode: Arc::new(AtomicBool::new(false)) }
    }
}

const TUNA_URL: &str = "http://127.0.0.1:1608/";

async fn post_to_tuna(lite_mode: Arc<AtomicBool>, data: Value) {
    let client = crate::net::client();
    if lite_mode.load(Ordering::Relaxed) {
        if client.request(reqwest::Method::OPTIONS, TUNA_URL).send().await.is_err() {
            return;
        }
        log::debug!("tuna-obs: webserver reachable again, leaving lite mode");
        lite_mode.store(false, Ordering::Relaxed);
    }
    if let Err(error) = client.post(TUNA_URL).json(&json!({ "data": data })).send().await {
        log::debug!("tuna-obs: webserver at port 1608 unreachable ({error}), entering lite mode");
        lite_mode.store(true, Ordering::Relaxed);
    }
}

impl Plugin for TunaObs {
    fn id(&self) -> &'static str {
        "tuna-obs"
    }

    fn start(&self, app: &AppHandle, _config: &Value) {
        request_time_updates(app, self.id());
        let lite_mode = self.lite_mode.clone();
        app.state::<SongInfoProvider>().register(self.id(), move |_, song, _| {
            if !has_song(song) {
                return;
            }
            let image = song.image_src.clone().unwrap_or_default();
            let data = json!({
                "duration": millis(Some(song.song_duration)).unwrap_or(0),
                "progress": millis(song.elapsed_seconds).unwrap_or(0),
                "cover": image,
                "cover_url": image,
                "album_url": image,
                "title": song.title,
                "alternativeTitle": song.alternative_title.clone().unwrap_or_default(),
                "artists": [song.artist],
                "status": if song.is_paused.unwrap_or(false) { "stopped" } else { "playing" },
                "album": song.album,
                "url": song.url.clone().unwrap_or_default(),
                "tags": song.tags,
            });
            tauri::async_runtime::spawn(post_to_tuna(lite_mode.clone(), data));
        });
    }
}
