//! IPC between the page (`window.ipcRenderer`, see `src/platform/ipc.ts`) and Rust.
//!
//! Plays the role of Electron's `ipcMain` + `webContents.send`:
//! - page `send(channel, ...args)` → `ipc_send` → every listener on `channel`
//! - page `invoke(channel, ...args)` → `ipc_invoke` → the one handler on `channel`
//! - Rust → page: [`send_to_page`] emits `pear:ipc` to the page
//!
//! Listeners and handlers carry an owner (`"core"` or a plugin id) so a
//! plugin's registrations can be dropped when it stops.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::future::BoxFuture;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::sync::oneshot;

pub const MAIN_WEBVIEW: &str = "main";

pub type Listener = Arc<dyn Fn(&AppHandle, &[Value]) + Send + Sync>;
pub type Handler =
    Arc<dyn Fn(AppHandle, Vec<Value>) -> BoxFuture<'static, Result<Value, String>> + Send + Sync>;

#[derive(Default)]
pub struct Ipc {
    listeners: Mutex<HashMap<String, Vec<(String, Listener)>>>,
    handlers: Mutex<HashMap<String, (String, Handler)>>,
    waiters: Mutex<HashMap<String, Vec<oneshot::Sender<Vec<Value>>>>>,
}

#[derive(Serialize, Clone)]
struct PageMessage<'a> {
    channel: &'a str,
    args: &'a [Value],
}

impl Ipc {
    /// Like `ipcMain.on`.
    pub fn on(
        &self,
        owner: &str,
        channel: &str,
        listener: impl Fn(&AppHandle, &[Value]) + Send + Sync + 'static,
    ) {
        self.listeners
            .lock()
            .unwrap()
            .entry(channel.to_owned())
            .or_default()
            .push((owner.to_owned(), Arc::new(listener)));
    }

    /// Like `ipcMain.handle`; replaces an existing handler on the channel.
    pub fn handle<F, Fut>(&self, owner: &str, channel: &str, handler: F)
    where
        F: Fn(AppHandle, Vec<Value>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Value, String>> + Send + 'static,
    {
        let handler: Handler = Arc::new(move |app, args| Box::pin(handler(app, args)));
        self.handlers
            .lock()
            .unwrap()
            .insert(channel.to_owned(), (owner.to_owned(), handler));
    }

    /// Drops every listener and handler registered by `owner`.
    pub fn remove_owner(&self, owner: &str) {
        for list in self.listeners.lock().unwrap().values_mut() {
            list.retain(|(o, _)| o != owner);
        }
        self.handlers.lock().unwrap().retain(|_, (o, _)| o != owner);
    }

    /// Resolves with the args of the next page `send` on `channel`, or `None`
    /// after `timeout`. Used where Electron did `ipcMain.once(...)` and waited
    /// for the renderer to answer (search, queue, shuffle, fullscreen).
    ///
    /// The waiter is registered when this is called, not when the future is
    /// first polled, so call it before sending the request it waits on.
    pub fn wait_for(
        &self,
        channel: &str,
        timeout: Duration,
    ) -> impl std::future::Future<Output = Option<Vec<Value>>> + Send + 'static {
        let (tx, rx) = oneshot::channel();
        self.waiters
            .lock()
            .unwrap()
            .entry(channel.to_owned())
            .or_default()
            .push(tx);
        async move { tokio::time::timeout(timeout, rx).await.ok()?.ok() }
    }

    /// Delivers a page `send` to waiters and listeners. Also used by Rust code
    /// that wants to raise a channel internally (like Electron's `ipcMain.emit`).
    pub fn dispatch(&self, app: &AppHandle, channel: &str, args: &[Value]) {
        if let Some(waiters) = self.waiters.lock().unwrap().remove(channel) {
            for waiter in waiters {
                let _ = waiter.send(args.to_vec());
            }
        }
        // Clone out of the lock: listeners may register or remove listeners
        let listeners: Vec<Listener> = self
            .listeners
            .lock()
            .unwrap()
            .get(channel)
            .map(|list| list.iter().map(|(_, l)| l.clone()).collect())
            .unwrap_or_default();
        for listener in listeners {
            listener(app, args);
        }
    }

    async fn invoke(&self, app: AppHandle, channel: &str, args: Vec<Value>) -> Result<Value, String> {
        let handler = self
            .handlers
            .lock()
            .unwrap()
            .get(channel)
            .map(|(_, h)| h.clone());
        match handler {
            Some(handler) => handler(app, args).await,
            None => Err(format!("No handler registered for '{channel}'")),
        }
    }
}

/// Like `webContents.send(channel, ...args)`.
pub fn send_to_page(app: &AppHandle, channel: &str, args: &[Value]) {
    crate::page::emit(app, "pear:ipc", PageMessage { channel, args });
}

#[tauri::command]
pub fn ipc_send(app: AppHandle, channel: String, args: Vec<Value>) {
    app.state::<Ipc>().dispatch(&app, &channel, &args);
}

#[tauri::command]
pub async fn ipc_invoke(app: AppHandle, channel: String, args: Vec<Value>) -> Result<Value, String> {
    let ipc = app.state::<Ipc>();
    ipc.invoke(app.clone(), &channel, args).await
}
