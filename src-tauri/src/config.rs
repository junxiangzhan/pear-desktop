//! The app config (`config.json`), owned by Rust. Port of `src/config/*` and
//! of the config watcher in `src/index.ts#initHook`.
//!
//! The page keeps a synchronous mirror (`src/config/store.ts`): it is
//! seeded from the payload and replaced by every `pear:config` broadcast.

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};

use serde::Deserialize;
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager};
use tokio::sync::watch;

use crate::{
    ipc::{self, arg, Ipc},
    plugins::Plugins,
};

/// Port of `src/config/defaults.ts`.
pub fn defaults() -> Value {
    json!({
        "window-size": { "width": 1100, "height": 550 },
        "window-maximized": false,
        "window-position": { "x": -1, "y": -1 },
        "url": "https://music.youtube.com",
        "options": {
            "tray": false,
            "appVisible": true,
            "autoUpdates": true,
            "alwaysOnTop": false,
            "hideMenu": false,
            "hideMenuWarned": false,
            "startAtLogin": false,
            "disableHardwareAcceleration": false,
            "removeUpgradeButton": false,
            "restartOnConfigChanges": false,
            "trayClickPlayPause": false,
            "autoResetAppCache": false,
            "resumeOnStart": true,
            "likeButtons": "",
            "swapLikeButtonsOrder": false,
            "proxy": "",
            "startingPage": "",
            "overrideUserAgent": false,
            "usePodcastParticipantAsArtist": false,
            "themes": []
        },
        "plugins": {}
    })
}

/// One plugin as reported by the page (`src/inject/plugin-stubs.ts`).
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    pub id: String,
    pub config: Value,
    pub restart_needed: bool,
}

pub struct Config {
    path: PathBuf,
    store: Mutex<Value>,
    manifest: Mutex<HashMap<String, ManifestEntry>>,
    manifest_ready: watch::Sender<bool>,
}

/// Deep merge where `b` wins; arrays are replaced, not concatenated
/// (`deepmergeCustom({ mergeArrays: false })` in `src/config/index.ts`).
/// A `Null` `b` means "nothing stored" (the JS callers pass `get(...) ?? {}`),
/// so it keeps `a`.
pub fn merge(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (a, Value::Null) => a.clone(),
        (Value::Object(a), Value::Object(b)) => {
            let mut out = a.clone();
            for (key, value) in b {
                let merged = match out.get(key) {
                    Some(existing) => merge(existing, value),
                    None => value.clone(),
                };
                out.insert(key.clone(), merged);
            }
            Value::Object(out)
        }
        (_, b) => b.clone(),
    }
}

fn get_path<'a>(root: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.').try_fold(root, |node, part| node.get(part))
}

fn set_path(root: &mut Value, key: &str, value: Value) {
    let mut node = root;
    let mut parts = key.split('.').peekable();
    while let Some(part) = parts.next() {
        if !node.is_object() {
            *node = Value::Object(Map::new());
        }
        let map = node.as_object_mut().unwrap();
        if parts.peek().is_none() {
            map.insert(part.to_owned(), value);
            return;
        }
        node = map.entry(part.to_owned()).or_insert_with(|| Value::Object(Map::new()));
    }
}

/// Plugins whose backend is not ported yet: always reported as disabled.
pub fn unavailable_plugins() -> &'static HashSet<String> {
    static LIST: OnceLock<HashSet<String>> = OnceLock::new();
    LIST.get_or_init(|| {
        #[derive(Deserialize)]
        struct Support {
            unavailable: Vec<String>,
        }
        let support: Support = serde_json::from_str(include_str!("../../plugin-support.json"))
            .expect("plugin-support.json is valid");
        support.unavailable.into_iter().collect()
    })
}

/// The stored config, or `{}` when there is none. A file that exists but
/// isn't a JSON object (a write cut short, a hand edit gone wrong) is moved
/// aside to `config.json.bad`; otherwise the next save would replace it
/// with the defaults and the settings would be gone without a trace.
fn read_stored(path: &Path) -> Value {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return json!({}),
        Err(error) => {
            log::error!("reading {} failed: {error}", path.display());
            return json!({});
        }
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(stored @ Value::Object(_)) => stored,
        result => {
            let bad = path.with_extension("json.bad");
            let reason = result.err().map_or("not an object".to_owned(), |e| e.to_string());
            let shown = path.display();
            match fs::rename(path, &bad) {
                Ok(()) => log::error!("{shown} is unreadable ({reason}); kept as {}", bad.display()),
                Err(error) => log::error!("{shown} is unreadable ({reason}); moving it failed: {error}"),
            }
            json!({})
        }
    }
}

/// Replaces `path` without ever leaving it half-written: the text goes to a
/// temporary file next to it, which is flushed and then renamed over it.
fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("json.tmp");
    let mut file = fs::File::create(&temp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path)
}

