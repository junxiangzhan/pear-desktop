//! Port of do-not-track's "With blocklists" mode
//! (src/plugins/do-not-track/blocker.ts, Ghostery's engine on Electron's
//! `session.webRequest`), on Brave's adblock-rust. Same lists: the tb.json
//! set (unless `disableDefaultLists`) plus `additionalBlockLists`, with
//! Ghostery's copy of uBlock Origin's resources for scriptlets and
//! redirects; cached like Electron when `cache` is on and there are no
//! additional lists (the cache is never refreshed, as in Electron).
//!
//! - Windows: every request of the main webview passes WebView2's
//!   `WebResourceRequested` ([`install`]); blocked ones get an empty 403,
//!   redirects their resource. The app's own `*.localhost` hosts are skipped.
//! - Android (GeckoView): the pear-gecko extension asks for each request
//!   over native messaging (`dnt_check`, assets/pear-gecko/background.js).
//! - Both: the page asks for the URL's cosmetic filters
//!   (`do-not-track:cosmetics`) where Electron loaded
//!   `@ghostery/adblocker-electron-preload`, and adds the hiding CSS and the
//!   scriptlets. Generic class/id hiding and procedural filters, which the
//!   Ghostery preload computes from the live DOM, are not ported, and
//!   neither is `removeparam`.
//!
//! The "In player" mode runs in the page unchanged. Unlike Electron, the
//! engine is dropped when switching back to it, not only when the plugin
//! stops.

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, OnceLock,
    },
    time::Duration,
};

use adblock::{
    lists::{FilterSet, ParseOptions},
    request::Request,
    resources::{MimeType, PermissionMask, Resource, ResourceType},
    Engine,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::sync::watch;

use super::Plugin;
use crate::ipc::{arg, Ipc};

const ID: &str = "do-not-track";
const WITH_BLOCKLISTS: &str = "With blocklists";
const TB_SOURCES: &str = "https://raw.githubusercontent.com/organization/tb-list/refs/heads/main/tb.json";
/// What Ghostery's `fromLists` loads besides the lists
const RESOURCES: &str =
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/ublock-origin/resources.json";
/// uBlock Origin only lets its own lists use `trusted-*` scriptlets; here
/// the default lists (chosen by Pear) get the permission, additional ones don't
const TRUSTED: PermissionMask = PermissionMask::from_bits(1);

fn engine() -> &'static watch::Sender<Option<Arc<Engine>>> {
    static ENGINE: OnceLock<watch::Sender<Option<Arc<Engine>>>> = OnceLock::new();
    ENGINE.get_or_init(|| watch::channel(None).0)
}

/// Bumped whenever blocking is turned on or off, so a slow load that is
/// no longer wanted is dropped
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub struct DoNotTrack;

impl Plugin for DoNotTrack {
    fn id(&self) -> &'static str {
        ID
    }

    fn start(&self, app: &AppHandle, config: &Value) {
        app.state::<Ipc>().handle(ID, "do-not-track:cosmetics", |_, args| async move {
            let url: String = arg(&args, 0)?;
            Ok(cosmetics(&url).await)
        });
        self.on_config_change(app, config);
    }

    fn stop(&self, _app: &AppHandle) {
        deactivate();
    }

    fn on_config_change(&self, app: &AppHandle, config: &Value) {
        let wanted = config["blocker"].as_str() == Some(WITH_BLOCKLISTS);
        if !wanted {
            deactivate();
        } else if engine().borrow().is_none() && !LOADING.load(Ordering::SeqCst) {
            activate(app, config.clone());
        }
    }
}

static LOADING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn deactivate() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    LOADING.store(false, Ordering::SeqCst);
    if engine().send_replace(None).is_some() {
        log::info!("do-not-track: blocking off");
    }
}

fn activate(app: &AppHandle, config: Value) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    LOADING.store(true, Ordering::SeqCst);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let loaded = load_engine(&app, &config).await;
        if GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        LOADING.store(false, Ordering::SeqCst);
        match loaded {
            Ok(loaded) => {
                engine().send_replace(Some(Arc::new(loaded)));
                log::info!("do-not-track: blocking on");
            }
            Err(error) => log::error!("do-not-track: loading the blocker engine failed: {error}"),
        }
    });
}

/// Like `fetchWithRetry`: three retries, half a second apart.
async fn fetch_text(url: &str) -> Result<String, String> {
    let mut retries = 3;
    loop {
        let result = async {
            crate::net::client()
                .get(url)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)?
                .text()
                .await
        }
        .await;
        match result {
            Ok(text) => return Ok(text),
            Err(_) if retries > 0 => {
                retries -= 1;
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(error) => return Err(format!("{url}: {}", error.without_url())),
        }
    }
}

