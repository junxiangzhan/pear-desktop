//! Port of the `downloader` backend (src/plugins/downloader/main/index.ts).
//!
//! The page finds the stream and the metadata with youtubei.js, which needs
//! BotGuard (`tauri-poc/inject/downloader.ts`); this side downloads, tags and
//! saves the file. There is no ffmpeg, so files keep YouTube's own audio
//! format (AAC in MP4, `.m4a`) whatever preset is chosen, rewritten from
//! YouTube's fragmented MP4 into a regular one (`remux.rs`; Windows' player
//! can't play the fragmented file), and are tagged with lofty (MP4 ilst)
//! instead of ID3.
//!
//! - `downloader:save` (invoke): downloads one song; progress goes back as
//!   `downloader:progress` `[id, ratio]`
//! - `downloader:prepare-folder` (invoke): a playlist's folder
//! - `downloader:dialog` (invoke), `downloader:progress-bar` (send): the
//!   message boxes and the taskbar progress of the Electron backend
//! - download on finish: song-info callback, asks the page to download the
//!   song that just finished (`downloader:download-on-finish`)
//!
//! Desktop saves into the download folder (setting, or the user's Downloads
//! folder). Android saves into the shared Music/Pear Desktop collection
//! through MediaStore (`android_app::save_audio`); its folder settings are
//! hidden.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::io::AsyncWriteExt;

use super::Plugin;
use crate::{
    config::Config,
    ipc::{send_to_page, Ipc},
    song_info::{cleanup_name, SongInfo, SongInfoEvent, SongInfoProvider},
};

mod remux;

const ID: &str = "downloader";
/// googlevideo throttles long unranged reads; youtubei.js reads 10 MB ranges
const CHUNK_SIZE: u64 = 10 * 1024 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

pub struct Downloader;

impl Plugin for Downloader {
    fn id(&self) -> &'static str {
        ID
    }

    fn start(&self, app: &AppHandle, _config: &Value) {
        let ipc = app.state::<Ipc>();
        ipc.handle(ID, "downloader:save", |app, args| async move {
            let request: SaveRequest = serde_json::from_value(args.into_iter().next().unwrap_or_default())
                .map_err(|error| error.to_string())?;
            save(&app, request).await
        });
        ipc.handle(ID, "downloader:prepare-folder", |app, args| async move {
            let title = args.first().and_then(Value::as_str).unwrap_or_default();
            prepare_folder(&app, title)
        });
        ipc.handle(ID, "downloader:dialog", |app, args| async move {
            show_dialog(&app, args.first().unwrap_or(&Value::Null));
            Ok(Value::Null)
        });
        ipc.on(ID, "downloader:progress-bar", |app, args| {
            set_progress_bar(app, args.first().and_then(Value::as_f64).unwrap_or(-1.0));
        });
        download_on_finish_setup(app);
    }

    fn stop(&self, app: &AppHandle) {
        set_progress_bar(app, -1.0);
    }
}

fn config(app: &AppHandle) -> Value {
    app.state::<Config>().plugin_config(ID)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveRequest {
    id: u64,
    url: String,
    content_length: Option<u64>,
    title: String,
    artist: String,
    album: Option<String>,
    track: Option<u32>,
    cover: Option<String>,
    /// A folder chosen for this download (download on finish)
    folder: Option<String>,
    /// A playlist's folder, inside the download folder
    subfolder: Option<String>,
}

struct Metadata {
    title: String,
    artist: String,
    album: Option<String>,
    track: Option<u32>,
    cover: Option<Vec<u8>>,
}

/// Like `filenamify` with a `replacement`: characters Windows, macOS or
/// Linux reject become `replacement`, reserved names get it appended, and
/// the name is cut to 255 bytes. NFC normalization happens in the page.
fn sanitize(name: &str, replacement: char) -> String {
    let mut result: String = name
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { replacement } else { c })
        .collect();
    let trimmed = result.trim_end_matches(['.', ' ']).trim_start();
    result = if trimmed.is_empty() { replacement.to_string() } else { trimmed.to_owned() };
    // filenamify only checks the whole name
    let upper = result.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.as_bytes()[3].is_ascii_digit());
    if reserved {
        result.push(replacement);
    }
    if result.len() > 255 {
        // Keep the extension when cutting
        let extension = Path::new(&result)
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| e.len() < 16)
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        let mut end = 255 - extension.len();
        while !result.is_char_boundary(end) {
            end -= 1;
        }
        result = format!("{}{extension}", &result[..end]);
    }
    result
}

