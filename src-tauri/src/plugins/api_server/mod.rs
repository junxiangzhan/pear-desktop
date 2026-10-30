//! Port of the `api-server` backend (src/plugins/api-server/backend/): the
//! same REST + WebSocket API and JWT auth, on axum instead of Hono.
//!
//! The OpenAPI document (`/doc`) and Swagger UI page (`/swagger`) are
//! static files in `api-docs/`. The document was generated from the
//! Electron backend's zod route schemas (see the `electron-reference` tag);
//! update it by hand when a route changes. Like Electron's, the page loads
//! Swagger UI from jsDelivr.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Path, Query, Request, State,
    },
    http::{header, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::Sha256;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tokio::sync::{broadcast, oneshot};

use super::Plugin;
use crate::{
    config::Config,
    ipc::{arg_or_warn, send_to_page, Ipc},
    menu::label,
    song_controls,
    song_info::{SongInfo, SongInfoEvent, SongInfoProvider},
};

mod auth;
mod routes;

pub(super) use auth::cors;
use routes::router;

type AppState = State<Arc<Shared>>;

const ID: &str = "api-server";
const AUTH_AT_FIRST: &str = "AUTH_AT_FIRST";
const NONE: &str = "NONE";

/// What the server listens on; a config change touching it restarts the server.
#[derive(Clone, PartialEq, Debug)]
struct Bind {
    hostname: String,
    port: u16,
    https: Option<(String, String)>,
}

impl Bind {
    fn from_config(config: &Value) -> Self {
        let cert = config["certPath"].as_str().unwrap_or_default();
        let key = config["keyPath"].as_str().unwrap_or_default();
        let https = config["useHttps"].as_bool().unwrap_or(false) && !cert.is_empty() && !key.is_empty();
        Self {
            hostname: config["hostname"].as_str().unwrap_or("0.0.0.0").to_owned(),
            port: config["port"].as_u64().and_then(|p| u16::try_from(p).ok()).unwrap_or(26538),
            https: https.then(|| (cert.to_owned(), key.to_owned())),
        }
    }
}

/// Player state the routes and sockets report, kept from page events.
struct Shared {
    app: AppHandle,
    song: Mutex<Option<SongInfo>>,
    repeat: Mutex<Option<Value>>,
    volume: Mutex<Option<Value>>,
    shuffle: AtomicBool,
    /// Messages for every open WebSocket
    events: broadcast::Sender<String>,
}

impl Shared {
    fn config(&self) -> Value {
        self.app.state::<Config>().plugin_config(ID)
    }

    /// Like `send()` in websocket.ts; `null` fields are left out as JS
    /// leaves out `undefined` ones.
    fn broadcast(&self, kind: &str, state: Value) {
        let _ = self.events.send(message(kind, state));
    }

    fn player_state(&self) -> Value {
        let song = self.song.lock().unwrap().clone();
        let volume = self.volume.lock().unwrap().clone().unwrap_or(Value::Null);
        json!({
            "isPlaying": song.as_ref().is_some_and(|s| !s.is_paused.unwrap_or(false)),
            "muted": volume["isMuted"].as_bool().unwrap_or(false),
            "position": song.as_ref().and_then(|s| s.elapsed_seconds).unwrap_or(0.0),
            "volume": volume["state"].as_f64().unwrap_or(100.0),
            "repeat": self.repeat.lock().unwrap().clone().unwrap_or(json!("NONE")),
            "shuffle": self.shuffle.load(Ordering::Relaxed),
            "song": song,
        })
    }
}

fn message(kind: &str, state: Value) -> String {
    let mut object = Map::new();
    object.insert("type".into(), json!(kind));
    if let Value::Object(fields) = state {
        object.extend(fields.into_iter().filter(|(_, v)| !v.is_null()));
    }
    Value::Object(object).to_string()
}

struct Running {
    handle: axum_server::Handle<SocketAddr>,
    bind: Bind,
}

pub struct ApiServer {
    running: Mutex<Option<Running>>,
    shared: Mutex<Option<Arc<Shared>>>,
}

impl ApiServer {
    pub fn new() -> Self {
        Self { running: Mutex::new(None), shared: Mutex::new(None) }
    }

    /// Like `run()`: errors are only logged, as in Electron.
    fn run(&self, shared: Arc<Shared>, bind: Bind) {
        let handle = axum_server::Handle::new();
        *self.running.lock().unwrap() = Some(Running { handle: handle.clone(), bind: bind.clone() });
        tauri::async_runtime::spawn(async move {
            if let Err(error) = serve(shared, bind.clone(), handle).await {
                log::error!("api-server: serving on {}:{} failed: {error}", bind.hostname, bind.port);
            }
        });
    }

    /// Like `end()`.
    fn end(&self) {
        if let Some(running) = self.running.lock().unwrap().take() {
            running.handle.shutdown();
        }
    }
}

impl Plugin for ApiServer {
    fn id(&self) -> &'static str {
        ID
    }

    fn start(&self, app: &AppHandle, config: &Value) {
        let (events, _) = broadcast::channel(64);
        let shared = Arc::new(Shared {
            app: app.clone(),
            song: Mutex::new(None),
            repeat: Mutex::new(None),
            volume: Mutex::new(None),
            shuffle: AtomicBool::new(false),
            events,
        });
        *self.shared.lock().unwrap() = Some(shared.clone());

        super::request_player_listeners(
            app,
            ID,
            &["seeked", "time-changed", "repeat-changed", "like-changed", "volume-changed", "shuffle-changed"],
        );

        let state = shared.clone();
        app.state::<SongInfoProvider>().register(ID, move |_, song, event| {
            *state.song.lock().unwrap() = Some(song.clone());
            match event {
                SongInfoEvent::VideoSrcChanged => {
                    state.broadcast("VIDEO_CHANGED", json!({ "song": song, "position": 0 }))
                }
                SongInfoEvent::PlayOrPaused => state.broadcast(
                    "PLAYER_STATE_CHANGED",
                    json!({ "isPlaying": song.is_paused == Some(false), "position": song.elapsed_seconds }),
                ),
                SongInfoEvent::TimeChanged => {
                    state.broadcast("POSITION_CHANGED", json!({ "position": song.elapsed_seconds }))
                }
            }
        });

        let ipc = app.state::<Ipc>();
        let state = shared.clone();
        ipc.on(ID, "peard:volume-changed", move |_, args| {
            let Some(volume) = arg_or_warn::<Value>("peard:volume-changed", args, 0) else { return };
            state.broadcast(
                "VOLUME_CHANGED",
                json!({ "volume": volume["state"], "muted": volume["isMuted"] }),
            );
            *state.volume.lock().unwrap() = Some(volume);
        });
        let state = shared.clone();
        ipc.on(ID, "peard:repeat-changed", move |_, args| {
            let Some(mode) = arg_or_warn::<Value>("peard:repeat-changed", args, 0) else { return };
            state.broadcast("REPEAT_CHANGED", json!({ "repeat": mode }));
            *state.repeat.lock().unwrap() = Some(mode);
        });
        let state = shared.clone();
        ipc.on(ID, "peard:seeked", move |_, args| {
            let Some(position) = arg_or_warn::<f64>("peard:seeked", args, 0) else { return };
            state.broadcast("POSITION_CHANGED", json!({ "position": position }));
        });
        let state = shared.clone();
        ipc.on(ID, "peard:shuffle-changed", move |_, args| {
            let Some(shuffle) = arg_or_warn::<bool>("peard:shuffle-changed", args, 0) else { return };
            state.shuffle.store(shuffle, Ordering::Relaxed);
            state.broadcast("SHUFFLE_CHANGED", json!({ "shuffle": shuffle }));
        });

        // The default secret is `Date.now()` at page load; keep the first
        // one so tokens survive reloads and restarts.
        if app.state::<Config>().get("plugins.api-server.secret").is_null() {
            let (app, secret) = (app.clone(), config["secret"].clone());
            tauri::async_runtime::spawn(async move {
                app.state::<Config>().set(&app, "plugins.api-server.secret", secret);
            });
        }

        self.run(shared, Bind::from_config(config));
    }

    fn stop(&self, _app: &AppHandle) {
        self.end();
        self.shared.lock().unwrap().take();
    }

    fn on_config_change(&self, _app: &AppHandle, config: &Value) {
        let bind = Bind::from_config(config);
        let same = self.running.lock().unwrap().as_ref().is_some_and(|r| r.bind == bind);
        if same {
            return;
        }
        self.end();
        if let Some(shared) = self.shared.lock().unwrap().clone() {
            self.run(shared, bind);
        }
    }
}

async fn serve(shared: Arc<Shared>, bind: Bind, handle: axum_server::Handle<SocketAddr>) -> Result<(), String> {
    let addr = tokio::net::lookup_host((bind.hostname.as_str(), bind.port))
        .await
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("hostname did not resolve")?;
    let service = router(shared).into_make_service_with_connect_info::<SocketAddr>();
    log::info!("api-server: listening on {addr} ({})", if bind.https.is_some() { "https" } else { "http" });
    match bind.https {
        Some((cert, key)) => {
            let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key)
                .await
                .map_err(|e| e.to_string())?;
            axum_server::bind_rustls(addr, tls).handle(handle).serve(service).await
        }
        None => axum_server::bind(addr).handle(handle).serve(service).await,
    }
    .map_err(|e| e.to_string())
}
