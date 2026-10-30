//! IPC between the page (`window.ipcRenderer`, see `src/platform/ipc.ts`) and Rust.
//!
//! Plays the role of Electron's `ipcMain` + `webContents.send`:
//! - page `send(channel, ...args)` → `ipc_send` → every listener on `channel`
//! - page `invoke(channel, ...args)` → `ipc_invoke` → the one handler on `channel`
//! - Rust → page: [`send_to_page`] emits `pear:ipc` to the page
//!
//! Listeners and handlers carry an owner (`"core"` or a plugin id) so a
//! plugin's registrations can be dropped when it stops. They decode their
//! arguments with [`arg`] (or [`arg_or_warn`] for listeners).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::future::BoxFuture;
use serde::{de::DeserializeOwned, Serialize};
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

/// Argument `index` of a page message, decoded into `T`. A missing argument
/// decodes like `null` (what the page sends for `undefined`), so `Option<T>`
/// makes it optional and any other type makes it required.
pub fn arg<T: DeserializeOwned>(args: &[Value], index: usize) -> Result<T, String> {
    T::deserialize(args.get(index).unwrap_or(&Value::Null))
        .map_err(|error| format!("argument {index}: {error}"))
}

/// [`arg`] for listeners, which have no caller to answer: a bad argument
/// is logged and the message skipped.
pub fn arg_or_warn<T: DeserializeOwned>(channel: &str, args: &[Value], index: usize) -> Option<T> {
    arg(args, index)
        .map_err(|error| log::warn!("ignoring '{channel}': {error}"))
        .ok()
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn arg_decodes_by_type() {
        let args = [json!("downloader"), json!({ "enabled": true }), json!(3)];
        assert_eq!(arg::<String>(&args, 0).unwrap(), "downloader");
        assert_eq!(arg::<Value>(&args, 1).unwrap(), json!({ "enabled": true }));
        assert_eq!(arg::<u64>(&args, 2).unwrap(), 3);
    }

    #[test]
    fn missing_arguments_are_only_allowed_when_optional() {
        let args = [json!(null)];
        assert_eq!(arg::<Option<String>>(&args, 0).unwrap(), None);
        assert_eq!(arg::<Option<String>>(&args, 5).unwrap(), None);
        assert_eq!(arg::<Value>(&args, 5).unwrap(), Value::Null);
        assert!(arg::<String>(&args, 0).unwrap_err().starts_with("argument 0:"));
        assert!(arg::<u64>(&[json!("7")], 0).is_err());
        assert_eq!(arg_or_warn::<bool>("test", &[], 0), None);
    }
}