#[cfg(desktop)]
fn download_folder(app: &AppHandle, folder: Option<String>) -> Result<PathBuf, String> {
    let configured = config(app)["downloadFolder"].as_str().filter(|f| !f.is_empty()).map(str::to_owned);
    match folder.or(configured) {
        Some(folder) => Ok(PathBuf::from(folder)),
        None => app.path().download_dir().map_err(|error| error.to_string()),
    }
}

/// Like `downloadPlaylist`'s folder check: `exists` while not skipping
/// existing files means the page reports it and stops.
fn prepare_folder(app: &AppHandle, title: &str) -> Result<Value, String> {
    let name = sanitize(title, ' ');
    #[cfg(desktop)]
    {
        let path = download_folder(app, None)?.join(&name);
        let exists = path.exists();
        if !exists {
            std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        }
        let skip_existing = config(app)["skipExisting"].as_bool().unwrap_or(false);
        Ok(json!({ "path": path, "exists": exists && !skip_existing }))
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Ok(json!({ "path": format!("Music/Pear Desktop/{name}"), "exists": false }))
    }
}

async fn save(app: &AppHandle, request: SaveRequest) -> Result<Value, String> {
    let title = cleanup_name(&request.title);
    let artist = cleanup_name(&request.artist);
    let name = if artist.is_empty() { title.clone() } else { format!("{artist} - {title}") };
    let filename = sanitize(&format!("{name}.m4a"), '_');
    let subfolder = request.subfolder.as_deref().map(|title| sanitize(title, ' '));
    let skip_existing = config(app)["skipExisting"].as_bool().unwrap_or(false);

    #[cfg(desktop)]
    let (target, part) = {
        let mut dir = download_folder(app, request.folder.clone())?;
        if let Some(subfolder) = &subfolder {
            dir.push(subfolder);
        }
        let target = dir.join(&filename);
        if skip_existing && target.exists() {
            return Ok(json!({ "path": target, "skipped": true }));
        }
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let part = dir.join(format!("{filename}.part"));
        (target, part)
    };
    #[cfg(mobile)]
    let part = {
        if skip_existing && crate::android_app::audio_exists(app, &filename, subfolder.as_deref()).await? {
            return Ok(json!({ "path": filename, "skipped": true }));
        }
        let dir = app.path().app_cache_dir().map_err(|error| error.to_string())?.join("downloads");
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        dir.join(format!("{}.m4a", request.id))
    };

    let result = async {
        download(app, &request, &part).await?;
        let cover = match &request.cover {
            Some(url) => fetch_cover(url).await.map_err(|error| log::warn!("downloader: no cover: {error}")).ok(),
            None => None,
        };
        let metadata = Metadata {
            title,
            artist,
            album: request.album.clone().filter(|album| !album.is_empty()),
            track: request.track,
            cover,
        };
        let path = part.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let data = std::fs::read(&path).map_err(|error| error.to_string())?;
            if let Some(regular) = remux::remux(&data).map_err(|error| format!("remuxing failed: {error}"))? {
                std::fs::write(&path, regular).map_err(|error| error.to_string())?;
            }
            write_tags(&path, metadata)
        })
        .await
        .map_err(|error| error.to_string())??;
        #[cfg(desktop)]
        {
            std::fs::rename(&part, &target).map_err(|error| error.to_string())?;
            Ok::<_, String>(target.display().to_string())
        }
        #[cfg(mobile)]
        {
            let uri = crate::android_app::save_audio(app, &part, &filename, subfolder.as_deref()).await?;
            let _ = std::fs::remove_file(&part);
            Ok::<_, String>(uri)
        }
    }
    .await;
    match result {
        Ok(path) => {
            log::info!("downloader: saved {path}");
            Ok(json!({ "path": path }))
        }
        Err(error) => {
            let _ = std::fs::remove_file(&part);
            Err(error)
        }
    }
}

