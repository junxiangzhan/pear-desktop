//! Tauri shell for Pear Desktop: hosts music.youtube.com with the renderer
//! bundle injected, and reimplements the Electron main process in Rust.

#[cfg(target_os = "android")]
mod android_media;
mod app_controls;
mod config;
mod dialogs;
#[cfg(target_os = "android")]
mod gecko_bridge;
mod ipc;
mod menu;
mod net;
mod page;
mod plugins;
mod prompt;
mod song_controls;
mod song_info;
#[cfg(target_os = "android")]
mod system_bars;
#[cfg(desktop)]
mod tray;
mod window;

use std::fs;

use serde_json::json;
use tauri::{
    http::{header, Response, StatusCode},
    Manager, WebviewUrl, WebviewWindowBuilder,
};

use crate::{
    config::Config,
    ipc::{Ipc, MAIN_WEBVIEW},
    plugins::Plugins,
    song_info::SongInfoProvider,
};

const YTM_URL: &str = "https://music.youtube.com";

// Built by `pnpm exec vite build -c tauri-poc/vite.inject.config.mts`
const RENDERER_JS: &str = include_str!("../inject/renderer.js");

// Custom protocol that serves the renderer bundle to the page. Windows and
// Android expose custom protocols as `https://<scheme>.localhost` (with
// `use_https_scheme`), the other platforms as `<scheme>://localhost`.
const PROTOCOL: &str = "pear";
#[cfg(any(windows, target_os = "android"))]
const RENDERER_URL: &str = "https://pear.localhost/renderer.js";
#[cfg(not(any(windows, target_os = "android")))]
const RENDERER_URL: &str = "pear://localhost/renderer.js";

/// The bundle served at `RENDERER_URL`.
struct RendererBundle(String);

/// Tiny init script that runs the payload in the top frame before page scripts.
///
/// The bundle is not passed as the init script itself: WebView2 injects init
/// scripts into every frame and its browser process spins at 100% CPU on a
/// script this size, hanging the page. A synchronous XHR keeps the
/// "before any page script" timing of Electron's preload. YTM's CSP allows
/// `'unsafe-eval'` but requires Trusted Types, hence the policy.
fn loader_script() -> String {
    format!(
        r#"(() => {{
  if (window.top !== window.self) return;
  // Once per document: on Android, wry runs init scripts at document start
  // and again from onPageStarted
  if (window.__pearRendererLoaded) return;
  const xhr = new XMLHttpRequest();
  xhr.open('GET', '{RENDERER_URL}', false);
  xhr.send();
  if (xhr.status !== 200) {{
    console.error('[pear-tauri] failed to load renderer', xhr.status);
    return;
  }}
  window.__pearRendererLoaded = true;
  const policy = window.trustedTypes?.createPolicy('pear-tauri-loader', {{
    createScript: (script) => script,
  }});
  const source = xhr.responseText + '\n//# sourceURL=pear-renderer.js';
  (0, eval)(policy ? policy.createScript(source) : source);
}})();"#
    )
}

/// What the renderer reads synchronously at startup, ahead of the bundle.
/// Built per request so a page reload sees the current config.
fn payload(app: &tauri::AppHandle) -> String {
    let config = app.state::<Config>();
    let unavailable: Vec<&String> = config::unavailable_plugins().iter().collect();
    format!(
        "window.__PEAR_STORED_CONFIG__ = {};\nwindow.__PEAR_UNAVAILABLE_PLUGINS__ = {};\nwindow.__PEAR_MOBILE__ = {};\n{}",
        config.snapshot(),
        json!(unavailable),
        cfg!(mobile),
        app.state::<RendererBundle>().0
    )
}

