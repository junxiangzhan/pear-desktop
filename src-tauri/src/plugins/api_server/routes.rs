//! The REST routes and the WebSocket of the API, on axum.

use super::*;
use auth::{authorize, authorized, guard};

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

pub(super) fn router(shared: Arc<Shared>) -> Router {
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
                    include_str!("../../../api-docs/openapi.json"),
                )
            }),
        )
        .route(
            "/swagger",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/html; charset=UTF-8")],
                    include_str!("../../../api-docs/swagger.html"),
                )
            }),
        )
        .layer(middleware::from_fn(cors))
        .with_state(shared)
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
