//! Builds the menu tree: the core items of `src/menu.ts` and the "Plugins"
//! submenu, as native items and as the Electron-shaped JSON in-app-menu draws.

use super::*;

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

pub(super) fn build(app: &AppHandle, data: &MenuData) -> tauri::Result<NativeMenu> {
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