async fn load_engine(app: &AppHandle, config: &Value) -> Result<Engine, String> {
    let additional: Vec<String> = config["additionalBlockLists"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|list| list.as_str().map(str::to_owned))
        .collect();
    let disable_default = match &config["disableDefaultLists"] {
        Value::Bool(disabled) => *disabled,
        Value::Array(list) => !list.is_empty(),
        _ => false,
    };
    let cache_dir = app.path().app_cache_dir().map_err(|error| error.to_string())?.join("tb_cache");
    let cache = (config["cache"].as_bool().unwrap_or(true) && additional.is_empty())
        .then(|| (cache_dir.join("tb-engine.dat"), cache_dir.join("tb-resources.json")));

    if let Some((engine_path, resources_path)) = &cache {
        if let (Ok(serialized), Ok(resources)) = (std::fs::read(engine_path), std::fs::read_to_string(resources_path)) {
            let mut engine = Engine::default();
            match engine.deserialize(&serialized) {
                Ok(()) => {
                    engine.use_resources(parse_resources(&resources));
                    log::info!("do-not-track: engine loaded from the cache");
                    return Ok(engine);
                }
                Err(error) => log::warn!("do-not-track: ignoring the cached engine: {error:?}"),
            }
        }
    }

    #[derive(Deserialize)]
    struct TbSources {
        tb: Vec<String>,
    }
    let default_lists = if disable_default {
        vec![]
    } else {
        // Like Electron, an unreadable tb.json means no default lists
        match fetch_text(TB_SOURCES).await.map(|text| serde_json::from_str::<TbSources>(&text)) {
            Ok(Ok(sources)) => sources.tb,
            Ok(Err(error)) => {
                log::warn!("do-not-track: tb.json is invalid: {error}");
                vec![]
            }
            Err(error) => {
                log::warn!("do-not-track: {error}");
                vec![]
            }
        }
    };

    let fetch_all = |urls: Vec<String>| {
        futures::future::join_all(urls.into_iter().map(|url| async move { (fetch_text(&url).await, url) }))
    };
    let (default_texts, additional_texts, resources) =
        futures::join!(fetch_all(default_lists), fetch_all(additional), fetch_text(RESOURCES));

    let mut filters = FilterSet::new(false);
    let mut count = 0;
    for (texts, permissions) in [(default_texts, TRUSTED), (additional_texts, PermissionMask::default())] {
        for (text, url) in texts {
            match text {
                Ok(text) => {
                    filters.add_filter_list(text, ParseOptions { permissions, ..ParseOptions::default() });
                    count += 1;
                }
                // Electron failed the whole load; one list less is better
                Err(error) => log::warn!("do-not-track: skipping list {url}: {error}"),
            }
        }
    }
    if count == 0 {
        return Err("no filter list could be loaded".into());
    }
    let resources = resources.map_err(|error| log::warn!("do-not-track: no resources: {error}")).ok();
    let engine = tauri::async_runtime::spawn_blocking(move || {
        let mut engine = Engine::new_with_filter_set(filters);
        if let Some(resources) = &resources {
            engine.use_resources(parse_resources(resources));
        }
        (engine, resources)
    })
    .await
    .map_err(|error| error.to_string())?;
    let (engine, resources) = engine;
    log::info!("do-not-track: engine built from {count} lists");

    if let (Some((engine_path, resources_path)), Some(resources)) = (&cache, resources) {
        let written = std::fs::create_dir_all(&cache_dir)
            .and_then(|()| std::fs::write(engine_path, engine.serialize()))
            .and_then(|()| std::fs::write(resources_path, resources));
        if let Err(error) = written {
            log::warn!("do-not-track: caching the engine failed: {error}");
        }
    }
    Ok(engine)
}

