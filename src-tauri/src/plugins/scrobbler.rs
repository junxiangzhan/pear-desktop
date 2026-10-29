//! Port of `src/plugins/scrobbler` (`main.ts`, `services/*`): "now playing"
//! on every song change and a scrobble once the song is half done (or at
//! 4 minutes), to Last.fm and/or ListenBrainz.
//!
//! Last.fm sign-in uses a window on last.fm; whether the user approved is read
//! from the page through its title (the window gets no IPC access).

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
#[cfg(desktop)]
use tauri::{webview::PageLoadEvent, WebviewUrl, WebviewWindowBuilder};
#[cfg(desktop)]
use tokio::sync::oneshot;

use super::Plugin;
#[cfg(desktop)]
use crate::ipc::MAIN_WEBVIEW;
use crate::{
    config::Config,
    song_info::{MediaType, SongInfo, SongInfoEvent, SongInfoProvider},
};

const ID: &str = "scrobbler";
const LASTFM_POST_URL: &str = "https://ws.audioscrobbler.com/2.0/";

#[derive(Default)]
struct State {
    config: Value,
    timer: Option<tauri::async_runtime::JoinHandle<()>>,
}

pub struct Scrobbler {
    state: Arc<Mutex<State>>,
}

impl Scrobbler {
    pub fn new() -> Self {
        Self { state: Arc::default() }
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

/// JS `String(number)`: integers without a fraction.
fn js_number(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}", n as i64) } else { n.to_string() }
}

/// Last.fm API signature: sorted `key + value` pairs (minus `format`), the
/// secret, then MD5 (https://www.last.fm/api/authspec).
fn api_sig(params: &BTreeMap<String, String>, secret: &str) -> String {
    let mut text: String = params
        .iter()
        .filter(|(key, _)| key.as_str() != "format")
        .map(|(key, value)| format!("{key}{value}"))
        .collect();
    text.push_str(secret);
    format!("{:x}", md5::compute(text.as_bytes()))
}

fn lastfm(config: &Value, key: &str) -> String {
    config["scrobblers"]["lastfm"][key].as_str().unwrap_or_default().to_owned()
}

fn save_lastfm(app: &AppHandle, token: Option<&str>, session_key: Option<&str>) {
    let config = app.state::<Config>();
    let default = config.plugin_default(ID);
    let update = json!({ "scrobblers": { "lastfm": { "token": token, "sessionKey": session_key } } });
    config.set_partial(app, &format!("plugins.{ID}"), &update, Some(&default));
}

async fn lastfm_get(config: &Value, method: &str, extra: &[(&str, String)]) -> Option<Value> {
    let mut params = BTreeMap::from([
        ("api_key".to_owned(), lastfm(config, "apiKey")),
        ("format".to_owned(), "json".to_owned()),
        ("method".to_owned(), method.to_owned()),
    ]);
    for (key, value) in extra {
        params.insert((*key).to_owned(), value.clone());
    }
    let sig = api_sig(&params, &lastfm(config, "secret"));
    params.insert("api_sig".to_owned(), sig);
    let response = crate::net::client()
        .get(lastfm(config, "apiRoot"))
        .query(&params)
        .send()
        .await
        .ok()?;
    response.json().await.ok()
}

async fn create_token(config: &Value) -> Option<String> {
    lastfm_get(config, "auth.gettoken", &[]).await?["token"].as_str().map(str::to_owned)
}

/// The approval page needs a second window, which mobile does not have.
#[cfg(mobile)]
async fn authenticate(_app: &AppHandle, _config: &Value, _token: &str) -> bool {
    log::warn!("scrobbler: last.fm sign-in needs a desktop window");
    false
}

/// Opens last.fm's approval page; resolves true once the user approved.
#[cfg(desktop)]
async fn authenticate(app: &AppHandle, config: &Value, token: &str) -> bool {
    const APPROVED: &str = "pear-lastfm:approved";
    let url = format!(
        "https://www.last.fm/api/auth/?api_key={}&token={token}",
        lastfm(config, "apiKey")
    );
    let Ok(url) = url.parse() else { return false };
    let (sender, receiver) = oneshot::channel::<bool>();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let resolve = move |sender: &Arc<Mutex<Option<oneshot::Sender<bool>>>>, approved: bool| {
        if let Some(sender) = sender.lock().unwrap().take() {
            let _ = sender.send(approved);
        }
    };

    let (on_load, on_title, on_close) = (sender.clone(), sender.clone(), sender.clone());
    let mut builder = crate::window::shared_webview_options(WebviewWindowBuilder::new(
        app,
        "lastfm-auth",
        WebviewUrl::External(url),
    ))
    .title("Last.fm")
    .inner_size(500.0, 600.0)
    .minimizable(false)
    .maximizable(false)
    .center()
    .on_page_load(move |window, payload| {
        if payload.event() != PageLoadEvent::Finished {
            return;
        }
        match payload.url().path() {
            // Still the approval screen while its confirm button is there
            "/api/auth" => {
                let _ = window.eval(format!(
                    "if (!document.getElementsByName('confirm').length) document.title = '{APPROVED}';"
                ));
            }
            "/api/None" => {
                resolve(&on_load, false);
                let _ = window.close();
            }
            _ => {}
        }
    })
    .on_document_title_changed(move |window, title| {
        if title == APPROVED {
            resolve(&on_title, true);
            let _ = window.close();
        }
    });
    if let Some(main) = app.get_webview_window(MAIN_WEBVIEW) {
        builder = match builder.parent(&main) {
            Ok(builder) => builder,
            Err(error) => {
                log::error!("scrobbler: parenting the last.fm window failed: {error}");
                return false;
            }
        };
    }
    let window = match builder.build() {
        Ok(window) => window,
        Err(error) => {
            log::error!("scrobbler: opening last.fm sign-in failed: {error}");
            return false;
        }
    };
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            resolve(&on_close, false);
        }
    });

    let approved = receiver.await.unwrap_or(false);
    if !approved {
        crate::dialogs::error(app, "Last.fm", "Last.fm authentication failed.");
    }
    approved
}

