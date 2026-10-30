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

type AppState = State<Arc<Shared>>;

/// A POST that only forwards a player command (204, like the Hono routes).
macro_rules! command {
    ($action:expr) => {
        post(|State(s): AppState| async move {
            $action(&s.app);
            StatusCode::NO_CONTENT
        })
    };
}

#[derive(Deserialize)]
struct Seconds {
    seconds: f64,
}

#[derive(Deserialize)]
struct Iteration {
    iteration: f64,
}

#[derive(Deserialize)]
struct Volume {
    volume: f64,
}

#[derive(Deserialize)]
struct Fullscreen {
    state: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddToQueue {
    video_id: String,
    #[serde(default = "insert_at_end")]
    insert_position: InsertPosition,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum InsertPosition {
    InsertAtEnd,
    InsertAfterCurrentVideo,
}

fn insert_at_end() -> InsertPosition {
    InsertPosition::InsertAtEnd
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveInQueue {
    to_index: f64,
}

#[derive(Deserialize)]
struct QueueIndex {
    index: u32,
}

#[derive(Deserialize)]
struct Search {
    query: String,
    params: Option<String>,
    continuation: Option<String>,
}

fn router(shared: Arc<Shared>) -> Router {
    let api = Router::new()
        .route("/previous", command!(song_controls::previous))
        .route("/next", command!(song_controls::next))
        .route("/play", command!(song_controls::play))
        .route("/pause", command!(song_controls::pause))
        .route("/toggle-play", command!(song_controls::play_pause))
        .route("/like-state", get(like_state))
        .route("/like", command!(song_controls::like))
        .route("/dislike", command!(song_controls::dislike))
        .route(
            "/seek-to",
            post(|State(s): AppState, Json(body): Json<Seconds>| async move {
                song_controls::seek_to(&s.app, body.seconds);
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/go-back",
            post(|State(s): AppState, Json(body): Json<Seconds>| async move {
                song_controls::go_back(&s.app, body.seconds);
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/go-forward",
            post(|State(s): AppState, Json(body): Json<Seconds>| async move {
                song_controls::go_forward(&s.app, body.seconds);
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/shuffle",
            get(|State(s): AppState| async move {
                let state = song_controls::is_shuffled(&s.app).await.unwrap_or(false);
                Json(json!({ "state": state }))
            })
            .post(|State(s): AppState| async move {
                song_controls::shuffle(&s.app);
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/repeat-mode",
            get(|State(s): AppState| async move {
                Json(json!({ "mode": s.repeat.lock().unwrap().clone() }))
            }),
        )
        .route(
            "/switch-repeat",
            post(|State(s): AppState, Json(body): Json<Iteration>| async move {
                song_controls::switch_repeat(&s.app, body.iteration.max(0.0) as u32);
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/volume",
            get(|State(s): AppState| async move {
                let volume = s.volume.lock().unwrap().clone();
                Json(volume.unwrap_or(json!({ "state": 0, "isMuted": false })))
            })
            .post(|State(s): AppState, Json(body): Json<Volume>| async move {
                song_controls::set_volume(&s.app, body.volume);
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/fullscreen",
            get(|State(s): AppState| async move {
                let state = song_controls::is_fullscreen(&s.app).await.unwrap_or(false);
                Json(json!({ "state": state }))
            })
            .post(|State(s): AppState, Json(body): Json<Fullscreen>| async move {
                song_controls::set_fullscreen(&s.app, body.state);
                StatusCode::NO_CONTENT
            }),
        )
        .route("/toggle-mute", command!(song_controls::mute_unmute))
        .route("/song-info", get(song_info))
        .route("/song", get(song_info))
        .route("/queue-info", get(queue_info))
        .route("/queue/next", get(next_song))
        .route(
            "/queue",
            get(queue_info)
                .post(|State(s): AppState, Json(body): Json<AddToQueue>| async move {
                    let position = match body.insert_position {
                        InsertPosition::InsertAtEnd => "INSERT_AT_END",
                        InsertPosition::InsertAfterCurrentVideo => "INSERT_AFTER_CURRENT_VIDEO",
                    };
                    song_controls::add_song_to_queue(&s.app, &body.video_id, position);
                    StatusCode::NO_CONTENT
                })
                .patch(|State(s): AppState, Json(body): Json<QueueIndex>| async move {
                    song_controls::set_queue_index(&s.app, body.index);
                    StatusCode::NO_CONTENT
                })
                .delete(|State(s): AppState| async move {
                    song_controls::clear_queue(&s.app);
                    StatusCode::NO_CONTENT
                }),
        )
        .route(
            "/queue/{index}",
            axum::routing::patch(
                |State(s): AppState, Path(index): Path<u32>, Json(body): Json<MoveInQueue>| async move {
                    song_controls::move_song_in_queue(&s.app, index, body.to_index.max(0.0) as u32);
                    StatusCode::NO_CONTENT
                },
            )
            .delete(|State(s): AppState, Path(index): Path<u32>| async move {
                song_controls::remove_song_from_queue(&s.app, index);
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/search",
            post(|State(s): AppState, Json(body): Json<Search>| async move {
                let results = song_controls::search(
                    &s.app,
                    &body.query,
                    body.params.as_deref(),
                    body.continuation.as_deref(),
                )
                .await;
                Json(results.unwrap_or(Value::Null))
            }),
        )
        .route_layer(middleware::from_fn_with_state(shared.clone(), guard))
        // Authenticates with a `token` query parameter instead
        .route("/ws", get(websocket));

    Router::new()
        .route("/auth/{id}", post(authorize))
        .nest("/api/v1", api)
        .route(
            "/doc",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "application/json")],
                    include_str!("../../api-docs/openapi.json"),
                )
            }),
        )
        .route(
            "/swagger",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/html; charset=UTF-8")],
                    include_str!("../../api-docs/swagger.html"),
                )
            }),
        )
        .layer(middleware::from_fn(cors))
        .with_state(shared)
}

/// Hono's `cors()` defaults plus `Access-Control-Allow-Private-Network`
/// (for web remote controls).
pub(super) async fn cors(request: Request, next: Next) -> Response {
    let mut response = if request.method() == Method::OPTIONS {
        let mut response = StatusCode::NO_CONTENT.into_response();
        let headers = response.headers_mut();
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET,HEAD,PUT,POST,DELETE,PATCH"),
        );
        if let Some(requested) = request.headers().get(header::ACCESS_CONTROL_REQUEST_HEADERS) {
            headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, requested.clone());
            headers.insert(header::VARY, HeaderValue::from_static("Access-Control-Request-Headers"));
        }
        response
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    headers.insert("access-control-allow-private-network", HeaderValue::from_static("true"));
    response
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, "Unauthorized").into_response()
}

/// Whether `token` belongs to an authorized client (or auth is off).
fn authorized(config: &Value, token: Option<&str>) -> bool {
    if config["authStrategy"] == NONE {
        return true;
    }
    let secret = config["secret"].as_str().unwrap_or_default();
    let Some(payload) = token.and_then(|token| jwt::verify(token, secret)) else { return false };
    let clients = config["authorizedClients"].as_array();
    payload["id"].as_str().is_some_and(|id| clients.is_some_and(|c| c.iter().any(|c| c == id)))
}

/// The `jwt()` middleware and the authorized-client check on `/api/*`.
async fn guard(State(s): AppState, request: Request, next: Next) -> Response {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_owned);
    if !authorized(&s.config(), token.as_deref()) {
        return unauthorized();
    }
    next.run(request).await
}

/// `POST /auth/{id}`: asks the user (AUTH_AT_FIRST), then issues a token.
async fn authorize(
    State(s): AppState,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
) -> Response {
    let config = s.config();
    let mut clients: Vec<Value> = config["authorizedClients"].as_array().cloned().unwrap_or_default();
    let known = clients.iter().any(|c| c == id.as_str());
    if !known && config["authStrategy"] == AUTH_AT_FIRST && !ask_user(&s.app, &id, remote).await {
        return (StatusCode::FORBIDDEN, ()).into_response();
    }
    if !known {
        clients.push(json!(id));
        s.app.state::<Config>().set(&s.app, "plugins.api-server.authorizedClients", json!(clients));
    }
    let payload = json!({ "id": id, "iat": now_seconds() });
    let token = jwt::sign(&payload, config["secret"].as_str().unwrap_or_default());
    Json(json!({ "accessToken": token })).into_response()
}

/// The Allow/Deny message box; translations come from the page.
async fn ask_user(app: &AppHandle, id: &str, remote: SocketAddr) -> bool {
    let key = "plugins.api-server.dialog.request";
    let message = label(app, &format!("{key}.message"), "Allow {ID} ({origin}) to access the API?")
        .replace("{ID}", id)
        .replace("{origin}", &remote.ip().to_string());
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .message(message)
        .title(label(app, &format!("{key}.title"), "API authorization request"))
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            label(app, &format!("{key}.buttons.allow"), "Allow"),
            label(app, &format!("{key}.buttons.deny"), "Deny"),
        ))
        .show(move |allowed| {
            let _ = tx.send(allowed);
        });
    rx.await.unwrap_or(false)
}

fn now_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

async fn song_info(State(s): AppState) -> Response {
    match s.song.lock().unwrap().clone() {
        Some(song) => Json(song).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// Electron read the like button's state with `executeJavaScript`; the page
/// answers `pear:get-like-state` (src/inject/plugin-glue.ts).
async fn like_state(State(s): AppState) -> Json<Value> {
    let answer = s.app.state::<Ipc>().wait_for("pear:like-state", Duration::from_secs(5));
    send_to_page(&s.app, "pear:get-like-state", &[]);
    let state = answer.await.and_then(|args| args.into_iter().next()).unwrap_or(Value::Null);
    Json(json!({ "state": state }))
}

async fn queue_info(State(s): AppState) -> Response {
    match song_controls::queue(&s.app).await.filter(|q| !q.is_null()) {
        Some(queue) => Json(queue).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// The item after the selected one, in a short SongInfo-like shape.
fn next_in_queue(queue: &Value) -> Option<Value> {
    let renderer = |item: &Value| -> Option<Value> {
        let direct = &item["playlistPanelVideoRenderer"];
        let wrapped = &item["playlistPanelVideoWrapperRenderer"]["primaryRenderer"]["playlistPanelVideoRenderer"];
        [direct, wrapped].into_iter().find(|r| r.is_object()).cloned()
    };
    let items = queue["items"].as_array().filter(|items| !items.is_empty())?;
    // Like JS `findIndex`: -1 when nothing is selected, so the first item is next
    let current = items
        .iter()
        .position(|item| renderer(item).is_some_and(|r| r["selected"] == true))
        .map_or(0, |i| i + 1);
    let next = renderer(items.get(current)?)?;
    Some(json!({
        "title": next["title"]["runs"][0]["text"],
        "videoId": next["videoId"],
        "thumbnail": next["thumbnail"],
        "lengthText": next["lengthText"],
        "shortBylineText": next["shortBylineText"],
    }))
}

async fn next_song(State(s): AppState) -> Response {
    let queue = song_controls::queue(&s.app).await.unwrap_or(Value::Null);
    match next_in_queue(&queue) {
        Some(next) => Json(next).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

async fn websocket(
    State(s): AppState,
    Query(query): Query<HashMap<String, String>>,
    upgrade: WebSocketUpgrade,
) -> Response {
    upgrade.on_upgrade(move |socket| socket_session(s, socket, query.get("token").cloned()))
}

async fn socket_session(s: Arc<Shared>, mut socket: WebSocket, token: Option<String>) {
    if !authorized(&s.config(), token.as_deref()) {
        let close = CloseFrame { code: 1008, reason: "Unauthorized".into() };
        let _ = socket.send(Message::Close(Some(close))).await;
        return;
    }
    let mut events = s.events.subscribe();
    if socket.send(Message::Text(message("PLAYER_INFO", s.player_state()).into())).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(text) => {
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }
}

/// HS256 tokens, as `hono/jwt` signs and verifies them.
mod jwt {
    use super::*;

    fn mac(secret: &str) -> Hmac<Sha256> {
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes any key length")
    }

    pub fn sign(payload: &Value, secret: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256","typ":"JWT"}"#);
        let body = URL_SAFE_NO_PAD.encode(payload.to_string());
        let mut mac = mac(secret);
        mac.update(format!("{header}.{body}").as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        format!("{header}.{body}.{signature}")
    }

    /// The payload of a valid token: HS256 signature, and `nbf`/`exp`/`iat`
    /// checked like `hono/jwt`'s `verify`.
    pub fn verify(token: &str, secret: &str) -> Option<Value> {
        let (signed, signature) = token.rsplit_once('.')?;
        let (header, body) = signed.split_once('.')?;
        let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).ok()?).ok()?;
        if header["alg"] != "HS256" {
            return None;
        }
        let mut mac = mac(secret);
        mac.update(signed.as_bytes());
        mac.verify_slice(&URL_SAFE_NO_PAD.decode(signature).ok()?).ok()?;
        let payload: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(body).ok()?).ok()?;
        let now = now_seconds() as f64;
        let after = |key: &str| payload[key].as_f64().is_some_and(|t| t > now);
        let before = |key: &str| payload[key].as_f64().is_some_and(|t| t <= now);
        if after("nbf") || before("exp") || after("iat") {
            return None;
        }
        Some(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_round_trip_and_reject_tampering() {
        let token = jwt::sign(&json!({ "id": "client", "iat": now_seconds() }), "secret");
        assert_eq!(jwt::verify(&token, "secret").unwrap()["id"], "client");
        assert!(jwt::verify(&token, "other").is_none());
        let mut tampered = token.clone();
        tampered.insert(token.find('.').unwrap() + 2, 'x');
        assert!(jwt::verify(&tampered, "secret").is_none());
    }

    #[test]
    fn tokens_issued_in_the_future_are_rejected() {
        let token = jwt::sign(&json!({ "id": "c", "iat": now_seconds() + 3600 }), "s");
        assert!(jwt::verify(&token, "s").is_none());
    }

    #[test]
    fn hono_tokens_verify() {
        // hono/jwt `sign({ id: 'abc', iat: 1700000000 }, 'secret')`, so tokens
        // issued by the Electron build keep working
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpZCI6ImFiYyIsImlhdCI6MTcwMDAwMDAwMH0.zM9wV94fmJaKszKyImSMjfDzvJrVXm1pccDpyXzu8to";
        assert_eq!(jwt::verify(token, "secret").unwrap()["id"], "abc");
    }

    #[test]
    fn next_song_follows_the_selected_item() {
        let item = |id: &str, selected: bool| {
            json!({ "playlistPanelVideoRenderer": { "videoId": id, "selected": selected, "title": { "runs": [{ "text": id }] } } })
        };
        let queue = json!({ "items": [item("a", false), item("b", true), item("c", false)] });
        assert_eq!(next_in_queue(&queue).unwrap()["videoId"], "c");
        let last = json!({ "items": [item("a", false), item("b", true)] });
        assert!(next_in_queue(&last).is_none());
    }

    #[test]
    fn auth_checks_the_client_list() {
        let token = jwt::sign(&json!({ "id": "c1", "iat": 0 }), "s");
        let config = json!({ "authStrategy": AUTH_AT_FIRST, "secret": "s", "authorizedClients": ["c1"] });
        assert!(authorized(&config, Some(&token)));
        assert!(!authorized(&json!({ "authStrategy": AUTH_AT_FIRST, "secret": "s", "authorizedClients": [] }), Some(&token)));
        assert!(!authorized(&config, None));
        assert!(authorized(&json!({ "authStrategy": NONE }), None));
    }
}