/// Ghostery's resources.json (`scriptlets` and `redirects`) as adblock-rust
/// resources. Scriptlets are uBlock Origin's function style, with `.fn`
/// helpers as dependencies.
fn parse_resources(json: &str) -> Vec<Resource> {
    #[derive(Deserialize)]
    struct Entry {
        name: String,
        #[serde(default)]
        aliases: Vec<String>,
        body: String,
        #[serde(default)]
        dependencies: Vec<String>,
        #[serde(rename = "contentType")]
        content_type: Option<String>,
    }
    #[derive(Deserialize)]
    struct Resources {
        #[serde(default)]
        scriptlets: Vec<Entry>,
        #[serde(default)]
        redirects: Vec<Entry>,
    }
    let Ok(resources) = serde_json::from_str::<Resources>(json) else {
        log::warn!("do-not-track: resources.json is invalid");
        return vec![];
    };
    let scriptlets = resources.scriptlets.into_iter().map(|entry| {
        let kind = if entry.name.ends_with(".fn") { MimeType::FnJavascript } else { MimeType::ApplicationJavascript };
        let permission = if entry.name.starts_with("trusted-") { TRUSTED } else { PermissionMask::default() };
        Resource {
            name: entry.name,
            aliases: entry.aliases,
            kind: ResourceType::Mime(kind),
            content: BASE64.encode(entry.body),
            dependencies: entry.dependencies,
            permission,
        }
    });
    let redirects = resources.redirects.into_iter().map(|entry| {
        let content_type = entry.content_type.unwrap_or_default();
        let (mime, encoded) = match content_type.strip_suffix(";base64") {
            Some(mime) => (mime.to_owned(), entry.body),
            None => (content_type, BASE64.encode(entry.body)),
        };
        Resource {
            name: entry.name,
            aliases: entry.aliases,
            kind: ResourceType::Mime(MimeType::from(mime.as_str())),
            content: encoded,
            dependencies: vec![],
            permission: PermissionMask::default(),
        }
    });
    scriptlets.chain(redirects).collect()
}

pub enum Verdict {
    Allow,
    Block,
    /// A `data:` URL to answer with instead
    Redirect(String),
}

/// The engine's answer for one request; `Allow` while blocking is off or
/// still loading. `request_type` uses webRequest's names.
pub fn check(url: &str, source_url: &str, request_type: &str) -> Verdict {
    let Some(engine) = engine().borrow().clone() else { return Verdict::Allow };
    let Ok(parsed) = url::Url::parse(url) else { return Verdict::Allow };
    let local = parsed.host_str().is_none_or(|host| host == "localhost" || host.ends_with(".localhost"));
    if !matches!(parsed.scheme(), "http" | "https" | "ws" | "wss") || local {
        return Verdict::Allow;
    }
    let Ok(request) = Request::new(url, source_url, request_type, "GET") else { return Verdict::Allow };
    let result = engine.check_network_request(&request);
    if !result.should_block() {
        return Verdict::Allow;
    }
    match result.redirect {
        Some(redirect) => Verdict::Redirect(redirect),
        None => Verdict::Block,
    }
}

/// `dnt_check` for the pear-gecko extension: a webRequest blocking answer.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn check_for_gecko(args: &Value) -> Value {
    let field = |name: &str| args[name].as_str().unwrap_or_default();
    match check(field("url"), field("source"), field("type")) {
        Verdict::Allow => json!({}),
        Verdict::Block => json!({ "cancel": true }),
        Verdict::Redirect(url) => json!({ "redirectUrl": url }),
    }
}

/// The URL's hiding CSS and scriptlets, once the engine is ready (the first
/// load downloads the lists).
async fn cosmetics(url: &str) -> Value {
    let mut receiver = engine().subscribe();
    let ready = tokio::time::timeout(Duration::from_secs(60), receiver.wait_for(Option::is_some)).await;
    let Ok(Ok(engine)) = ready.map(|result| result.map(|engine| engine.clone())) else {
        return json!({ "css": "", "script": "" });
    };
    let Some(engine) = engine else { return json!({ "css": "", "script": "" }) };
    let resources = engine.url_cosmetic_resources(url);
    let mut selectors: Vec<&String> = resources.hide_selectors.iter().collect();
    selectors.sort();
    // One rule each: a selector the browser rejects drops its whole rule
    let css = selectors
        .iter()
        .map(|selector| format!("{selector} {{ display: none !important; }}\n"))
        .collect::<String>();
    json!({ "css": css, "script": resources.injected_script })
}

