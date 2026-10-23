//! HTTP for Rust code (the role of Electron's `net.fetch`) and the page's
//! `net_fetch` command, which `src/providers/net.ts` uses on Tauri for
//! cross-origin requests that Electron allowed by stripping CORS headers.
//!
//! Any script on music.youtube.com can call `net_fetch`, so it only reaches
//! hosts listed in [`PAGE_FETCH_HOSTS`].

use std::{collections::HashMap, sync::OnceLock};

use serde::{Deserialize, Serialize};

/// Hosts the page may fetch through Rust: the synced-lyrics providers that
/// use `crossOriginFetch` (MusixMatch goes through `synced-lyrics:fetch`).
const PAGE_FETCH_HOSTS: &[&str] = &[
    "lrclib.net",
    "genius.com",
    "www.megalobiz.com",
    "ytmbrowseproxy.zvz.be",
];

pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(concat!("PearDesktop-Tauri/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("building the HTTP client")
    })
}

#[derive(Deserialize, Default)]
pub struct FetchInit {
    method: Option<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
    body: Option<String>,
}

#[derive(Serialize)]
pub struct FetchResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

fn host_allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| PAGE_FETCH_HOSTS.contains(&host))
}

pub async fn fetch(url: &str, init: FetchInit) -> Result<FetchResponse, String> {
    let url = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
    let method = init
        .method
        .as_deref()
        .unwrap_or("GET")
        .parse::<reqwest::Method>()
        .map_err(|e| e.to_string())?;
    let mut request = client().request(method, url);
    for (name, value) in &init.headers {
        request = request.header(name, value);
    }
    if let Some(body) = init.body {
        request = request.body(body);
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_owned())))
        .collect();
    let body = response.text().await.map_err(|e| e.to_string())?;
    Ok(FetchResponse { status, headers, body })
}

#[tauri::command]
pub async fn net_fetch(url: String, init: Option<FetchInit>) -> Result<FetchResponse, String> {
    let parsed = reqwest::Url::parse(&url).map_err(|e| e.to_string())?;
    if !host_allowed(&parsed) {
        return Err(format!("net_fetch: {} is not an allowed host", parsed.host_str().unwrap_or("?")));
    }
    fetch(&url, init.unwrap_or_default()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_listed_https_hosts_are_allowed() {
        let allowed = |url: &str| host_allowed(&reqwest::Url::parse(url).unwrap());
        assert!(allowed("https://lrclib.net/api/get"));
        assert!(!allowed("http://lrclib.net/api/get"));
        assert!(!allowed("https://evil.example/lrclib.net"));
        assert!(!allowed("https://lrclib.net.evil.example/"));
    }
}
