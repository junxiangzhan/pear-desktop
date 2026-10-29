//! Port of the `discord` backend (src/plugins/discord/): Rich Presence over
//! Discord's local IPC socket, replacing `@xhayper/discord-rpc`.
//!
//! One task owns the connection and the timers of `DiscordService`
//! (activity timeout, throttled update, reconnect retry); everything else
//! talks to it through `Command`s. The menu runs in the page and reaches the
//! service through `src/plugins/discord/service.ts`:
//! `discord:status` (invoke, and pushed on change), `discord:connect`,
//! `discord:clear-activity`.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc,
    time::{sleep_until, Instant},
};

use super::Plugin;
use crate::{
    ipc::{send_to_page, Ipc},
    song_info::{SongInfo, SongInfoEvent, SongInfoProvider},
};

/// Application ID registered by the pear-desktop team (constants.ts)
const CLIENT_ID: &str = "1177081335727267940";
const PROGRESS_THROTTLE: Duration = Duration::from_millis(15_000);
const TIME_UPDATE_DEBOUNCE: Duration = Duration::from_millis(5000);
const RETRY_DELAY: Duration = Duration::from_millis(5000);
const HANGUL_FILLER: char = '\u{3164}';
const APPLICATION_NAME: &str = "\u{59}\u{6f}\u{75}\u{54}\u{75}\u{62}\u{65} \u{4d}\u{75}\u{73}\u{69}\u{63}";
const PLAY_ON_KEY: &str = "playOn\u{59}\u{6f}\u{75}\u{54}\u{75}\u{62}\u{65}\u{4d}\u{75}\u{73}\u{69}\u{63}";

// Opcodes of Discord's IPC frames
const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;
const OP_PING: u32 = 3;
const OP_PONG: u32 = 4;

enum Command {
    Connect,
    Disconnect,
    Song(Box<SongInfo>, SongInfoEvent),
    ClearActivity,
    Config(Value),
    /// A frame from connection `generation`, `None` once it closed
    Incoming(u64, Option<(u32, Value)>),
}

pub struct Discord {
    commands: Mutex<Option<mpsc::UnboundedSender<Command>>>,
    /// Connected and ready, for the menu's connect item
    connected: Arc<AtomicBool>,
}

impl Discord {
    pub fn new() -> Self {
        Self { commands: Mutex::new(None), connected: Arc::default() }
    }

    fn send(&self, command: Command) {
        if let Some(commands) = self.commands.lock().unwrap().as_ref() {
            let _ = commands.send(command);
        }
    }
}

impl Plugin for Discord {
    fn id(&self) -> &'static str {
        "discord"
    }

    fn start(&self, app: &AppHandle, config: &Value) {
        let (tx, rx) = mpsc::unbounded_channel();
        *self.commands.lock().unwrap() = Some(tx.clone());
        self.connected.store(false, Ordering::Relaxed);

        super::request_player_listeners(app, self.id(), &["time-changed"]);
        let ipc = app.state::<Ipc>();
        let connected = self.connected.clone();
        ipc.handle(self.id(), "discord:status", move |_, _| {
            let connected = connected.load(Ordering::Relaxed);
            async move { Ok(json!(connected)) }
        });
        let commands = tx.clone();
        ipc.on(self.id(), "discord:connect", move |_, _| {
            let _ = commands.send(Command::Connect);
        });
        let commands = tx.clone();
        ipc.on(self.id(), "discord:clear-activity", move |_, _| {
            let _ = commands.send(Command::ClearActivity);
        });
        let commands = tx.clone();
        app.state::<SongInfoProvider>().register(self.id(), move |_, song, event| {
            let _ = commands.send(Command::Song(Box::new(song.clone()), event));
        });

        let service = Service::new(app.clone(), config.clone(), tx, self.connected.clone());
        tauri::async_runtime::spawn(service.run(rx));
    }

    fn stop(&self, _app: &AppHandle) {
        // Dropping the sender ends the task, which closes the connection
        if let Some(commands) = self.commands.lock().unwrap().take() {
            let _ = commands.send(Command::Disconnect);
        }
    }

    fn on_config_change(&self, _app: &AppHandle, config: &Value) {
        self.send(Command::Config(config.clone()));
    }
}

type Writer = Box<dyn AsyncWrite + Send + Unpin>;

