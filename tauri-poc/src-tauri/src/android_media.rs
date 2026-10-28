//! Android background playback: song info goes to `PlaybackService`
//! (gen/android/.../PlaybackService.kt), a `mediaPlayback` foreground service
//! with a media session. Without it Android freezes the process as soon as
//! the app leaves the screen, which stops the WebView's audio. Presses in the
//! system media controls come back as song controls.

use std::{sync::Mutex, time::Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::{
    ipc::{Channel, InvokeResponseBody},
    plugin::{Builder, PluginHandle, TauriPlugin},
    AppHandle, Manager, Wry,
};

use crate::{
    song_controls,
    song_info::{SongInfo, SongInfoEvent, SongInfoProvider},
};

struct MediaSession(PluginHandle<Wry>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Playback<'a> {
    title: &'a str,
    artist: &'a str,
    album: Option<&'a str>,
    artwork_url: Option<&'a str>,
    is_paused: bool,
    duration_ms: u64,
    position_ms: u64,
}

pub fn init() -> TauriPlugin<Wry> {
    Builder::new("pear-media-session")
        .setup(|app, api| {
            let handle = api.register_android_plugin("io.github.junxiangzhan.peartauri", "MediaSessionPlugin")?;
            app.manage(MediaSession(handle));
            Ok(())
        })
        .build()
}

/// What was last sent, to skip the per-second time updates that only
/// confirm the position the session already extrapolates.
struct Sent {
    video_id: String,
    is_paused: bool,
    position: f64,
    at: Instant,
}

fn is_redundant(last: &Option<Sent>, song: &SongInfo, event: SongInfoEvent) -> bool {
    let Some(last) = last else { return false };
    if event != SongInfoEvent::TimeChanged || last.video_id != song.video_id {
        return false;
    }
    let paused = song.is_paused.unwrap_or(false);
    let expected = if last.is_paused { last.position } else { last.position + last.at.elapsed().as_secs_f64() };
    paused == last.is_paused && (song.elapsed_seconds.unwrap_or(0.0) - expected).abs() <= 2.0
}

/// Connects the song-info provider and the control channel.
pub fn register(app: &AppHandle) {
    let channel: Channel = Channel::new({
        let app = app.clone();
        move |body| {
            let message: Value = match body {
                InvokeResponseBody::Json(json) => serde_json::from_str(&json).unwrap_or_default(),
                InvokeResponseBody::Raw(raw) => serde_json::from_slice(&raw).unwrap_or_default(),
            };
            match message["action"].as_str().unwrap_or_default() {
                "play" => song_controls::play(&app),
                "pause" => song_controls::pause(&app),
                "next" => song_controls::next(&app),
                "previous" => song_controls::previous(&app),
                "seek" => {
                    let ms = message["value"].as_f64().unwrap_or(0.0);
                    song_controls::seek_to(&app, ms / 1000.0);
                }
                other => log::warn!("media session: unknown action {other:?}"),
            }
            Ok(())
        }
    });
    // Plugin calls block until Kotlin answers on the UI thread
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let session = handle.state::<MediaSession>();
        let args = serde_json::json!({ "channel": channel });
        if let Err(error) = session.0.run_mobile_plugin::<()>("setActionChannel", args) {
            log::error!("media session: registering the control channel failed: {error}");
        }
    });

    let last: Mutex<Option<Sent>> = Mutex::new(None);
    app.state::<SongInfoProvider>().register("core", move |app, song, event| {
        if song.title.is_empty() && song.artist.is_empty() {
            return;
        }
        {
            let mut last = last.lock().unwrap();
            if is_redundant(&last, song, event) {
                return;
            }
            *last = Some(Sent {
                video_id: song.video_id.clone(),
                is_paused: song.is_paused.unwrap_or(false),
                position: song.elapsed_seconds.unwrap_or(0.0),
                at: Instant::now(),
            });
        }
        let payload = serde_json::to_value(Playback {
            title: &song.title,
            artist: &song.artist,
            album: song.album.as_deref(),
            artwork_url: song.image_src.as_deref(),
            is_paused: song.is_paused.unwrap_or(false),
            duration_ms: (song.song_duration * 1000.0) as u64,
            position_ms: (song.elapsed_seconds.unwrap_or(0.0) * 1000.0) as u64,
        })
        .unwrap_or_default();
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let session = app.state::<MediaSession>();
            if let Err(error) = session.0.run_mobile_plugin::<()>("update", payload) {
                log::warn!("media session: update failed: {error}");
            }
        });
    });
}
