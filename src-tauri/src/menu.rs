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

/// A built item: the native Tauri item and its Electron-shaped JSON
/// (`commandId`, `label`, `type`, `checked`, `submenu.items`...).
struct Node {
    #[cfg(desktop)]
    native: MenuItemKind<Wry>,
    json: Value,
}

type Items = Vec<Node>;

struct Builder<'a> {
    #[cfg_attr(mobile, allow(dead_code))]
    app: &'a AppHandle,
    labels: &'a HashMap<String, String>,
    checks: HashMap<String, CheckItem>,
    radio_groups: HashMap<String, Arc<Vec<String>>>,
    next_command_id: u64,
}

impl Builder<'_> {
    fn label(&self, key: &str) -> String {
        let full = format!("main.menu.{key}");
        self.labels.get(&full).cloned().unwrap_or_else(|| {
            // Before the page sends translations: the key's last word
            key.rsplit('.').next().unwrap_or(key).replace('-', " ")
        })
    }

    fn json(&mut self, id: &str, text: &str, kind: &str, enabled: bool) -> Value {
        self.next_command_id += 1;
        json!({ "commandId": self.next_command_id, "id": id, "label": text, "type": kind, "enabled": enabled, "visible": true })
    }

    fn item(&mut self, id: &str, text: &str, enabled: bool, accelerator: Option<&str>) -> Node {
        let mut json = self.json(id, text, "normal", enabled);
        json["accelerator"] = json!(accelerator);
        Node {
            #[cfg(desktop)]
            native: MenuItem::with_id(self.app, id, text, enabled, accelerator)
                // Electron accelerators Tauri rejects are dropped, not fatal
                .or_else(|_| MenuItem::with_id(self.app, id, text, enabled, None::<&str>))
                .expect("creating a menu item")
                .kind(),
            json,
        }
    }

    fn check(&mut self, id: &str, text: &str, checked: bool, enabled: bool) -> Node {
        #[cfg(desktop)]
        let item = CheckMenuItem::with_id(self.app, id, text, enabled, checked, None::<&str>)
            .expect("creating a check menu item");
        #[cfg(mobile)]
        let item = CheckItem::new(checked);
        self.checks.insert(id.to_owned(), item.clone());
        Node {
            #[cfg(desktop)]
            native: item.kind(),
            json: self.json(id, text, "checkbox", enabled),
        }
    }

    /// Check items acting as one radio group.
    fn radios(&mut self, options: Vec<(String, String, bool)>) -> Items {
        let ids: Arc<Vec<String>> = Arc::new(options.iter().map(|(id, _, _)| id.clone()).collect());
        options
            .into_iter()
            .map(|(id, text, checked)| {
                self.radio_groups.insert(id.clone(), ids.clone());
                let mut node = self.check(&id, &text, checked, true);
                node.json["type"] = json!("radio");
                node
            })
            .collect()
    }

    fn separator(&mut self) -> Node {
        Node {
            #[cfg(desktop)]
            native: PredefinedMenuItem::separator(self.app).expect("creating a separator").kind(),
            json: self.json("", "", "separator", true),
        }
    }

    fn submenu(&mut self, text: &str, items: Items) -> Node {
        #[cfg(desktop)]
        let native = {
            let refs: Vec<&dyn IsMenuItem<Wry>> =
                items.iter().map(|i| &i.native as &dyn IsMenuItem<Wry>).collect();
            Submenu::with_items(self.app, text, true, &refs)
                .expect("creating a submenu")
                .kind()
        };
        let mut json = self.json("", text, "submenu", true);
        json["submenu"] = json!({ "items": items.into_iter().map(|n| n.json).collect::<Vec<_>>() });
        Node {
            #[cfg(desktop)]
            native,
            json,
        }
    }

    /// A plugin's serialized template. Consecutive radio items form a group.
    fn template(&mut self, items: &[TemplateItem]) -> Items {
        let mut out = Items::new();
        let mut radio_run: Vec<(String, String, bool)> = Vec::new();
        let flush = |builder: &mut Self, out: &mut Items, run: &mut Vec<(String, String, bool)>| {
            if !run.is_empty() {
                out.extend(builder.radios(std::mem::take(run)));
            }
        };
        for item in items.iter().filter(|i| i.visible != Some(false)) {
            let kind = item.kind.as_deref().unwrap_or("normal");
            let id = item.command_id.map(|c| format!("cmd:{c}")).unwrap_or_default();
            let text = item.label.clone().unwrap_or_default();
            let enabled = item.enabled != Some(false);
            if kind == "radio" {
                radio_run.push((id, text, item.checked.unwrap_or(false)));
                continue;
            }
            flush(self, &mut out, &mut radio_run);
            match kind {
                "separator" => out.push(self.separator()),
                "checkbox" => out.push(self.check(&id, &text, item.checked.unwrap_or(false), enabled)),
                "submenu" => {
                    let children = self.template(item.submenu.as_deref().unwrap_or_default());
                    out.push(self.submenu(&text, children));
                }
                _ => out.push(self.item(&id, &text, enabled, item.accelerator.as_deref())),
            }
        }
        flush(self, &mut out, &mut radio_run);
        out
    }
}