/// `DiscordService` plus the debounce of `main.ts`.
struct Service {
    app: AppHandle,
    config: Value,
    auto_reconnect: bool,
    /// Commands to itself (frames from the reader task)
    commands: mpsc::UnboundedSender<Command>,
    connected: Arc<AtomicBool>,
    writer: Option<Writer>,
    generation: u64,
    ready: bool,
    last_song: Option<SongInfo>,
    last_progress_update: Option<Instant>,
    last_time_update_sent: Option<Instant>,
    nonce: u64,
    // TimerManager's timers
    clear_activity_at: Option<Instant>,
    update_timeout: Option<(Instant, SongInfo)>,
    retry_at: Option<Instant>,
}

async fn sleep_until_some(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

impl Service {
    fn new(
        app: AppHandle,
        config: Value,
        commands: mpsc::UnboundedSender<Command>,
        connected: Arc<AtomicBool>,
    ) -> Self {
        let auto_reconnect = config["autoReconnect"].as_bool().unwrap_or(true);
        Self {
            app,
            config,
            auto_reconnect,
            commands,
            connected,
            writer: None,
            generation: 0,
            ready: false,
            last_song: None,
            last_progress_update: None,
            last_time_update_sent: None,
            nonce: 0,
            clear_activity_at: None,
            update_timeout: None,
            retry_at: None,
        }
    }

    async fn run(mut self, mut commands: mpsc::UnboundedReceiver<Command>) {
        self.connect().await;
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    None | Some(Command::Disconnect) => break,
                    Some(command) => self.handle(command).await,
                },
                _ = sleep_until_some(self.retry_at) => {
                    self.retry_at = None;
                    if self.auto_reconnect && self.writer.is_none() {
                        self.connect().await;
                    }
                }
                _ = sleep_until_some(self.clear_activity_at) => {
                    self.clear_activity_at = None;
                    self.clear_activity().await;
                }
                _ = sleep_until_some(self.update_timeout.as_ref().map(|(at, _)| *at)) => {
                    let (_, snapshot) = self.update_timeout.take().expect("timer was set");
                    self.throttled_update(snapshot).await;
                }
            }
        }
        self.disconnect().await;
    }

    async fn handle(&mut self, command: Command) {
        match command {
            Command::Connect => self.connect().await,
            Command::Disconnect => {}
            Command::ClearActivity => self.clear_activity().await,
            Command::Song(song, event) => {
                // main.ts: only while connected, time updates debounced
                if !self.is_connected() {
                    return;
                }
                let now = Instant::now();
                let due = self
                    .last_time_update_sent
                    .is_none_or(|sent| now - sent > TIME_UPDATE_DEBOUNCE);
                if event != SongInfoEvent::TimeChanged || due {
                    self.update_activity(*song).await;
                    self.last_time_update_sent = Some(now);
                }
            }
            Command::Config(config) => {
                self.auto_reconnect = config["autoReconnect"].as_bool().unwrap_or(true);
                self.config = config;
                if self.is_connected() {
                    if let Some(song) = self.last_song.clone() {
                        self.update_activity(song).await;
                    }
                }
                self.set_activity_timeout();
                // main.ts#onConfigChange
                if !self.is_connected() {
                    self.connect().await;
                }
            }
            Command::Incoming(generation, frame) if generation == self.generation => {
                self.incoming(frame).await;
            }
            Command::Incoming(..) => {} // from a closed connection
        }
    }

    fn is_connected(&self) -> bool {
        self.writer.is_some() && self.ready
    }

    fn set_status(&self, connected: bool) {
        if self.connected.swap(connected, Ordering::Relaxed) != connected {
            // Like `refreshCallbacks`: the menu relabels its connect item
            send_to_page(&self.app, "discord:status", &[json!(connected)]);
        }
    }

    /// Like `connect()` and the retry loop: one attempt, then a retry in 5 s
    /// while auto-reconnect is on. The connection becomes usable at READY.
    async fn connect(&mut self) {
        if self.writer.is_some() {
            return;
        }
        self.retry_at = None;
        self.ready = false;
        match open_socket().await {
            Ok((reader, mut writer)) => {
                let handshake = json!({ "v": 1, "client_id": CLIENT_ID });
                if let Err(error) = write_frame(&mut writer, OP_HANDSHAKE, &handshake).await {
                    log::debug!("discord: handshake failed: {error}");
                    self.connect_failed();
                    return;
                }
                self.generation += 1;
                self.writer = Some(writer);
                tauri::async_runtime::spawn(read_frames(reader, self.generation, self.commands.clone()));
            }
            Err(error) => {
                log::debug!("discord: not reachable: {error}");
                self.connect_failed();
            }
        }
    }

    fn connect_failed(&mut self) {
        self.reset_info();
        if self.auto_reconnect {
            self.retry_at = Some(Instant::now() + RETRY_DELAY);
        }
    }

    async fn incoming(&mut self, frame: Option<(u32, Value)>) {
        match frame {
            Some((OP_FRAME, payload)) if payload["evt"] == "READY" => {
                log::info!("discord: connected");
                self.ready = true;
                self.set_status(true);
                if let Some(song) = self.last_song.clone() {
                    self.update_activity(song).await;
                }
            }
            Some((OP_FRAME, payload)) if payload["evt"] == "ERROR" => {
                log::warn!("discord: request failed: {}", payload["data"]);
            }
            Some((OP_PING, payload)) => {
                if let Some(writer) = self.writer.as_mut() {
                    let _ = write_frame(writer, OP_PONG, &payload).await;
                }
            }
            Some((OP_CLOSE, payload)) => {
                log::info!("discord: closed by Discord: {payload}");
                self.disconnected();
            }
            Some(_) => {}
            None => self.disconnected(),
        }
    }

    /// The RPC client's `disconnected` event.
    fn disconnected(&mut self) {
        self.writer = None;
        self.connect_failed();
    }

    /// Like `resetInfo()`.
    fn reset_info(&mut self) {
        self.ready = false;
        self.last_song = None;
        self.last_progress_update = None;
        self.clear_activity_at = None;
        self.update_timeout = None;
        self.retry_at = None;
        self.set_status(false);
    }

    /// Like `disconnect()`: no reconnecting until the config says so again.
    async fn disconnect(&mut self) {
        self.auto_reconnect = false;
        if let Some(mut writer) = self.writer.take() {
            let _ = write_frame(&mut writer, OP_CLOSE, &json!({})).await;
            let _ = writer.shutdown().await;
        }
        self.generation += 1;
        self.reset_info();
    }

    async fn request(&mut self, command: &str, args: Value) {
        self.nonce += 1;
        let payload = json!({ "cmd": command, "args": args, "nonce": self.nonce.to_string() });
        if let Some(writer) = self.writer.as_mut() {
            if let Err(error) = write_frame(writer, OP_FRAME, &payload).await {
                log::warn!("discord: {command} failed: {error}");
            }
        }
    }

    async fn set_activity(&mut self, song: &SongInfo) {
        let activity = build_activity(song, &self.config, now_ms());
        self.request("SET_ACTIVITY", json!({ "pid": std::process::id(), "activity": activity }))
            .await;
    }

    /// Like `updateActivity()`: immediate on song change, pause or seek,
    /// otherwise at most every 15 s with a trailing update.
    async fn update_activity(&mut self, song: SongInfo) {
        if song.title.is_empty() && song.artist.is_empty() {
            if self.last_song.as_ref().is_some_and(|last| !last.video_id.is_empty()) {
                self.clear_activity().await;
                self.last_song = None;
            }
            return;
        }
        self.clear_activity_at = None;
        if !self.is_connected() {
            return;
        }

        let now = Instant::now();
        let changed = self.last_song.as_ref().map(|last| {
            let song_changed = song.video_id != last.video_id;
            let pause_changed = song.is_paused != last.is_paused;
            let seeked = !song_changed
                && is_seek(last.elapsed_seconds.unwrap_or(0.0), song.elapsed_seconds.unwrap_or(0.0));
            song_changed || pause_changed || seeked
        });
        let throttle_over = self
            .last_progress_update
            .is_none_or(|sent| now - sent > PROGRESS_THROTTLE);

        if changed == Some(true) || throttle_over {
            self.update_timeout = None;
            self.set_activity(&song).await;
            self.last_progress_update = Some(now);
            self.last_song = Some(song);
            self.set_activity_timeout();
        } else {
            let sent = self.last_progress_update.unwrap_or(now);
            self.update_timeout = Some((sent + PROGRESS_THROTTLE, song.clone()));
            self.last_song = Some(song);
        }
    }

    /// The trailing update scheduled by `update_activity`.
    async fn throttled_update(&mut self, snapshot: SongInfo) {
        let still_current = self.last_song.as_ref().is_some_and(|last| {
            last.video_id == snapshot.video_id && last.is_paused == snapshot.is_paused
        });
        if !still_current || !self.is_connected() {
            return;
        }
        self.set_activity(&snapshot).await;
        self.last_progress_update = Some(Instant::now());
        if let Some(last) = self.last_song.as_mut() {
            last.elapsed_seconds = snapshot.elapsed_seconds;
        }
        self.set_activity_timeout();
    }

    /// Like `setActivityTimeout()`: clear the presence after a long pause.
    fn set_activity_timeout(&mut self) {
        self.clear_activity_at = None;
        let paused = self.last_song.as_ref().is_some_and(|last| last.is_paused == Some(true));
        let enabled = self.config["activityTimeoutEnabled"].as_bool().unwrap_or(false);
        let timeout = self.config["activityTimeoutTime"].as_f64().unwrap_or(0.0);
        if paused && enabled && timeout > 0.0 {
            self.clear_activity_at = Some(Instant::now() + Duration::from_millis(timeout as u64));
        }
    }

    /// Like `clearActivity()`.
    async fn clear_activity(&mut self) {
        if self.is_connected() {
            self.request("SET_ACTIVITY", json!({ "pid": std::process::id() })).await;
        }
        self.last_progress_update = None;
        self.last_song = None;
        self.clear_activity_at = None;
        self.update_timeout = None;
    }
}

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as f64
}