/// Port of `LastFmScrobbler.createSession`: trades the token for a session
/// key, signing in again when the token is not (or no longer) approved.
async fn create_session(app: &AppHandle, config: &Value) {
    let token = lastfm(config, "token");
    let response = lastfm_get(config, "auth.getsession", &[("token", token)]).await;
    if let Some(key) = response.as_ref().and_then(|r| r["session"]["key"].as_str()) {
        save_lastfm(app, Some(&lastfm(config, "token")), Some(key));
        return;
    }
    let Some(token) = create_token(config).await else {
        log::warn!("scrobbler: could not get a Last.fm token");
        return;
    };
    save_lastfm(app, Some(&token), None);
    if authenticate(app, config, &token).await {
        let mut retry = config.clone();
        retry["scrobblers"]["lastfm"]["token"] = json!(token);
        let response = lastfm_get(&retry, "auth.getsession", &[("token", token.clone())]).await;
        if let Some(key) = response.as_ref().and_then(|r| r["session"]["key"].as_str()) {
            save_lastfm(app, Some(&token), Some(key));
        }
    }
}

fn title_and_artist(song: &SongInfo, config: &Value) -> (String, String) {
    let title = match (&song.alternative_title, config["alternativeTitles"].as_bool().unwrap_or(true)) {
        (Some(alternative), true) => alternative.clone(),
        _ => song.title.clone(),
    };
    let artist = match (song.tags.first(), config["alternativeArtist"].as_bool().unwrap_or(true)) {
        (Some(tag), true) => tag.clone(),
        _ => song.artist.clone(),
    };
    (title, artist)
}

/// Port of `postSongDataToAPI` (now playing or scrobble).
async fn lastfm_post(app: &AppHandle, song: &SongInfo, method: &str, timestamp: Option<u64>) {
    let config = app.state::<Config>().plugin_config(ID);
    if lastfm(&config, "sessionKey").is_empty() {
        return;
    }
    let (title, artist) = title_and_artist(song, &config);
    let mut params = BTreeMap::from([
        ("track".to_owned(), title),
        ("duration".to_owned(), js_number(song.song_duration)),
        ("artist".to_owned(), artist),
        ("api_key".to_owned(), lastfm(&config, "apiKey")),
        ("sk".to_owned(), lastfm(&config, "sessionKey")),
        ("format".to_owned(), "json".to_owned()),
        ("method".to_owned(), method.to_owned()),
    ]);
    if let Some(album) = song.album.as_deref().filter(|a| !a.is_empty()) {
        params.insert("album".to_owned(), album.to_owned());
    }
    if let Some(timestamp) = timestamp {
        params.insert("timestamp".to_owned(), timestamp.to_string());
    }
    let sig = api_sig(&params, &lastfm(&config, "secret"));
    params.insert("api_sig".to_owned(), sig);
    if let Err(error) = crate::net::client().post(LASTFM_POST_URL).form(&params).send().await {
        log::warn!("scrobbler: Last.fm {method} failed: {error}");
    }
}