/// Like `app.setAppUserModelId` in src/index.ts: the identity Windows shows
/// for the app (taskbar grouping, notifications, media controls).
#[cfg(windows)]
fn set_app_user_model_id(identifier: &str) {
    use windows::{core::HSTRING, Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID};
    if let Err(error) = unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(identifier)) } {
        log::warn!("setting the AppUserModelID failed: {error}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    set_app_user_model_id("com.github.pear-devs.tauri-poc");

    let mut builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init());

    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                // Like the `second-instance` handler in src/index.ts
                if let Some(window) = app.get_webview_window(MAIN_WEBVIEW) {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }))
            // Replaces the hand-rolled window-size/position saving in src/index.ts
            .plugin(
                tauri_plugin_window_state::Builder::new()
                    // Geometry only: decorations follow in-app-menu and
                    // visibility follows the tray options, not the last session
                    .with_state_flags(
                        tauri_plugin_window_state::StateFlags::all()
                            - tauri_plugin_window_state::StateFlags::DECORATIONS
                            - tauri_plugin_window_state::StateFlags::VISIBLE,
                    )
                    .build(),
            )
            .plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                None,
            ))
            .plugin(tauri_plugin_global_shortcut::Builder::new().build());
    }
    #[cfg(target_os = "android")]
    {
        builder = builder
            .plugin(android_media::init())
            .plugin(gecko_bridge::init())
            .plugin(system_bars::init());
    }

    builder = builder.invoke_handler(tauri::generate_handler![
            ipc::ipc_send,
            ipc::ipc_invoke,
            config::config_set,
            config::config_set_partial,
            config::config_edit,
            config::plugins_manifest,
            app_controls::app_restart,
            menu::menu_update,
            menu::menu_set_checked,
            prompt::prompt_open,
            prompt::prompt_submit,
            net::net_fetch,
            window::window_set_always_on_top,
            window::window_start_dragging,
            window::window_toggle_maximize,
        ]);
    #[cfg(desktop)]
    {
        builder = builder.on_menu_event(menu::handle_event);
    }

    builder
        .register_uri_scheme_protocol(PROTOCOL, |ctx, request| {
            if request.uri().path() != "/renderer.js" {
                return Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Vec::new())
                    .unwrap();
            }
            Response::builder()
                .header(header::CONTENT_TYPE, "text/javascript; charset=utf-8")
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, YTM_URL)
                .header(header::CACHE_CONTROL, "no-store")
                .body(payload(ctx.app_handle()).into_bytes())
                .unwrap()
        })
        .setup(|app| {
            let config_dir = match window::profile_dir() {
                Some(dir) => dir,
                None => app.path().app_config_dir()?,
            };
            let config_path = config_dir.join("config.json");
            app.manage(Config::load(config_path));
            app.manage(Ipc::default());
            app.manage(SongInfoProvider::default());
            app.manage(Plugins::new(plugins::all()));
            app.manage(menu::MenuState::default());
            app.manage(prompt::Prompts::default());
            app.manage(window::PageTitle::default());

            let ipc = app.state::<Ipc>();
            config::register_ipc(&ipc);
            song_info::register_ipc(&ipc);
            app_controls::register_ipc(&ipc);
            dialogs::register_ipc(&ipc);
            // in-app-menu is the only menu on Android, but its default comes
            // from the user agent (desktop Linux there, where it is off)
            #[cfg(mobile)]
            {
                let config = app.state::<Config>();
                if !config.get_bool("plugins.in-app-menu.enabled") {
                    config.set(app.handle(), "plugins.in-app-menu.enabled", json!(true));
                }
            }
            #[cfg(target_os = "android")]
            {
                android_media::register(app.handle());
                system_bars::register(app.handle());
                if page::in_gecko() {
                    gecko_bridge::register(app.handle());
                }
            }

            // Diagnostic switch: inject a file from disk instead of the built-in bundle
            let bundle = match std::env::var("PEAR_POC_INJECT_FILE") {
                Ok(path) => fs::read_to_string(path)?,
                Err(_) => RENDERER_JS.to_owned(),
            };
            app.manage(RendererBundle(bundle));

            create_main_window(app)?;
            // Built with fallback labels now; rebuilt once the page sends translations
            menu::rebuild(app.handle());
            #[cfg(desktop)]
            {
                tray::sync(app.handle());
                tray::register(app.handle());
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Like `options.resumeOnStart` in src/index.ts.
fn start_url(config: &Config) -> String {
    if config.get_bool("options.resumeOnStart") {
        config.get_str("url")
    } else {
        YTM_URL.to_owned()
    }
}

fn create_main_window(app: &tauri::App) -> tauri::Result<()> {
    let config = app.state::<Config>();
    // In GeckoView mode MainActivity shows GeckoView over this webview, which
    // stays blank: it only keeps Tauri's Android lifecycle going
    let start_url = if page::in_gecko() { "about:blank".to_owned() } else { start_url(&config) };
    let start_url = start_url
        .parse()
        .unwrap_or_else(|_| YTM_URL.parse().expect("valid default URL"));

    let mut builder = window::shared_webview_options(WebviewWindowBuilder::new(
        app,
        MAIN_WEBVIEW,
        WebviewUrl::External(start_url),
    ));
    // Diagnostic switch: load the bare site to tell shell issues from renderer issues
    if std::env::var_os("PEAR_POC_NO_INJECT").is_none() && !page::in_gecko() {
        builder = builder.initialization_script_for_all_frames(loader_script());
    }

    #[cfg(desktop)]
    {
        // in-app-menu draws the title bar and menu in the page (restart needed)
        let in_app_menu = plugins::in_app_menu::enabled_at_startup(&config);
        app.state::<menu::MenuState>()
            .in_app
            .store(in_app_menu, std::sync::atomic::Ordering::Relaxed);
        let tray = config.get_bool("options.tray");
        builder = builder
            .decorations(!in_app_menu)
            .title("Pear Desktop")
            .inner_size(1100.0, 700.0)
            .min_inner_size(325.0, 425.0)
            .always_on_top(config.get_bool("options.alwaysOnTop"))
            // Like `options.appVisible`: start hidden in the tray
            .visible(!tray || config.get_bool("options.appVisible"))
            // Window materials only show through a transparent window; the
            // transparent-player plugin needs a restart to toggle, like Electron
            .transparent(config.get_bool("plugins.transparent-player.enabled"))
            .on_document_title_changed(|window, title| window::apply_title(&window, Some(title)));
    }

    // On Android, MainActivity presents the WebView as desktop Chrome (the
    // plugins need the desktop site, and Google sign-in refuses WebViews)
    if let Ok(user_agent) = std::env::var("PEAR_POC_USER_AGENT") {
        builder = builder.user_agent(&user_agent);
    }

    let window = builder.build()?;
    window::apply_title(&window, None);
    window::install_close_to_tray(&window);
    if std::env::var_os("PEAR_POC_DEVTOOLS").is_some() {
        window.open_devtools();
    }
    Ok(())
}
