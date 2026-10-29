//! Rust ports of plugin backends (the `backend` part of `createPlugin`).
//!
//! A backend is started when its plugin is enabled, after the page has sent
//! the plugin defaults. Its IPC registrations and song-info callbacks use the
//! plugin id as owner and are dropped automatically when it stops.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::{
    config::Config,
    ipc::{send_to_page, Ipc},
    song_info::SongInfoProvider,
};

/// Asks the page to report player events (`peard:setup-<name>-listener`),
/// now and whenever the player API loads again. Electron only asked on
/// `peard:player-api-loaded`, which a plugin enabled at runtime has missed;
/// the page sets each listener up once, so asking twice is harmless.
pub fn request_player_listeners(app: &AppHandle, owner: &str, names: &'static [&'static str]) {
    let send = move |app: &AppHandle| {
        for name in names {
            send_to_page(app, &format!("peard:setup-{name}-listener"), &[]);
        }
    };
    send(app);
    app.state::<Ipc>().on(owner, "peard:player-api-loaded", move |app, _| send(app));
}

pub trait Plugin: Send + Sync + 'static {
    fn id(&self) -> &'static str;
    /// Like `backend.start(ctx)`; `config` is the merged plugin config.
    fn start(&self, app: &AppHandle, config: &Value);
    /// Like `backend.stop(ctx)`. Registrations owned by the plugin are
    /// removed after this returns.
    fn stop(&self, _app: &AppHandle) {}
    /// Like `backend.onConfigChange(config)`.
    fn on_config_change(&self, _app: &AppHandle, _config: &Value) {}
}

pub struct Plugins {
    registry: HashMap<&'static str, Arc<dyn Plugin>>,
    running: Mutex<HashSet<&'static str>>,
}

impl Plugins {
    pub fn new(list: Vec<Arc<dyn Plugin>>) -> Self {
        Self {
            registry: list.into_iter().map(|p| (p.id(), p)).collect(),
            running: Mutex::new(HashSet::new()),
        }
    }

    pub fn start(&self, app: &AppHandle, id: &str) {
        let Some(plugin) = self.registry.get(id) else { return };
        if !self.running.lock().unwrap().insert(plugin.id()) {
            return;
        }
        let config = app.state::<Config>().plugin_config(id);
        plugin.start(app, &config);
        log::info!("plugin {id}: backend started");
    }

    pub fn stop(&self, app: &AppHandle, id: &str) {
        let Some(plugin) = self.registry.get(id) else { return };
        if !self.running.lock().unwrap().remove(plugin.id()) {
            return;
        }
        plugin.stop(app);
        app.state::<Ipc>().remove_owner(id);
        app.state::<SongInfoProvider>().remove_owner(id);
        log::info!("plugin {id}: backend stopped");
    }

    pub fn config_changed(&self, app: &AppHandle, id: &str, config: &Value) {
        let Some(plugin) = self.registry.get(id) else { return };
        if self.running.lock().unwrap().contains(plugin.id()) {
            plugin.on_config_change(app, config);
        }
    }

    /// Starts every enabled backend; called once the plugin defaults are known.
    pub fn start_enabled(&self, app: &AppHandle) {
        let config = app.state::<Config>();
        for id in self.registry.keys() {
            if config.plugin_enabled(id) {
                self.start(app, id);
            }
        }
    }
}

mod amuse;
mod api_server;
pub mod auth_proxy_adapter;
pub mod in_app_menu;
mod discord;
pub mod do_not_track;
mod downloader;
mod notifications;
mod now_playing_post;
#[cfg(desktop)]
mod picture_in_picture;
mod scrobbler;
#[cfg(desktop)]
mod shortcuts;
mod sponsorblock;
mod synced_lyrics;
#[cfg(windows)]
mod taskbar_mediacontrol;
#[cfg(desktop)]
mod transparent_player;

/// The ported backends. Each id must also be removed from
/// `plugin-support.json` so the page enables it.
pub fn all() -> Vec<Arc<dyn Plugin>> {
    #[cfg_attr(mobile, allow(unused_mut))]
    let mut list: Vec<Arc<dyn Plugin>> = vec![
        Arc::new(sponsorblock::SponsorBlock),
        Arc::new(synced_lyrics::SyncedLyrics),
        Arc::new(now_playing_post::LumiaStream),
        Arc::new(now_playing_post::TunaObs::new()),
        Arc::new(notifications::Notifications::new()),
        Arc::new(scrobbler::Scrobbler::new()),
        Arc::new(discord::Discord::new()),
        Arc::new(downloader::Downloader),
        Arc::new(do_not_track::DoNotTrack),
        Arc::new(api_server::ApiServer::new()),
        Arc::new(amuse::Amuse::new()),
        // The only menu on Android, where there is no native one
        Arc::new(in_app_menu::InAppMenu::new()),
    ];
    // Window and keyboard features of the desktop shell
    #[cfg(desktop)]
    {
        list.push(Arc::new(auth_proxy_adapter::AuthProxyAdapter));
        list.push(Arc::new(picture_in_picture::PictureInPicture::new()));
        list.push(Arc::new(transparent_player::TransparentPlayer));
        list.push(Arc::new(shortcuts::Shortcuts::new()));
        list.push(Arc::new(shortcuts::PreciseVolume::new()));
    }
    #[cfg(windows)]
    list.push(Arc::new(taskbar_mediacontrol::TaskbarMediaControl));
    list
}