/// Port of `ListenbrainzScrobbler` (`playing_now` or `single`).
async fn listenbrainz_submit(app: &AppHandle, song: &SongInfo, listen_type: &str) {
    let config = app.state::<Config>().plugin_config(ID);
    let lb = &config["scrobblers"]["listenbrainz"];
    let (Some(api_root), Some(token)) = (
        lb["apiRoot"].as_str().filter(|s| !s.is_empty()),
        lb["token"].as_str().filter(|s| !s.is_empty()),
    ) else {
        return;
    };
    let (title, artist) = title_and_artist(song, &config);
    let mut payload = json!({
        "track_metadata": {
            "artist_name": artist,
            "track_name": title,
            "release_name": song.album,
            "additional_info": {
                "media_player": "Pear Tauri",
                "submission_client": "Pear Tauri - Scrobbler Plugin",
                "origin_url": song.url,
                "duration": song.song_duration,
            }
        }
    });
    if listen_type == "single" {
        payload["listened_at"] = json!(now_secs());
    }
    let result = crate::net::client()
        .post(format!("{api_root}submit-listens"))
        .header("Authorization", format!("Token {token}"))
        .json(&json!({ "listen_type": listen_type, "payload": [payload] }))
        .send()
        .await;
    if let Err(error) = result {
        log::warn!("scrobbler: ListenBrainz {listen_type} failed: {error}");
    }
}

fn enabled(config: &Value, service: &str) -> bool {
    config["scrobblers"][service]["enabled"].as_bool().unwrap_or(false)
}

async fn now_playing(app: AppHandle, song: SongInfo, config: Value) {
    if enabled(&config, "lastfm") {
        lastfm_post(&app, &song, "track.updateNowPlaying", None).await;
    }
    if enabled(&config, "listenbrainz") {
        listenbrainz_submit(&app, &song, "playing_now").await;
    }
}

async fn scrobble(app: AppHandle, song: SongInfo, config: Value) {
    if enabled(&config, "lastfm") {
        // Same as Electron, which subtracts seconds from milliseconds here
        let timestamp = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as f64
            - song.elapsed_seconds.unwrap_or(0.0))
            / 1000.0;
        lastfm_post(&app, &song, "track.scrobble", Some(timestamp.trunc() as u64)).await;
    }
    if enabled(&config, "listenbrainz") {
        listenbrainz_submit(&app, &song, "single").await;
    }
}

fn ensure_lastfm_session(app: &AppHandle, config: &Value) {
    if enabled(config, "lastfm") && lastfm(config, "sessionKey").is_empty() {
        let (app, config) = (app.clone(), config.clone());
        tauri::async_runtime::spawn(async move { create_session(&app, &config).await });
    }
}

impl Plugin for Scrobbler {
    fn id(&self) -> &'static str {
        ID
    }

    fn start(&self, app: &AppHandle, config: &Value) {
        self.state.lock().unwrap().config = config.clone();
        ensure_lastfm_session(app, config);

        let state = self.state.clone();
        app.state::<SongInfoProvider>().register(ID, move |app, song, event| {
            if event == SongInfoEvent::TimeChanged {
                return;
            }
            let mut state = state.lock().unwrap();
            if let Some(timer) = state.timer.take() {
                timer.abort();
            }
            if song.is_paused.unwrap_or(false) {
                return;
            }
            let config = state.config.clone();
            // Scrobblers handle official music videos fine; other media is opt-in
            if !config["scrobbleOtherMedia"].as_bool().unwrap_or(true)
                && !matches!(song.media_type, MediaType::Audio | MediaType::OriginalMusicVideo)
            {
                return;
            }
            // Scrobble when the song is halfway through, or at the 4 minute mark
            let scrobble_at = (song.song_duration / 2.0).ceil().min(240.0);
            let elapsed = song.elapsed_seconds.unwrap_or(0.0);
            if scrobble_at > elapsed {
                let (app, song, config) = (app.clone(), song.clone(), config.clone());
                state.timer = Some(tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_secs_f64(scrobble_at - elapsed)).await;
                    scrobble(app, song, config).await;
                }));
            }
            tauri::async_runtime::spawn(now_playing(app.clone(), song.clone(), config));
        });
    }

    fn stop(&self, _app: &AppHandle) {
        if let Some(timer) = self.state.lock().unwrap().timer.take() {
            timer.abort();
        }
    }

    fn on_config_change(&self, app: &AppHandle, config: &Value) {
        let was_enabled = enabled(&self.state.lock().unwrap().config, "lastfm");
        self.state.lock().unwrap().config = config.clone();
        if !was_enabled {
            ensure_lastfm_session(app, config);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_skips_format_and_sorts_keys() {
        let params = BTreeMap::from([
            ("method".to_owned(), "auth.gettoken".to_owned()),
            ("format".to_owned(), "json".to_owned()),
            ("api_key".to_owned(), "key".to_owned()),
        ]);
        let expected = format!("{:x}", md5::compute("api_keykeymethodauth.gettokensecret"));
        assert_eq!(api_sig(&params, "secret"), expected);
    }

    #[test]
    fn numbers_print_like_javascript() {
        assert_eq!(js_number(213.0), "213");
        assert_eq!(js_number(1.5), "1.5");
    }
}
