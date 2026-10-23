use std::{fs, path::PathBuf};

use serde_json::Value;
use tauri::{
    http::{header, Response, StatusCode},
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder,
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

// Android's WebView gets the mobile site unless it looks like desktop Chrome,
// and the renderer plugins only understand the desktop DOM.
#[cfg(mobile)]
const DEFAULT_USER_AGENT: Option<&str> = Some(
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
);
#[cfg(desktop)]
const DEFAULT_USER_AGENT: Option<&str> = None;

/// The script served at `RENDERER_URL`: stored config followed by the bundle.
struct RendererPayload(String);

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
  const xhr = new XMLHttpRequest();
  xhr.open('GET', '{RENDERER_URL}', false);
  xhr.send();
  if (xhr.status !== 200) {{
    console.error('[pear-tauri-poc] failed to load renderer', xhr.status);
    return;
  }}
  const policy = window.trustedTypes?.createPolicy('pear-tauri-poc-loader', {{
    createScript: (script) => script,
  }});
  const source = xhr.responseText + '\n//# sourceURL=pear-renderer.js';
  (0, eval)(policy ? policy.createScript(source) : source);
}})();"#
    )
}

fn config_path(app: &AppHandle) -> tauri::Result<PathBuf> {
    Ok(app.path().app_config_dir()?.join("config.json"))
}

fn load_config(app: &AppHandle) -> Value {
    config_path(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| Value::Object(Default::default()))
}

#[tauri::command]
fn save_config(app: AppHandle, config: Value) -> Result<(), String> {
    let path = config_path(&app).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![save_config])
        .register_uri_scheme_protocol(PROTOCOL, |ctx, request| {
            if request.uri().path() != "/renderer.js" {
                return Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Vec::new())
                    .unwrap();
            }
            let payload = ctx.app_handle().state::<RendererPayload>();
            Response::builder()
                .header(header::CONTENT_TYPE, "text/javascript; charset=utf-8")
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, YTM_URL)
                .header(header::CACHE_CONTROL, "no-store")
                .body(payload.0.as_bytes().to_vec())
                .unwrap()
        })
        .setup(|app| {
            // Diagnostic switch: serve a file from disk instead of the built-in bundle
            let renderer_js = match std::env::var("PEAR_POC_INJECT_FILE") {
                Ok(path) => fs::read_to_string(path)?,
                Err(_) => RENDERER_JS.to_owned(),
            };
            // The renderer reads config synchronously, so it is baked into the
            // payload instead of fetched over (async-only) Tauri IPC.
            app.manage(RendererPayload(format!(
                "window.__PEAR_STORED_CONFIG__ = {};\n{}",
                load_config(app.handle()),
                renderer_js
            )));

            let mut builder =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::External(YTM_URL.parse()?))
                    .use_https_scheme(true);
            // Diagnostic switch: load the bare site to tell shell issues from renderer issues
            if std::env::var_os("PEAR_POC_NO_INJECT").is_none() {
                builder = builder.initialization_script_for_all_frames(loader_script());
            }

            #[cfg(desktop)]
            {
                builder = builder
                    .title("Pear Desktop (Tauri PoC)")
                    .inner_size(1100.0, 700.0)
                    .min_inner_size(325.0, 425.0);
            }

            // Diagnostic switch: expose the Chrome DevTools Protocol on this port.
            // Setting browser args replaces wry's defaults, so they are repeated here.
            #[cfg(windows)]
            if let Ok(port) = std::env::var("PEAR_POC_DEBUG_PORT") {
                builder = builder.additional_browser_args(&format!(
                    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port={port}"
                ));
            }

            let user_agent = std::env::var("PEAR_POC_USER_AGENT")
                .ok()
                .or(DEFAULT_USER_AGENT.map(String::from));
            if let Some(user_agent) = user_agent {
                builder = builder.user_agent(&user_agent);
            }

            let window = builder.build()?;
            if std::env::var_os("PEAR_POC_DEVTOOLS").is_some() {
                window.open_devtools();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