/// Windows: filters the main webview's requests through [`check`].
#[cfg(windows)]
pub fn install(window: &tauri::WebviewWindow) {
    use webview2_com::{
        take_pwstr,
        Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_22, COREWEBVIEW2_WEB_RESOURCE_CONTEXT, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_CSP_VIOLATION_REPORT, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST, COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
        },
        WebResourceRequestedEventHandler,
    };
    use windows::{
        core::{Interface, HSTRING, PWSTR},
        Win32::UI::Shell::SHCreateMemStream,
    };

    fn request_type(context: COREWEBVIEW2_WEB_RESOURCE_CONTEXT) -> &'static str {
        match context {
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => "document",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => "stylesheet",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => "image",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => "media",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => "font",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => "script",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => "xhr",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => "websocket",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => "ping",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_CSP_VIOLATION_REPORT => "csp_report",
            _ => "other",
        }
    }

    let installed = window.with_webview(|webview| {
        let environment = webview.environment();
        // SAFETY: WebView2 calls on the webview's own (main) thread, where
        // `with_webview` runs this
        let register = || -> windows::core::Result<()> {
            unsafe {
                let core = webview.controller().CoreWebView2()?;
                let filter = HSTRING::from("*");
                match core.cast::<ICoreWebView2_22>() {
                    // Service workers and iframes too
                    Ok(core) => core.AddWebResourceRequestedFilterWithRequestSourceKinds(
                        &filter,
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                        COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                    )?,
                    Err(_) => core.AddWebResourceRequestedFilter(&filter, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?,
                }
                let mut token = 0i64;
                core.add_WebResourceRequested(
                    &WebResourceRequestedEventHandler::create(Box::new(move |sender, args| {
                        let (Some(sender), Some(args)) = (sender, args) else { return Ok(()) };
                        if engine().borrow().is_none() {
                            return Ok(());
                        }
                        let request = args.Request()?;
                        let mut uri = PWSTR::null();
                        request.Uri(&mut uri)?;
                        let uri = take_pwstr(uri);
                        let mut source = PWSTR::null();
                        sender.Source(&mut source)?;
                        let source = take_pwstr(source);
                        let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
                        args.ResourceContext(&mut context)?;
                        let response = match check(&uri, &source, request_type(context)) {
                            Verdict::Allow => return Ok(()),
                            Verdict::Block => {
                                environment.CreateWebResourceResponse(None, 403, &HSTRING::from("Blocked"), &HSTRING::new())?
                            }
                            Verdict::Redirect(data_url) => {
                                // data:<mime>;base64,<body>
                                let (head, body) = data_url.split_once(',').unwrap_or_default();
                                let mime = head.trim_start_matches("data:").trim_end_matches(";base64");
                                let body = BASE64.decode(body).unwrap_or_default();
                                let stream = SHCreateMemStream(Some(&body));
                                environment.CreateWebResourceResponse(
                                    stream.as_ref(),
                                    200,
                                    &HSTRING::from("OK"),
                                    &HSTRING::from(format!("Content-Type: {mime}")),
                                )?
                            }
                        };
                        args.SetResponse(&response)?;
                        Ok(())
                    })),
                    &mut token,
                )?;
                Ok(())
            }
        };
        if let Err(error) = register() {
            log::error!("do-not-track: filtering requests is unavailable: {error}");
        }
    });
    if let Err(error) = installed {
        log::error!("do-not-track: filtering requests is unavailable: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_listed_requests_and_runs_scriptlets() {
        let resources = r#"{
            "scriptlets": [
                { "name": "helper.fn", "aliases": [], "body": "function helper(){return 1}", "dependencies": [] },
                { "name": "set-constant.js", "aliases": ["set.js"], "body": "function setConstant(a,b){helper()}", "dependencies": ["helper.fn"] }
            ],
            "redirects": [
                { "name": "noop.js", "aliases": [], "body": "(function(){})()", "contentType": "application/javascript" }
            ]
        }"#;
        let mut filters = FilterSet::new(false);
        filters.add_filter_list(
            "||tracker.example^\n||ads.example/ad.js$script,redirect=noop.js\nmusic.youtube.com##+js(set, foo, 1)\nmusic.youtube.com##.ad-slot"
                .into(),
            ParseOptions::default(),
        );
        let mut engine = Engine::new_with_filter_set(filters);
        engine.use_resources(parse_resources(resources));
        let request = |url: &str, kind: &str| {
            engine.check_network_request(&Request::new(url, "https://music.youtube.com/", kind, "GET").unwrap())
        };
        assert!(request("https://tracker.example/pixel", "image").should_block());
        assert!(!request("https://music.youtube.com/youtubei/v1/next", "xhr").should_block());
        let redirected = request("https://ads.example/ad.js", "script");
        assert!(redirected.should_block());
        assert!(redirected.redirect.unwrap().starts_with("data:application/javascript;base64,"));
        let cosmetic = engine.url_cosmetic_resources("https://music.youtube.com/watch?v=x");
        assert!(cosmetic.hide_selectors.contains(".ad-slot"));
        assert!(cosmetic.injected_script.contains("setConstant(\"foo\", \"1\")"), "{}", cosmetic.injected_script);
        assert!(cosmetic.injected_script.contains("function helper"));
    }
}