/// Like `isSeek` in utils.ts.
fn is_seek(old_seconds: f64, new_seconds: f64) -> bool {
    (new_seconds - old_seconds).abs() > 2.0
}

/// Like `truncateString` in utils.ts, counting UTF-16 units like JS.
fn truncate(text: &str, length: usize) -> String {
    if text.encode_utf16().count() <= length {
        return text.to_owned();
    }
    let mut units = 0;
    let kept: String = text
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= length - 3
        })
        .collect();
    format!("{kept}...")
}

/// Like `sanitizeActivityText` in utils.ts: Discord wants 2 to 128 characters.
fn sanitize(input: Option<&str>) -> String {
    let text = input.map(str::trim).filter(|t| !t.is_empty()).unwrap_or("undefined");
    let mut text = truncate(text, 128);
    if text.encode_utf16().count() < 2 {
        text.push('⠀');
    }
    text
}

/// Like `padHangulFields` in utils.ts.
fn pad_short(value: &str) -> String {
    match value.encode_utf16().count() {
        1 => format!("{value}{HANGUL_FILLER}"),
        _ => value.to_owned(),
    }
}

/// `buildActivityInfo` plus the RPC client's `setActivity` formatting.
fn build_activity(song: &SongInfo, config: &Value, now_ms: f64) -> Value {
    let title = pad_short(&song.title);
    let artist = pad_short(&song.artist);
    let album = song.album.as_deref().map(pad_short);

    let mut activity = json!({
        "type": 2, // Listening
        "created_at": now_ms as u64,
        "instance": false,
        "details": sanitize(Some(song.alternative_title.as_deref().unwrap_or(&title))),
        "state": sanitize(Some(song.tags.first().map(String::as_str).unwrap_or(&artist))),
    });
    if let Some(display_type) = config.get("statusDisplayType").filter(|v| !v.is_null()) {
        activity["status_display_type"] = display_type.clone();
    }
    if let Some(url) = song.url.as_deref().filter(|u| !u.is_empty()) {
        activity["details_url"] = json!(url);
    }
    if let Some(url) = song.artist_url.as_deref().filter(|u| !u.is_empty()) {
        activity["state_url"] = json!(url);
    }

    let mut assets = serde_json::Map::new();
    if let Some(image) = song.image_src.as_deref().filter(|i| !i.is_empty()) {
        assets.insert("large_image".into(), json!(image));
    }
    let large_text = if song.is_paused == Some(true) {
        Some("⏸︎".to_owned())
    } else {
        album.filter(|a| !a.is_empty()).map(|a| sanitize(Some(&a)))
    };
    if let Some(text) = large_text {
        assets.insert("large_text".into(), json!(text));
    }
    if !assets.is_empty() {
        activity["assets"] = Value::Object(assets);
    }

    let hide_duration = config["hideDurationLeft"].as_bool().unwrap_or(false);
    if song.is_paused != Some(true) && !hide_duration && song.song_duration > 0.0 {
        if let Some(elapsed) = song.elapsed_seconds {
            let start = now_ms - elapsed * 1000.0;
            activity["timestamps"] = json!({
                "start": (start / 1000.0).floor() as i64,
                "end": ((start + song.song_duration * 1000.0) / 1000.0).floor() as i64,
            });
        }
    }

    let mut buttons = Vec::new();
    if config[PLAY_ON_KEY].as_bool().unwrap_or(true) {
        if let Some(url) = song.url.as_deref().filter(|u| !u.is_empty()) {
            buttons.push(json!({ "label": format!("Play on {APPLICATION_NAME}"), "url": url }));
        }
    }
    if !config["hideGitHubButton"].as_bool().unwrap_or(false) {
        buttons.push(json!({
            "label": "View App On GitHub",
            "url": "https://github.com/pear-devs/pear-desktop",
        }));
    }
    if !buttons.is_empty() {
        activity["buttons"] = json!(buttons);
    }
    activity
}

