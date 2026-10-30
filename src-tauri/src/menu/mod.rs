//! The native app menu. Port of `src/menu.ts` (core items) plus the
//! "Plugins" submenu, whose per-plugin items come from the page
//! (`src/inject/menu-runtime.ts` evaluates each plugin's `menu(ctx)` and sends a
//! serialized template). Clicks on plugin items go back to the page as
//! `peard:menu-event`, where the original click handler runs.
//!
//! Labels are the `main.menu.*` i18n keys; the page sends their translations.
//! Tauri has no radio items, so radios are check items in emulated groups.
//!
//! Android has no native menu: only the Electron-shaped tree is built, for
//! in-app-menu to draw, and check states are kept here (`CheckItem`).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde::Deserialize;
use serde_json::{json, Value};
#[cfg(desktop)]
use tauri::{
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, MenuItemKind, PredefinedMenuItem, Submenu},
    Wry,
};
use tauri::{AppHandle, Manager};

#[cfg(desktop)]
type CheckItem = CheckMenuItem<Wry>;
#[cfg(desktop)]
type NativeMenu = Menu<Wry>;

/// A check item's state where there is no native menu, with the calls of
/// `CheckMenuItem` that this module uses.
#[cfg(mobile)]
#[derive(Clone)]
struct CheckItem(Arc<std::sync::atomic::AtomicBool>);
#[cfg(mobile)]
impl CheckItem {
    fn new(checked: bool) -> Self {
        Self(Arc::new(checked.into()))
    }
    fn is_checked(&self) -> tauri::Result<bool> {
        Ok(self.0.load(std::sync::atomic::Ordering::Relaxed))
    }
    fn set_checked(&self, checked: bool) -> tauri::Result<()> {
        self.0.store(checked, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }
}
#[cfg(mobile)]
type NativeMenu = ();

#[cfg(desktop)]
use crate::ipc::MAIN_WEBVIEW;
use crate::{
    app_controls,
    config::Config,
    ipc,
    page,
};

mod actions;
mod builder;

use actions::dispatch;
use builder::build;

/// One item of a plugin's serialized Electron menu template.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TemplateItem {
    command_id: Option<u64>,
    label: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    checked: Option<bool>,
    enabled: Option<bool>,
    visible: Option<bool>,
    accelerator: Option<String>,
    submenu: Option<Vec<TemplateItem>>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct PluginEntry {
    id: String,
    name: String,
    enabled: bool,
    /// False while the plugin's backend is not ported (greyed out)
    #[serde(default = "available_by_default")]
    available: bool,
    submenu: Option<Vec<TemplateItem>>,
}

fn available_by_default() -> bool {
    true
}

/// Plugins whose switch is greyed out: on Android in-app-menu is the only
/// menu, so turning it off would leave no way back but adb.
fn always_on(plugin: &str) -> bool {
    cfg!(mobile) && plugin == "in-app-menu"
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct MenuData {
    plugins: Vec<PluginEntry>,
    /// `main.menu.*` key → translated label
    labels: HashMap<String, String>,
    /// Keys of `startingPages` in `src/providers/extracted-data.ts`
    starting_pages: Vec<String>,
    /// `(code, native name)`
    languages: Vec<(String, String)>,
}

#[derive(Default)]
pub struct MenuState {
    data: Mutex<MenuData>,
    checks: Mutex<HashMap<String, CheckItem>>,
    /// Item id → ids of every item in its radio group (itself included)
    radio_groups: Mutex<HashMap<String, Arc<Vec<String>>>>,
    /// The same menu as Electron's `Menu` JSON, for in-app-menu's `get-menu`
    tree: Mutex<Vec<Value>>,
    #[cfg_attr(mobile, allow(dead_code))]
    zoom: Mutex<f64>,
    /// Set when in-app-menu draws the menu in the page (no native menu bar)
    pub in_app: std::sync::atomic::AtomicBool,
}

/// Builds and installs the menu from the last data the page sent.
///
/// On Windows/Linux it goes on the main window only: an app-wide menu is also
/// attached to prompt windows, and closing one tore the shared menu down.
pub fn rebuild(app: &AppHandle) {
    let data = app.state::<MenuState>().data.lock().unwrap().clone();
    let menu = match build(app, &data) {
        Ok(menu) => menu,
        Err(error) => return log::error!("building the menu failed: {error}"),
    };
    if cfg!(mobile) || app.state::<MenuState>().in_app.load(std::sync::atomic::Ordering::Relaxed) {
        // in-app-menu draws it in the page; keep the built items for its clicks
        ipc::send_to_page(app, "refresh-in-app-menu", &[]);
        return;
    }
    #[cfg(mobile)]
    let _ = menu;
    #[cfg(desktop)]
    install(app, menu);
}

#[cfg(desktop)]
fn install(app: &AppHandle, menu: NativeMenu) {
    #[cfg(target_os = "macos")]
    let installed = app.set_menu(menu).map(|_| ());
    #[cfg(not(target_os = "macos"))]
    let installed = match app.get_webview_window(MAIN_WEBVIEW) {
        Some(window) => window.set_menu(menu).map(|_| ()),
        None => Ok(()),
    };
    if let Err(error) = installed {
        log::error!("installing the menu failed: {error}");
    }
}

/// The menu as Electron's `Menu` JSON (`{ items }`), with current check states.
pub fn electron_menu(app: &AppHandle) -> Value {
    let state = app.state::<MenuState>();
    let tree = state.tree.lock().unwrap().clone();
    let checks = check_items(app);
    let items: Vec<Value> = tree.iter().map(|node| with_checks(node, &checks)).collect();
    json!({ "items": items })
}

/// The check items, cloned out of the lock. Item calls made off the main
/// thread wait for it, and the main thread takes the lock when rebuilding,
/// so the lock must never be held across one.
fn check_items(app: &AppHandle) -> HashMap<String, CheckItem> {
    app.state::<MenuState>().checks.lock().unwrap().clone()
}

fn check_item(app: &AppHandle, id: &str) -> Option<CheckItem> {
    app.state::<MenuState>().checks.lock().unwrap().get(id).cloned()
}

fn with_checks(node: &Value, checks: &HashMap<String, CheckItem>) -> Value {
    let mut node = node.clone();
    if let Some(item) = node["id"].as_str().and_then(|id| checks.get(id)) {
        node["checked"] = json!(item.is_checked().unwrap_or(false));
    }
    if let Some(items) = node["submenu"]["items"].as_array() {
        let items: Vec<Value> = items.iter().map(|child| with_checks(child, checks)).collect();
        node["submenu"]["items"] = json!(items);
    }
    node
}

fn find_by_command_id(nodes: &[Value], command_id: u64) -> Option<Value> {
    nodes.iter().find_map(|node| {
        if node["commandId"].as_u64() == Some(command_id) {
            return Some(node.clone());
        }
        find_by_command_id(node["submenu"]["items"].as_array()?, command_id)
    })
}

/// Like `get-menu-by-id`: one item as Electron JSON, or null.
pub fn electron_item(app: &AppHandle, command_id: u64) -> Value {
    let state = app.state::<MenuState>();
    let found = find_by_command_id(&state.tree.lock().unwrap(), command_id);
    found.map(|node| with_checks(&node, &check_items(app))).unwrap_or(Value::Null)
}

/// A click from in-app-menu (`peard:menu-event`). No native item toggled
/// itself, so check items are toggled here before the shared dispatch.
pub fn click_by_command_id(app: &AppHandle, command_id: u64) {
    let state = app.state::<MenuState>();
    let Some(node) = find_by_command_id(&state.tree.lock().unwrap(), command_id) else { return };
    let Some(id) = node["id"].as_str().filter(|id| !id.is_empty()).map(str::to_owned) else { return };
    if node["type"] == "checkbox" {
        if let Some(item) = check_item(app, &id) {
            let _ = item.set_checked(!item.is_checked().unwrap_or(false));
        }
    }
    dispatch(app, &id);
}

/// After a click: radios stay checked and uncheck their group; check items
/// report their (already toggled) state.
fn settle_check(app: &AppHandle, id: &str) -> Option<bool> {
    let checks = check_items(app);
    let group = app.state::<MenuState>().radio_groups.lock().unwrap().get(id).cloned();
    if let Some(group) = group {
        for member in group.iter() {
            if let Some(item) = checks.get(member) {
                let _ = item.set_checked(member == id);
            }
        }
        return Some(true);
    }
    checks.get(id).and_then(|item| item.is_checked().ok())
}

/// `config.setMenuOption`: also restarts when `restartOnConfigChanges` is on.
pub fn set_menu_option(app: &AppHandle, key: &str, value: Value) {
    let config = app.state::<Config>();
    config.set(app, key, value);
    if config.get_bool("options.restartOnConfigChanges") {
        app_controls::restart(app);
    }
}

#[cfg(desktop)]
pub fn handle_event(app: &AppHandle, event: MenuEvent) {
    dispatch(app, event.id().as_ref());
}

/// A translated label from the page (`main.menu.*`, `main.tray.*`), or
/// `fallback` before the page has sent them.
pub fn label(app: &AppHandle, key: &str, fallback: &str) -> String {
    app.state::<MenuState>()
        .data
        .lock()
        .unwrap()
        .labels
        .get(key)
        .cloned()
        .unwrap_or_else(|| fallback.to_owned())
}

#[tauri::command]
pub fn menu_update(app: AppHandle, data: MenuData) {
    log::info!(
        "menu: {} plugins ({} with a submenu), {} labels",
        data.plugins.len(),
        data.plugins.iter().filter(|p| p.submenu.is_some()).count(),
        data.labels.len()
    );
    let state = app.state::<MenuState>();
    let labels_changed = {
        let mut current = state.data.lock().unwrap();
        let changed = current.labels != data.labels;
        *current = data;
        changed
    };
    rebuild(&app);
    if labels_changed {
        #[cfg(desktop)]
        crate::tray::relabel(&app);
    }
}

/// A plugin click handler changed `item.checked` (e.g. reset after a
/// cancelled prompt); mirror it on the native item.
#[tauri::command]
pub fn menu_set_checked(app: AppHandle, command_id: u64, checked: bool) {
    if let Some(item) = check_item(&app, &format!("cmd:{command_id}")) {
        let _ = item.set_checked(checked);
    }
}