impl Config {
    pub fn load(path: PathBuf) -> Self {
        let stored = read_stored(&path);
        Self {
            path,
            store: Mutex::new(merge(&defaults(), &stored)),
            manifest: Mutex::new(HashMap::new()),
            manifest_ready: watch::Sender::new(false),
        }
    }

    pub fn snapshot(&self) -> Value {
        self.store.lock().unwrap().clone()
    }

    pub fn get(&self, key: &str) -> Value {
        get_path(&self.store.lock().unwrap(), key)
            .cloned()
            .unwrap_or(Value::Null)
    }

    pub fn get_bool(&self, key: &str) -> bool {
        self.get(key).as_bool().unwrap_or(false)
    }

    pub fn get_str(&self, key: &str) -> String {
        self.get(key).as_str().unwrap_or_default().to_owned()
    }

    /// Saves the store. Called with the store locked, so saves land in order.
    fn persist(&self, store: &Value) {
        match serde_json::to_string_pretty(store) {
            Ok(text) => {
                if let Err(error) = write_atomically(&self.path, &text) {
                    log::error!("writing {} failed: {error}", self.path.display());
                }
            }
            Err(error) => log::error!("serializing the config failed: {error}"),
        }
    }

    /// Like `config.set`. Runs the plugin watcher and syncs the page mirror.
    pub fn set(&self, app: &AppHandle, key: &str, value: Value) {
        let (before, after) = {
            let mut store = self.store.lock().unwrap();
            let before = store.clone();
            set_path(&mut store, key, value);
            self.persist(&store);
            (before, store.clone())
        };
        self.after_change(app, &before, &after);
    }

    /// Like `config.setPartial`: deep-merges `value` over the current value
    /// (itself over `default`).
    pub fn set_partial(&self, app: &AppHandle, key: &str, value: &Value, default: Option<&Value>) {
        let current = self.get(key);
        let base = match default {
            Some(default) => merge(default, &current),
            None => current,
        };
        self.set(app, key, merge(&base, value));
    }

    pub fn plugin_default(&self, id: &str) -> Value {
        self.manifest
            .lock()
            .unwrap()
            .get(id)
            .map(|entry| entry.config.clone())
            .unwrap_or_else(|| json!({ "enabled": false }))
    }

    /// The plugin's defaults merged with its stored options, like the
    /// `peard:get-config` handler in `src/index.ts`.
    pub fn plugin_config(&self, id: &str) -> Value {
        let stored = self.get(&format!("plugins.{id}"));
        let mut config = merge(&self.plugin_default(id), &stored);
        if unavailable_plugins().contains(id) {
            config["enabled"] = Value::Bool(false);
        }
        config
    }

    pub fn plugin_enabled(&self, id: &str) -> bool {
        self.plugin_config(id)["enabled"].as_bool().unwrap_or(false)
    }

    pub fn set_manifest(&self, entries: Vec<ManifestEntry>) {
        let mut manifest = self.manifest.lock().unwrap();
        for mut entry in entries {
            // The Tauri downloader saves the source audio by default:
            // Electron's default "mp3 (256kbps)" re-encodes it into a file
            // twice as large without being better (plugins/downloader)
            if entry.id == "downloader" {
                entry.config["selectedPreset"] = json!("Source");
            }
            manifest.insert(entry.id.clone(), entry);
        }
        drop(manifest);
        self.manifest_ready.send_replace(true);
    }

    /// Plugin defaults come from the page; config reads made before the
    /// manifest arrives wait for it (bounded, so a broken page cannot hang them).
    pub async fn wait_for_manifest(&self) {
        let mut ready = self.manifest_ready.subscribe();
        let _ = tokio::time::timeout(Duration::from_secs(10), ready.wait_for(|ready| *ready)).await;
    }

    /// The watcher from `src/index.ts#initHook`, per changed plugin:
    /// enable/unload on an `enabled` flip, then `onConfigChange` and
    /// `config-changed`. Always ends by syncing the page mirror.
    fn after_change(&self, app: &AppHandle, before: &Value, after: &Value) {
        let plugins = app.state::<Plugins>();
        let ids: HashSet<String> = [before, after]
            .iter()
            .filter_map(|store| store.get("plugins").and_then(Value::as_object))
            .flat_map(|map| map.keys().cloned())
            .collect();

        for id in ids {
            let old_stored = before.get("plugins").and_then(|p| p.get(&id));
            let new_stored = after.get("plugins").and_then(|p| p.get(&id));
            if old_stored == new_stored {
                continue;
            }
            let default = self.plugin_default(&id);
            let enabled_in = |stored: Option<&Value>| {
                !unavailable_plugins().contains(&id)
                    && merge(&default, stored.unwrap_or(&Value::Null))["enabled"]
                        .as_bool()
                        .unwrap_or(false)
            };
            let (was_enabled, is_enabled) = (enabled_in(old_stored), enabled_in(new_stored));
            let config = self.plugin_config(&id);

            if was_enabled != is_enabled {
                let channel = if is_enabled { "plugin:enable" } else { "plugin:unload" };
                ipc::send_to_page(app, channel, &[json!(id)]);
                if is_enabled {
                    plugins.start(app, &id);
                } else {
                    plugins.stop(app, &id);
                }
                let restart_needed = self
                    .manifest
                    .lock()
                    .unwrap()
                    .get(&id)
                    .is_some_and(|entry| entry.restart_needed);
                if restart_needed {
                    crate::dialogs::need_restart(app, &id);
                }
            }
            if is_enabled {
                plugins.config_changed(app, &id, &config);
            }
            ipc::send_to_page(app, "config-changed", &[json!(id), config]);
        }

        crate::page::emit(app, "pear:config", after);
    }
}