async fn write_frame(writer: &mut Writer, op: u32, payload: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(payload)?;
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&op.to_le_bytes());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    writer.write_all(&frame).await?;
    writer.flush().await
}

async fn read_frames(
    mut reader: Box<dyn AsyncRead + Send + Unpin>,
    generation: u64,
    commands: mpsc::UnboundedSender<Command>,
) {
    loop {
        let mut header = [0u8; 8];
        if reader.read_exact(&mut header).await.is_err() {
            break;
        }
        let op = u32::from_le_bytes(header[..4].try_into().unwrap());
        let length = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        let mut body = vec![0u8; length];
        if reader.read_exact(&mut body).await.is_err() {
            break;
        }
        let payload = serde_json::from_slice(&body).unwrap_or(Value::Null);
        if commands.send(Command::Incoming(generation, Some((op, payload)))).is_err() {
            return;
        }
    }
    let _ = commands.send(Command::Incoming(generation, None));
}

type Halves = (Box<dyn AsyncRead + Send + Unpin>, Writer);

/// Discord listens on `discord-ipc-0` to `-9`, the first free one.
#[cfg(windows)]
async fn open_socket() -> std::io::Result<Halves> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let mut last_error = None;
    for index in 0..10 {
        match ClientOptions::new().open(format!(r"\\.\pipe\discord-ipc-{index}")) {
            Ok(pipe) => {
                let (reader, writer) = tokio::io::split(pipe);
                return Ok((Box::new(reader), Box::new(writer)));
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.expect("tried at least one pipe"))
}

#[cfg(unix)]
async fn open_socket() -> std::io::Result<Halves> {
    let base = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .iter()
        .find_map(|key| std::env::var(key).ok())
        .unwrap_or_else(|| "/tmp".to_owned());
    let mut last_error = None;
    for index in 0..10 {
        match tokio::net::UnixStream::connect(format!("{base}/discord-ipc-{index}")).await {
            Ok(stream) => {
                let (reader, writer) = stream.into_split();
                return Ok((Box::new(reader), Box::new(writer)));
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.expect("tried at least one socket"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> SongInfo {
        SongInfo {
            title: "Song".into(),
            artist: "Artist".into(),
            album: Some("A".into()),
            video_id: "abc".into(),
            url: Some("https://music.youtube.com/watch?v=abc".into()),
            image_src: Some("https://i.ytimg.com/x.jpg".into()),
            song_duration: 200.0,
            elapsed_seconds: Some(20.0),
            is_paused: Some(false),
            ..Default::default()
        }
    }

    #[test]
    fn activity_matches_the_electron_payload() {
        let config = json!({ "statusDisplayType": 2, PLAY_ON_KEY: true, "hideGitHubButton": false });
        let activity = build_activity(&song(), &config, 1_000_000.0);
        assert_eq!(activity["type"], 2);
        assert_eq!(activity["details"], "Song");
        assert_eq!(activity["state"], "Artist");
        assert_eq!(activity["status_display_type"], 2);
        assert_eq!(activity["assets"]["large_text"], format!("A{HANGUL_FILLER}"));
        assert_eq!(activity["timestamps"], json!({ "start": 980, "end": 1180 }));
        assert_eq!(activity["buttons"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn paused_activity_has_no_timestamps() {
        let mut song = song();
        song.is_paused = Some(true);
        let activity = build_activity(&song, &json!({ "hideGitHubButton": true, PLAY_ON_KEY: false }), 0.0);
        assert_eq!(activity["assets"]["large_text"], "⏸︎");
        assert!(activity.get("timestamps").is_none());
        assert!(activity.get("buttons").is_none());
    }

    #[test]
    fn text_is_sanitized_like_utils_ts() {
        assert_eq!(sanitize(Some("  ")), "undefined");
        assert_eq!(sanitize(Some("x")), "x⠀");
        let long = "a".repeat(200);
        assert_eq!(sanitize(Some(&long)), format!("{}...", "a".repeat(125)));
    }
}