/// Streams the file in `CHUNK_SIZE` ranges (the `range` parameter, like
/// youtubei.js), reporting progress to the page.
async fn download(app: &AppHandle, request: &SaveRequest, path: &Path) -> Result<(), String> {
    let mut file = tokio::fs::File::create(path).await.map_err(|error| error.to_string())?;
    let total = request.content_length.filter(|&length| length > 0);
    let mut downloaded = 0u64;
    let mut last_report = Instant::now() - PROGRESS_INTERVAL;
    loop {
        let mut url = reqwest::Url::parse(&request.url).map_err(|error| error.to_string())?;
        if let Some(total) = total {
            let end = (downloaded + CHUNK_SIZE).min(total) - 1;
            url.query_pairs_mut().append_pair("range", &format!("{downloaded}-{end}"));
        }
        let mut response = crate::net::client()
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| error.without_url().to_string())?;
        let size = total.or(response.content_length());
        let before = downloaded;
        while let Some(chunk) = response.chunk().await.map_err(|error| error.without_url().to_string())? {
            file.write_all(&chunk).await.map_err(|error| error.to_string())?;
            downloaded += chunk.len() as u64;
            if let Some(size) = size {
                if last_report.elapsed() >= PROGRESS_INTERVAL {
                    last_report = Instant::now();
                    send_to_page(app, "downloader:progress", &[json!(request.id), json!(downloaded as f64 / size as f64)]);
                }
            }
        }
        match total {
            Some(total) if downloaded < total => {
                if downloaded == before {
                    return Err("the download stopped".into());
                }
            }
            _ => break,
        }
    }
    file.flush().await.map_err(|error| error.to_string())?;
    send_to_page(app, "downloader:progress", &[json!(request.id), json!(1.0)]);
    Ok(())
}

/// The cover as PNG, cropped like `cropMaxWidth`: 1280×720 artwork has the
/// square cover between 280 px margins.
async fn fetch_cover(url: &str) -> Result<Vec<u8>, String> {
    let bytes = crate::net::client()
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| error.to_string())?
        .bytes()
        .await
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut image = image::load_from_memory(&bytes).map_err(|error| error.to_string())?;
        if image.width() == 1280 && image.height() == 720 {
            image = image.crop_imm(280, 0, 720, 720);
        }
        let mut png = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|error| error.to_string())?;
        Ok(png.into_inner())
    })
    .await
    .map_err(|error| error.to_string())?
}

fn write_tags(path: &Path, metadata: Metadata) -> Result<(), String> {
    use lofty::{
        config::WriteOptions,
        mp4::Ilst,
        picture::{MimeType, Picture, PictureType},
        tag::{Accessor, TagExt},
    };

    let mut tag = Ilst::new();
    tag.set_title(metadata.title);
    if !metadata.artist.is_empty() {
        tag.set_artist(metadata.artist);
    }
    if let Some(album) = metadata.album {
        tag.set_album(album);
    }
    if let Some(track) = metadata.track {
        tag.set_track(track);
    }
    if let Some(cover) = metadata.cover {
        tag.insert_picture(
            Picture::unchecked(cover)
                .pic_type(PictureType::CoverFront)
                .mime_type(MimeType::Png)
                .build(),
        );
    }
    tag.save_to_path(path, WriteOptions::default())
        .map_err(|error| format!("writing the tags failed: {error}"))
}

