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
    io::AsyncWriteExt,
    sync::mpsc,
    time::{sleep_until, Instant},
};

use super::Plugin;
use crate::{
    ipc::{send_to_page, Ipc},
    song_info::{SongInfo, SongInfoEvent, SongInfoProvider},
};

mod activity;
mod rpc;

use activity::{build_activity, is_seek};
use rpc::{
    open_socket, read_frames, write_frame, Writer, OP_CLOSE, OP_FRAME, OP_HANDSHAKE, OP_PING, OP_PONG,
};

/// Application ID registered by the pear-desktop team (constants.ts)
const CLIENT_ID: &str = "1177081335727267940";
const PROGRESS_THROTTLE: Duration = Duration::from_millis(15_000);
const TIME_UPDATE_DEBOUNCE: Duration = Duration::from_millis(5000);
const RETRY_DELAY: Duration = Duration::from_millis(5000);

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