/// Registers the config IPC used by the renderer's plugin contexts.
pub fn register_ipc(ipc: &Ipc) {
    ipc.handle("core", "peard:get-config", |app, args| async move {
        let id: String = arg(&args, 0)?;
        let config = app.state::<Config>();
        config.wait_for_manifest().await;
        Ok(config.plugin_config(&id))
    });
    ipc.handle("core", "peard:set-config", |app, args| async move {
        let id: String = arg(&args, 0)?;
        let value: Value = arg(&args, 1)?;
        let config = app.state::<Config>();
        config.wait_for_manifest().await;
        let default = config.plugin_default(&id);
        config.set_partial(&app, &format!("plugins.{id}"), &value, Some(&default));
        Ok(Value::Null)
    });
}

#[tauri::command]
pub fn config_set(app: AppHandle, key: String, value: Value) {
    app.state::<Config>().set(&app, &key, value);
}

#[tauri::command]
pub fn config_set_partial(app: AppHandle, key: String, value: Value, default_value: Option<Value>) {
    app.state::<Config>()
        .set_partial(&app, &key, &value, default_value.as_ref());
}

#[tauri::command]
pub fn config_edit(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = app.state::<Config>().path.clone();
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn plugins_manifest(app: AppHandle, manifest: Vec<ManifestEntry>) {
    app.state::<Config>().set_manifest(manifest);
    app.state::<Plugins>().start_enabled(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_replaces_arrays_and_merges_objects() {
        let merged = merge(
            &json!({ "a": { "x": 1, "y": [1, 2] }, "b": 1 }),
            &json!({ "a": { "y": [3] }, "c": 2 }),
        );
        assert_eq!(merged, json!({ "a": { "x": 1, "y": [3] }, "b": 1, "c": 2 }));
    }

    #[test]
    fn merge_with_nothing_stored_keeps_defaults() {
        let defaults = json!({ "enabled": true, "speed": 1 });
        assert_eq!(merge(&defaults, &Value::Null), defaults);
    }

    #[test]
    fn set_path_creates_intermediate_objects() {
        let mut root = json!({});
        set_path(&mut root, "plugins.foo.enabled", json!(true));
        assert_eq!(root, json!({ "plugins": { "foo": { "enabled": true } } }));
        assert_eq!(get_path(&root, "plugins.foo.enabled"), Some(&json!(true)));
    }

    /// A fresh, empty directory under the system temp dir for one test.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("pear-tauri-config-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_config_reads_as_empty() {
        let dir = temp_dir("missing");
        assert_eq!(read_stored(&dir.join("config.json")), json!({}));
        assert!(!dir.join("config.json.bad").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unreadable_config_is_kept_aside() {
        let dir = temp_dir("bad");
        let path = dir.join("config.json");
        fs::write(&path, r#"{ "options": { "tray": tr"#).unwrap();
        assert_eq!(read_stored(&path), json!({}));
        assert!(!path.exists());
        assert_eq!(
            fs::read_to_string(dir.join("config.json.bad")).unwrap(),
            r#"{ "options": { "tray": tr"#
        );

        fs::write(&path, "null").unwrap();
        assert_eq!(read_stored(&path), json!({}));
        assert_eq!(fs::read_to_string(dir.join("config.json.bad")).unwrap(), "null");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn atomic_write_replaces_the_file_and_leaves_no_temp_file() {
        let dir = temp_dir("write");
        let path = dir.join("nested").join("config.json");
        write_atomically(&path, r#"{"a":1}"#).unwrap();
        write_atomically(&path, r#"{"a":2}"#).unwrap();
        assert_eq!(read_stored(&path), json!({ "a": 2 }));
        assert!(!path.with_extension("json.tmp").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn plugin_support_lists_backend_plugins() {
        assert!(!unavailable_plugins().contains("downloader"));
        assert!(!unavailable_plugins().contains("discord"));
    }
}