fn show_dialog(app: &AppHandle, options: &Value) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

    let message = [options["message"].as_str(), options["detail"].as_str()]
        .into_iter()
        .flatten()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let kind = if options["error"].as_bool().unwrap_or(false) {
        MessageDialogKind::Error
    } else {
        MessageDialogKind::Info
    };
    app.dialog()
        .message(message)
        .title(options["title"].as_str().unwrap_or_default())
        .kind(kind)
        .show(|_| {});
}

/// Electron's `setProgressBar`: above 1 is indeterminate, below 0 removes it.
fn set_progress_bar(app: &AppHandle, value: f64) {
    #[cfg(desktop)]
    {
        use tauri::window::{ProgressBarState, ProgressBarStatus};

        let Some(window) = app.get_webview_window(crate::ipc::MAIN_WEBVIEW) else { return };
        let state = if value < 0.0 {
            ProgressBarState { status: Some(ProgressBarStatus::None), progress: None }
        } else if value > 1.0 {
            ProgressBarState { status: Some(ProgressBarStatus::Indeterminate), progress: None }
        } else {
            ProgressBarState {
                status: Some(ProgressBarStatus::Normal),
                progress: Some((value * 100.0).round() as u64),
            }
        };
        let _ = window.set_progress_bar(state);
    }
    #[cfg(mobile)]
    let _ = (app, value);
}

#[derive(Default)]
struct OnFinish {
    current_url: Option<String>,
    duration: f64,
    time: f64,
}

/// Port of `downloadSongOnFinishSetup`: when the next song starts, downloads
/// the previous one if it played to its last `seconds` or past `percent`.
fn download_on_finish_setup(app: &AppHandle) {
    let state = Arc::new(Mutex::new(OnFinish::default()));
    app.state::<SongInfoProvider>().register(ID, move |app, song: &SongInfo, event| {
        let mut state = state.lock().unwrap();
        if event == SongInfoEvent::TimeChanged {
            let elapsed = song.elapsed_seconds.unwrap_or(0.0);
            if elapsed > state.time {
                state.time = elapsed;
            }
            return;
        }
        let config = config(app);
        let on_finish = &config["downloadOnFinish"];
        if song.is_paused == Some(true)
            || song.url == state.current_url
            || !on_finish["enabled"].as_bool().unwrap_or(false)
        {
            return;
        }
        if let Some(url) = state.current_url.clone() {
            let (duration, time) = (state.duration, state.time);
            let finished = duration > 0.0
                && match on_finish["mode"].as_str() {
                    Some("percent") => time >= duration * (on_finish["percent"].as_f64().unwrap_or(10.0) / 100.0),
                    _ => duration - time <= on_finish["seconds"].as_f64().unwrap_or(20.0),
                };
            if finished {
                let folder = on_finish["folder"].as_str().or(config["downloadFolder"].as_str());
                send_to_page(app, "downloader:download-on-finish", &[json!(url), json!(folder)]);
            }
        }
        state.current_url = song.url.clone();
        state.duration = song.song_duration;
        state.time = 0.0;
    });
    super::request_player_listeners(app, ID, &["time-changed"]);
}

#[cfg(test)]
mod tests {
    use super::sanitize;

    #[test]
    fn sanitizes_file_names() {
        assert_eq!(sanitize("AC/DC - Back: In Black?.m4a", '_'), "AC_DC - Back_ In Black_.m4a");
        assert_eq!(sanitize("Album. ", ' '), "Album");
        assert_eq!(sanitize("con", '_'), "con_");
        assert_eq!(sanitize("lpt1", '_'), "lpt1_");
        let long = format!("{}.m4a", "語".repeat(100));
        let cut = sanitize(&long, '_');
        assert!(cut.len() <= 255 && cut.ends_with(".m4a"));
    }
}
