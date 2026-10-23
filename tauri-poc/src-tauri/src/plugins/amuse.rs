//! Port of the `amuse` backend (src/plugins/amuse/backend.ts): the current
//! song for the Amuse OBS widget on port 9863.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{extract::State, middleware, routing::get, Json, Router};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use super::Plugin;
use crate::{
    menu::label,
    song_info::{SongInfo, SongInfoProvider},
};

const PORT: u16 = 9863;

type Current = Arc<Mutex<SongInfo>>;

/// Like `formatSongInfo`.
fn format(info: &SongInfo) -> Value {
    json!({
        "player": {
            "hasSong": !info.artist.is_empty() && !info.title.is_empty(),
            "isPaused": info.is_paused.unwrap_or(false),
            "seekbarCurrentPosition": info.elapsed_seconds.unwrap_or(0.0),
        },
        "track": {
            "duration": info.song_duration,
            "title": info.title,
            "author": info.artist,
            "cover": info.image_src.clone().unwrap_or_default(),
            "url": info.url.clone().unwrap_or_default(),
            "id": info.video_id,
            "isAdvertisement": false,
        },
    })
}

pub struct Amuse {
    server: Mutex<Option<axum_server::Handle<SocketAddr>>>,
}

impl Amuse {
    pub fn new() -> Self {
        Self { server: Mutex::new(None) }
    }
}

impl Plugin for Amuse {
    fn id(&self) -> &'static str {
        "amuse"
    }

    fn start(&self, app: &AppHandle, _config: &Value) {
        let current: Current = Arc::default();
        let latest = current.clone();
        app.state::<SongInfoProvider>().register(self.id(), move |_, song, _| {
            *latest.lock().unwrap() = song.clone();
        });

        let greeting = label(
            app,
            "plugins.amuse.response.query",
            "Amuse API server is running. GET /query to get song info.",
        );
        let song = |State(current): State<Current>| async move { Json(format(&current.lock().unwrap())) };
        let router = Router::new()
            .route("/", get(move || async move { greeting }))
            .route("/query", get(song))
            .route("/api", get(song))
            .layer(middleware::from_fn(super::api_server::cors))
            .with_state(current);

        let handle = axum_server::Handle::new();
        *self.server.lock().unwrap() = Some(handle.clone());
        tauri::async_runtime::spawn(async move {
            let addr = SocketAddr::from(([0, 0, 0, 0], PORT));
            if let Err(error) = axum_server::bind(addr).handle(handle).serve(router.into_make_service()).await {
                log::error!("amuse: serving on port {PORT} failed: {error}");
            }
        });
    }

    fn stop(&self, _app: &AppHandle) {
        if let Some(handle) = self.server.lock().unwrap().take() {
            handle.shutdown();
        }
    }
}
