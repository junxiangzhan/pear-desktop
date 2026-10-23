//! Port of `src/providers/song-controls.ts`: player commands are messages to
//! the page, handled in `src/renderer.ts` once the player API is ready.
//! Requests that Electron answered with `ipcMain.once` wait with a timeout,
//! so a page that never answers cannot hang the caller.
#![allow(dead_code)] // the full API is kept for plugins ported in later rounds

use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::ipc::{send_to_page, Ipc};

const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

fn send(app: &AppHandle, channel: &str, args: &[Value]) {
    send_to_page(app, channel, args);
}

pub fn previous(app: &AppHandle) {
    send(app, "peard:previous-video", &[]);
}
pub fn next(app: &AppHandle) {
    send(app, "peard:next-video", &[]);
}
pub fn play(app: &AppHandle) {
    send(app, "peard:play", &[]);
}
pub fn pause(app: &AppHandle) {
    send(app, "peard:pause", &[]);
}
pub fn play_pause(app: &AppHandle) {
    send(app, "peard:toggle-play", &[]);
}
pub fn like(app: &AppHandle) {
    send(app, "peard:update-like", &[json!("LIKE")]);
}
pub fn dislike(app: &AppHandle) {
    send(app, "peard:update-like", &[json!("DISLIKE")]);
}
pub fn seek_to(app: &AppHandle, seconds: f64) {
    send(app, "peard:seek-to", &[json!(seconds)]);
}
pub fn go_back(app: &AppHandle, seconds: f64) {
    send(app, "peard:seek-by", &[json!(-seconds)]);
}
pub fn go_forward(app: &AppHandle, seconds: f64) {
    send(app, "peard:seek-by", &[json!(seconds)]);
}
pub fn shuffle(app: &AppHandle) {
    send(app, "peard:shuffle", &[]);
}
pub fn switch_repeat(app: &AppHandle, times: u32) {
    send(app, "peard:switch-repeat", &[json!(times)]);
}
pub fn set_volume(app: &AppHandle, volume: f64) {
    send(app, "peard:update-volume", &[json!(volume)]);
}
pub fn mute_unmute(app: &AppHandle) {
    send(app, "peard:toggle-mute", &[]);
}

pub fn set_fullscreen(app: &AppHandle, fullscreen: bool) {
    if let Some(window) = app.get_webview_window(crate::ipc::MAIN_WEBVIEW) {
        let _ = window.set_fullscreen(fullscreen);
    }
    send(app, "peard:click-fullscreen-button", &[json!(fullscreen)]);
}

/// Electron synthesized a `/` keydown with `webContents.sendInputEvent`.
pub fn open_search_box(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(crate::ipc::MAIN_WEBVIEW) {
        let _ = window.eval(
            "document.dispatchEvent(new KeyboardEvent('keydown', { key: '/', code: 'Slash', bubbles: true }))",
        );
    }
}

pub fn add_song_to_queue(app: &AppHandle, video_id: &str, position: &str) {
    send(app, "peard:add-to-queue", &[json!(video_id), json!(position)]);
}
pub fn move_song_in_queue(app: &AppHandle, from: u32, to: u32) {
    send(app, "peard:move-in-queue", &[json!(from), json!(to)]);
}
pub fn remove_song_from_queue(app: &AppHandle, index: u32) {
    send(app, "peard:remove-from-queue", &[json!(index)]);
}
pub fn set_queue_index(app: &AppHandle, index: u32) {
    send(app, "peard:set-queue-index", &[json!(index)]);
}
pub fn clear_queue(app: &AppHandle) {
    send(app, "peard:clear-queue", &[]);
}

/// Sends `request` and waits for the page's `response` message.
async fn ask(app: &AppHandle, request: &str, args: &[Value], response: &str) -> Option<Vec<Value>> {
    let ipc = app.state::<Ipc>();
    let answer = ipc.wait_for(response, ANSWER_TIMEOUT);
    send(app, request, args);
    answer.await
}

pub async fn is_shuffled(app: &AppHandle) -> Option<bool> {
    ask(app, "peard:get-shuffle", &[], "peard:get-shuffle-response")
        .await?
        .first()?
        .as_bool()
}

pub async fn is_fullscreen(app: &AppHandle) -> Option<bool> {
    ask(app, "peard:get-fullscreen", &[], "peard:set-fullscreen")
        .await?
        .first()?
        .as_bool()
}

pub async fn queue(app: &AppHandle) -> Option<Value> {
    ask(app, "peard:get-queue", &[], "peard:get-queue-response")
        .await?
        .into_iter()
        .next()
}

pub async fn search(
    app: &AppHandle,
    query: &str,
    params: Option<&str>,
    continuation: Option<&str>,
) -> Option<Value> {
    ask(
        app,
        "peard:search",
        &[json!(query), json!(params), json!(continuation)],
        "peard:search-results",
    )
    .await?
    .into_iter()
    .next()
}