fn build(app: &AppHandle, data: &MenuData) -> tauri::Result<NativeMenu> {
    let config = app.state::<Config>();
    let mut b = Builder {
        app,
        labels: &data.labels,
        checks: HashMap::new(),
        radio_groups: HashMap::new(),
        next_command_id: 0,
    };

    // Plugins: a checkbox per plugin, or a submenu when it is enabled and has a menu
    let enabled_label = b.label("plugins.enabled");
    let mut plugin_items = Items::new();
    for plugin in &data.plugins {
        let toggle_id = format!("plugin:{}", plugin.id);
        let locked = always_on(&plugin.id);
        match &plugin.submenu {
            Some(template) if plugin.enabled => {
                let mut children = vec![b.check(&toggle_id, &enabled_label, true, !locked), b.separator()];
                children.extend(b.template(template));
                plugin_items.push(b.submenu(&plugin.name, children));
            }
            _ => plugin_items.push(b.check(&toggle_id, &plugin.name, plugin.enabled, plugin.available && !locked)),
        }
    }
    let plugins = b.submenu(&b.label("plugins.label"), plugin_items);

    // Options
    let starting_page = config.get_str("options.startingPage");
    let mut starting_pages = vec![(
        "radio:startingPage:".to_owned(),
        b.label("options.submenu.starting-page.unset"),
        starting_page.is_empty(),
    )];
    starting_pages.extend(data.starting_pages.iter().map(|name| {
        (format!("radio:startingPage:{name}"), name.clone(), &starting_page == name)
    }));
    let starting_pages = b.radios(starting_pages);

    let like_buttons = config.get_str("options.likeButtons");
    let mut like_items = b.radios(vec![
        ("radio:likeButtons:".into(), b.label("options.submenu.visual-tweaks.submenu.like-buttons.default"), like_buttons.is_empty()),
        ("radio:likeButtons:force".into(), b.label("options.submenu.visual-tweaks.submenu.like-buttons.force-show"), like_buttons == "force"),
        ("radio:likeButtons:hide".into(), b.label("options.submenu.visual-tweaks.submenu.like-buttons.hide"), like_buttons == "hide"),
    ]);
    like_items.push(b.check(
        "opt:swapLikeButtonsOrder",
        &b.label("options.submenu.visual-tweaks.submenu.like-buttons.swap"),
        config.get_bool("options.swapLikeButtonsOrder"),
        true,
    ));
    // Window, tray and keyboard items only exist on desktop
    let desktop = cfg!(desktop);
    let mut visual_tweaks = vec![
        b.check(
            "opt:removeUpgradeButton",
            &b.label("options.submenu.visual-tweaks.submenu.remove-upgrade-button"),
            config.get_bool("options.removeUpgradeButton"),
            true,
        ),
        b.submenu(&b.label("options.submenu.visual-tweaks.submenu.like-buttons.label"), like_items),
    ];
    if desktop {
        visual_tweaks.push(b.item(
            "act:customWindowTitle",
            &b.label("options.submenu.visual-tweaks.submenu.custom-window-title.label"),
            true,
            None,
        ));
    }

    let (tray, visible) = (config.get_bool("options.tray"), config.get_bool("options.appVisible"));
    let mut tray_items = b.radios(vec![
        ("radio:tray:disabled".into(), b.label("options.submenu.tray.submenu.disabled"), !tray),
        ("radio:tray:show".into(), b.label("options.submenu.tray.submenu.enabled-and-show-app"), tray && visible),
        ("radio:tray:hide".into(), b.label("options.submenu.tray.submenu.enabled-and-hide-app"), tray && !visible),
    ]);
    tray_items.push(b.separator());
    tray_items.push(b.check(
        "opt:trayClickPlayPause",
        &b.label("options.submenu.tray.submenu.play-pause-on-click"),
        config.get_bool("options.trayClickPlayPause"),
        true,
    ));

    let language = config.get_str("options.language");
    let languages = b.radios(
        data.languages
            .iter()
            .map(|(code, name)| (format!("radio:language:{code}"), name.clone(), &language == code))
            .collect(),
    );

    let mut advanced = vec![
        b.item("act:setProxy", &b.label("options.submenu.advanced-options.submenu.set-proxy.label"), true, None),
        b.check(
            "opt:restartOnConfigChanges",
            &b.label("options.submenu.advanced-options.submenu.restart-on-config-changes"),
            config.get_bool("options.restartOnConfigChanges"),
            true,
        ),
        b.separator(),
        b.item("act:editConfig", &b.label("options.submenu.advanced-options.submenu.edit-config-json"), true, None),
    ];
    if desktop {
        advanced.push(b.item(
            "act:toggleDevTools",
            &b.label("options.submenu.advanced-options.submenu.toggle-dev-tools"),
            true,
            None,
        ));
    }

    let mut options_items = vec![
        b.check("opt:resumeOnStart", &b.label("options.submenu.resume-on-start"), config.get_bool("options.resumeOnStart"), true),
        b.submenu(&b.label("options.submenu.starting-page.label"), starting_pages),
        b.submenu(&b.label("options.submenu.visual-tweaks.label"), visual_tweaks),
    ];
    if desktop {
        options_items.push(b.check(
            "opt:alwaysOnTop",
            &b.label("options.submenu.always-on-top"),
            config.get_bool("options.alwaysOnTop"),
            true,
        ));
        options_items.push(b.check(
            "opt:startAtLogin",
            &b.label("options.submenu.start-at-login"),
            config.get_bool("options.startAtLogin"),
            true,
        ));
        options_items.push(b.submenu(&b.label("options.submenu.tray.label"), tray_items));
    }
    options_items.push(b.separator());
    options_items.push(b.submenu(&b.label("options.submenu.language.label"), languages));
    options_items.push(b.submenu(&b.label("options.submenu.advanced-options.label"), advanced));
    let options = b.submenu(&b.label("options.label"), options_items);

    let mut view_items = vec![
        b.item("act:reload", &b.label("view.submenu.reload"), true, Some("CmdOrCtrl+R")),
        b.item("act:forceReload", &b.label("view.submenu.force-reload"), true, Some("CmdOrCtrl+Shift+R")),
    ];
    if desktop {
        view_items.extend([
            b.separator(),
            b.item("act:zoomIn", &b.label("view.submenu.zoom-in"), true, Some("CmdOrCtrl+Plus")),
            b.item("act:zoomOut", &b.label("view.submenu.zoom-out"), true, Some("CmdOrCtrl+-")),
            b.item("act:resetZoom", &b.label("view.submenu.reset-zoom"), true, Some("CmdOrCtrl+0")),
            b.separator(),
            b.item("act:toggleFullscreen", &b.label("view.submenu.toggle-fullscreen"), true, Some("F11")),
        ]);
    } else {
        // Hides the status and navigation bars (Android immersive mode); remembered
        view_items.extend([
            b.separator(),
            b.check(
                "opt:fullscreen",
                &b.label("view.submenu.toggle-fullscreen"),
                config.get_bool("options.fullscreen"),
                true,
            ),
        ]);
    }
    let view = b.submenu(&b.label("view.label"), view_items);

    let mut navigation_items = vec![
        b.item("act:goBack", &b.label("navigation.submenu.go-back"), true, Some("Alt+Left")),
        b.item("act:goForward", &b.label("navigation.submenu.go-forward"), true, Some("Alt+Right")),
        b.item("act:copyUrl", &b.label("navigation.submenu.copy-current-url"), true, Some("CmdOrCtrl+L")),
    ];
    navigation_items.extend([
        b.separator(),
        b.item("act:restart", &b.label("navigation.submenu.restart"), true, None),
    ]);
    // Android apps are left with Back or Home, not quit
    if desktop {
        navigation_items.push(b.item("act:quit", &b.label("navigation.submenu.quit"), true, Some("CmdOrCtrl+Q")));
    }
    let navigation = b.submenu(&b.label("navigation.label"), navigation_items);

    // A submenu like Electron's (`submenu: [{ role: 'about' }]`): in-app-menu's
    // top-level buttons only open submenus
    let about_label = b.label("about");
    let about_item = b.item("act:about", &format!("{about_label} Pear Tauri"), true, None);
    let about = b.submenu(&about_label, vec![about_item]);

    let top = [plugins, options, view, navigation, about];
    #[cfg(desktop)]
    let menu = {
        let refs: Vec<&dyn IsMenuItem<Wry>> =
            top.iter().map(|i| &i.native as &dyn IsMenuItem<Wry>).collect();
        Menu::with_items(app, &refs)?
    };

    let state = app.state::<MenuState>();
    *state.checks.lock().unwrap() = b.checks;
    *state.radio_groups.lock().unwrap() = b.radio_groups;
    *state.tree.lock().unwrap() = top.into_iter().map(|n| n.json).collect();
    #[cfg(desktop)]
    return Ok(menu);
    #[cfg(mobile)]
    return Ok(());
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
fn zoom(app: &AppHandle, change: impl FnOnce(f64) -> f64) {
    let state = app.state::<MenuState>();
    let mut zoom = state.zoom.lock().unwrap();
    if *zoom == 0.0 {
        *zoom = 1.0;
    }
    *zoom = change(*zoom).clamp(0.25, 5.0);
    if let Some(window) = app.get_webview_window(MAIN_WEBVIEW) {
        let _ = window.set_zoom(*zoom);
    }
}

#[cfg(desktop)]
pub fn handle_event(app: &AppHandle, event: MenuEvent) {
    dispatch(app, event.id().as_ref());
}

/// Runs a menu item by its id, after its check state was updated (natively
/// by a click on the menu bar, or by `click_by_command_id`).
fn dispatch(app: &AppHandle, id: &str) {
    let id = id.to_owned();
    let checked = settle_check(app, &id);
    let config = app.state::<Config>();
    // The window items (always on top, devtools, full screen) are desktop-only
    #[cfg(desktop)]
    let window = app.get_webview_window(MAIN_WEBVIEW);

    if let Some(command_id) = id.strip_prefix("cmd:") {
        let command_id: u64 = command_id.parse().unwrap_or_default();
        ipc::send_to_page(app, "peard:menu-event", &[json!(command_id), json!(checked)]);
        return;
    }
    if let Some(plugin) = id.strip_prefix("plugin:") {
        if always_on(plugin) {
            return;
        }
        // `config.plugins.enable/disable` → setMenuOptions
        let mut value = config.get(&format!("plugins.{plugin}"));
        if !value.is_object() {
            value = json!({});
        }
        value["enabled"] = json!(checked.unwrap_or(false));
        set_menu_option(app, &format!("plugins.{plugin}"), value);
        return;
    }
    if let Some(option) = id.strip_prefix("opt:") {
        let checked = checked.unwrap_or(false);
        set_menu_option(app, &format!("options.{option}"), json!(checked));
        match option {
            #[cfg(desktop)]
            "alwaysOnTop" => {
                if let Some(window) = &window {
                    let _ = window.set_always_on_top(checked);
                }
            }
            "startAtLogin" => crate::window::set_start_at_login(app, checked),
            #[cfg(target_os = "android")]
            "fullscreen" => crate::android_app::set_fullscreen(app, checked),
            "trayClickPlayPause" => {}
            _ => {}
        }
        return;
    }
    if let Some(rest) = id.strip_prefix("radio:") {
        let (group, value) = rest.split_once(':').unwrap_or((rest, ""));
        match group {
            "startingPage" => config.set(app, "options.startingPage", json!(value)),
            "likeButtons" => config.set(app, "options.likeButtons", json!(value)),
            "tray" => {
                set_menu_option(app, "options.tray", json!(value != "disabled"));
                set_menu_option(app, "options.appVisible", json!(value != "hide"));
                #[cfg(desktop)]
                crate::tray::sync(app);
            }
            "language" => {
                config.set(app, "options.language", json!(value));
                // The renderer reads the language at load
                page::reload(app);
            }
            _ => {}
        }
        return;
    }

    match id.as_str() {
        "act:customWindowTitle" => crate::window::prompt_custom_title(app),
        "act:setProxy" => crate::proxy::prompt_proxy(app),
        "act:editConfig" => {
            if let Err(error) = crate::config::config_edit(app.clone()) {
                log::error!("opening config.json failed: {error}");
            }
        }
        #[cfg(desktop)]
        "act:toggleDevTools" => {
            if let Some(window) = &window {
                if window.is_devtools_open() {
                    window.close_devtools();
                } else {
                    window.open_devtools();
                }
            }
        }
        "act:reload" | "act:forceReload" => page::reload(app),
        #[cfg(desktop)]
        "act:zoomIn" => zoom(app, |z| z + 0.1),
        #[cfg(desktop)]
        "act:zoomOut" => zoom(app, |z| z - 0.1),
        #[cfg(desktop)]
        "act:resetZoom" => zoom(app, |_| 1.0),
        #[cfg(desktop)]
        "act:toggleFullscreen" => {
            if let Some(window) = &window {
                let fullscreen = window.is_fullscreen().unwrap_or(false);
                let _ = window.set_fullscreen(!fullscreen);
            }
        }
        "act:goBack" => page::eval(app, "history.back()"),
        "act:goForward" => page::eval(app, "history.forward()"),
        "act:copyUrl" => {
            // Off the main thread: on Android the URL may come from Kotlin
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Some(url) = page::url(&app) {
                    use tauri_plugin_clipboard_manager::ClipboardExt;
                    let _ = app.clipboard().write_text(url);
                }
            });
        }
        "act:restart" => app_controls::restart(app),
        "act:quit" => app.exit(0),
        "act:about" => crate::dialogs::about(app),
        // The tray's own menu handler takes these (tray.rs); Tauri sends
        // every menu event here too
        id if id.starts_with("tray:") => {}
        _ => log::warn!("unhandled menu item '{id}'"),
    }
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
