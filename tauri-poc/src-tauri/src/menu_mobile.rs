//! `menu` on Android and iOS: no native menu or tray, but backends still
//! read the translated labels the page sends with its menu (`menu_update`).

use std::{collections::HashMap, sync::Mutex};

use serde::Deserialize;
use tauri::{AppHandle, Manager};

/// The part of the page's menu data used without a native menu.
#[derive(Deserialize, Default)]
pub struct MenuData {
    labels: HashMap<String, String>,
}

#[derive(Default)]
pub struct MenuState {
    data: Mutex<MenuData>,
}

/// A translated label from the page, or `fallback` before it has sent them.
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

pub fn rebuild(_app: &AppHandle) {}

#[tauri::command]
pub fn menu_update(app: AppHandle, data: MenuData) {
    *app.state::<MenuState>().data.lock().unwrap() = data;
}

#[tauri::command]
pub fn menu_set_checked(_command_id: u64, _checked: bool) {}
