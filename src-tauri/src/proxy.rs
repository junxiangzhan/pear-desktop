//! `options.proxy`, like `src/index.ts`: read once at startup, applied to the
//! webviews (Windows; WebView2 takes the proxy when it starts) and to Rust's
//! own requests (`net::client`). With auth-proxy-adapter on, the webviews use
//! its local relay instead, since Chromium can't pass SOCKS credentials. On
//! Android the pear-gecko extension applies the proxy itself, credentials
//! included (`assets/pear-gecko/background.js`), and changes apply at once.

use std::sync::OnceLock;

use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::{config::Config, menu::label, prompt};

static WEBVIEW_PROXY: OnceLock<Option<url::Url>> = OnceLock::new();

/// The upstream proxy URL from the settings, if any.
pub fn configured(config: &Config) -> Option<String> {
    let proxy = config.get_str("options.proxy");
    let proxy = proxy.trim();
    (!proxy.is_empty()).then(|| proxy.to_owned())
}

/// Called once in `setup`, before any webview or HTTP client exists.
pub fn init(config: &Config) {
    let upstream = configured(config);
    crate::net::set_proxy(upstream.clone());
    let webview = upstream.and_then(|upstream| {
        let url = if config.get_bool("plugins.auth-proxy-adapter.enabled") {
            let (hostname, port) = crate::plugins::auth_proxy_adapter::listen_address(&config.get("plugins.auth-proxy-adapter"));
            // The page loads through it, before plugin backends start
            #[cfg(desktop)]
            crate::plugins::auth_proxy_adapter::ensure_running((hostname.clone(), port), upstream.clone());
            format!("socks5://{hostname}:{port}")
        } else {
            upstream
        };
        let shown = crate::plugins::auth_proxy_adapter::redact(&url);
        log::info!("using proxy {shown}");
        url.parse().map_err(|error| log::error!("invalid proxy {shown}: {error}")).ok()
    });
    let _ = WEBVIEW_PROXY.set(webview);
}

/// The proxy every webview starts with (see `window::shared_webview_options`).
#[cfg_attr(mobile, allow(dead_code))] // GeckoView: the extension applies it
pub fn webview_proxy() -> Option<&'static url::Url> {
    WEBVIEW_PROXY.get().and_then(Option::as_ref)
}

/// The "Set proxy" menu item, like `setProxy` in `src/menu.ts`.
pub fn prompt_proxy(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let key = "main.menu.options.submenu.advanced-options.submenu.set-proxy";
        let current = app.state::<Config>().get_str("options.proxy");
        let options = json!({
            "title": label(&app, &format!("{key}.prompt.title"), "Set proxy"),
            "label": label(&app, &format!("{key}.prompt.label"), "Enter proxy address"),
            "value": current,
            "type": "input",
            "inputAttrs": {
                "type": "url",
                "placeholder": label(&app, &format!("{key}.prompt.placeholder"), "Example: socks5://127.0.0.1:9999"),
            },
            "width": 450,
        });
        let value = match prompt::open(&app, options).await {
            Ok(value) => value,
            Err(error) => {
                log::error!("proxy prompt failed: {error}");
                return;
            }
        };
        let Some(proxy) = value.as_str() else { return }; // cancelled
        if proxy == current {
            return;
        }
        crate::menu::set_menu_option(&app, "options.proxy", json!(proxy));
        // The webviews and Rust's HTTP client take the proxy at startup
        #[cfg(desktop)]
        crate::dialogs::need_restart(&app, "proxy");
    });
}
