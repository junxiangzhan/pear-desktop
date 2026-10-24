//! IPC with the page when it runs in GeckoView (Android, GECKOVIEW-PLAN.md).
//!
//! The page has no `__TAURI_INTERNALS__` there: `inject/tauri.ts` sends
//! `{ id, cmd, args }` to the built-in extension (assets/pear-gecko), which
//! forwards it over native messaging to `GeckoBridgePlugin` (Kotlin), and
//! that over a [`Channel`] to here. Commands run the same functions as the
//! Tauri commands in `lib.rs`; the answer `{ id, ok | err }`, events
//! `{ event, payload }` and scripts `{ eval }` go back with the plugin's
//! `deliver`.
//!
//! Two extension ports talk to Rust: `page` (the content script in the YTM
//! tab) and `background` (which keeps the loader's config snapshot current).

use std::sync::mpsc;

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{
    ipc::{Channel, InvokeResponseBody},
    plugin::{Builder, PluginHandle, TauriPlugin},
    AppHandle, Manager, Wry,
};

use crate::{app_controls, config, ipc, menu, net, prompt, window};

const PAGE: &str = "page";
const BACKGROUND: &str = "background";

/// Messages to Kotlin, delivered in order by one thread: plugin calls block
/// until Kotlin answers, and the per-second song updates must not reorder.
struct Outbox(mpsc::Sender<(&'static str, Value)>);

/// For calls that answer, outside the delivery order.
struct Plugin(PluginHandle<Wry>);

#[derive(Deserialize)]
struct Request {
    id: Option<u64>,
    cmd: String,
    #[serde(default)]
    args: Value,
}

pub fn init() -> TauriPlugin<Wry> {
    Builder::new("pear-gecko-bridge")
        .setup(|app, api| {
            let handle: PluginHandle<Wry> =
                api.register_android_plugin("com.github.pear_devs.tauri_poc", "GeckoBridgePlugin")?;
            app.manage(Plugin(handle.clone()));
            let (sender, receiver) = mpsc::channel::<(&'static str, Value)>();
            std::thread::Builder::new()
                .name("gecko-bridge".into())
                .spawn(move || {
                    for (command, args) in receiver {
                        if let Err(error) = handle.run_mobile_plugin::<()>(command, args) {
                            log::warn!("gecko bridge: {command} failed: {error}");
                        }
                    }
                })?;
            app.manage(Outbox(sender));
            Ok(())
        })
        .build()
}

/// Gives Kotlin the channel for messages from the extension.
pub fn register(app: &AppHandle) {
    let channel: Channel = Channel::new({
        let app = app.clone();
        move |body| {
            let message: Value = match body {
                InvokeResponseBody::Json(json) => serde_json::from_str(&json).unwrap_or_default(),
                InvokeResponseBody::Raw(raw) => serde_json::from_slice(&raw).unwrap_or_default(),
            };
            let port = message["port"].as_str().unwrap_or(PAGE).to_owned();
            match serde_json::from_str::<Request>(message["message"].as_str().unwrap_or_default()) {
                Ok(request) => handle(&app, port, request),
                Err(error) => log::warn!("gecko bridge: bad message from {port}: {error}"),
            }
            Ok(())
        }
    });
    let _ = app.state::<Outbox>().0.send(("setChannel", json!({ "channel": channel })));
}

fn send(app: &AppHandle, port: &str, message: Value) {
    let delivery = json!({ "port": port, "message": message.to_string() });
    if app.state::<Outbox>().0.send(("deliver", delivery)).is_err() {
        log::warn!("gecko bridge: the delivery thread is gone");
    }
}

/// Like `emit_to(MAIN_WEBVIEW, …)`. The background script also needs the
/// config, for the snapshot the next page load starts with.
pub fn emit(app: &AppHandle, event: &str, payload: impl Serialize) {
    let message = json!({ "event": event, "payload": payload });
    if event == "pear:config" {
        send(app, BACKGROUND, message.clone());
    }
    send(app, PAGE, message);
}

/// The URL GeckoView shows (blocks until Kotlin answers).
pub fn page_url(app: &AppHandle) -> Option<String> {
    #[derive(Deserialize)]
    struct PageUrl {
        url: Option<String>,
    }
    match app.state::<Plugin>().0.run_mobile_plugin::<PageUrl>("pageUrl", json!({})) {
        Ok(answer) => answer.url,
        Err(error) => {
            log::warn!("gecko bridge: reading the page URL failed: {error}");
            None
        }
    }
}

pub fn eval(app: &AppHandle, script: &str) {
    let delivery = json!({ "port": PAGE, "eval": script });
    let _ = app.state::<Outbox>().0.send(("deliver", delivery));
}

fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    serde_json::from_value(args.get(name).cloned().unwrap_or(Value::Null))
        .map_err(|error| format!("argument '{name}': {error}"))
}

/// Runs a command; synchronous ones in order on the calling (UI) thread, as
/// Tauri runs sync commands on the main thread, async ones on the runtime.
fn handle(app: &AppHandle, port: String, request: Request) {
    let Request { id, cmd, args } = request;
    let reply = move |app: &AppHandle, cmd: &str, result: Result<Value, String>| {
        if let Err(error) = &result {
            log::warn!("gecko bridge: {cmd} failed: {error}");
        }
        let Some(id) = id else { return };
        let message = match result {
            Ok(value) => json!({ "id": id, "ok": value }),
            Err(error) => json!({ "id": id, "err": error }),
        };
        send(app, &port, message);
    };
    if !ASYNC_COMMANDS.contains(&cmd.as_str()) {
        let result = run_sync(app.clone(), &cmd, &args);
        return reply(app, &cmd, result);
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = run_async(&app, &cmd, args).await;
        reply(&app, &cmd, result);
    });
}

const ASYNC_COMMANDS: [&str; 4] = ["ipc_invoke", "plugins_manifest", "net_fetch", "prompt_open"];

fn run_sync(app: AppHandle, cmd: &str, args: &Value) -> Result<Value, String> {
    match cmd {
        "ipc_send" => ipc::ipc_send(app, arg(args, "channel")?, arg(args, "args")?),
        "config_set" => config::config_set(app, arg(args, "key")?, arg(args, "value")?),
        "config_set_partial" => config::config_set_partial(
            app,
            arg(args, "key")?,
            arg(args, "value")?,
            arg(args, "defaultValue")?,
        ),
        "config_edit" => config::config_edit(app)?,
        "app_restart" => app_controls::app_restart(app),
        "menu_update" => menu::menu_update(app, arg(args, "data")?),
        "menu_set_checked" => menu::menu_set_checked(app, arg(args, "commandId")?, arg(args, "checked")?),
        "window_set_always_on_top" => window::window_set_always_on_top(app, arg(args, "flag")?)?,
        "window_start_dragging" => window::window_start_dragging(app)?,
        "window_toggle_maximize" => window::window_toggle_maximize(app)?,
        // Rust pushes every event over the bridge (see `emit`)
        "plugin:event|listen" => {}
        "gecko_payload" => return Ok(payload(&app)),
        _ => return Err(format!("unknown command '{cmd}'")),
    }
    Ok(Value::Null)
}

async fn run_async(app: &AppHandle, cmd: &str, args: Value) -> Result<Value, String> {
    match cmd {
        "ipc_invoke" => ipc::ipc_invoke(app.clone(), arg(&args, "channel")?, arg(&args, "args")?).await,
        "plugins_manifest" => {
            config::plugins_manifest(app.clone(), arg(&args, "manifest")?).await;
            Ok(Value::Null)
        }
        "net_fetch" => {
            let response = net::net_fetch(arg(&args, "url")?, arg(&args, "init")?).await?;
            serde_json::to_value(response).map_err(|e| e.to_string())
        }
        "prompt_open" => prompt::prompt_open(app.clone(), arg(&args, "options")?).await,
        _ => Err(format!("unknown command '{cmd}'")),
    }
}

/// What the loader puts in the page before the bundle runs (the Tauri
/// version is `lib.rs#payload`), plus the URL to start at.
fn payload(app: &AppHandle) -> Value {
    let config = app.state::<config::Config>();
    let unavailable: Vec<&String> = config::unavailable_plugins().iter().collect();
    json!({
        "config": config.snapshot(),
        "unavailable": unavailable,
        "startUrl": crate::start_url(&config),
    })
}
