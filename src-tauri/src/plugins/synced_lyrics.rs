//! Port of `src/plugins/synced-lyrics/backend.ts`: a fetch the renderer can
//! use for requests that need forbidden headers (MusixMatch's `Cookie`).
//! Answers `[status, text, headers]`, as the Electron handler does.

use std::collections::HashMap;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use super::Plugin;
use crate::ipc::{arg, Ipc};

pub struct SyncedLyrics;

/// `RequestInit.headers` as an object or as `[name, value]` pairs.
fn headers_of(init: &Value) -> HashMap<String, String> {
    match &init["headers"] {
        Value::Object(map) => map
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
            .collect(),
        Value::Array(pairs) => pairs
            .iter()
            .filter_map(|pair| Some((pair[0].as_str()?.to_owned(), pair[1].as_str()?.to_owned())))
            .collect(),
        _ => HashMap::new(),
    }
}

impl Plugin for SyncedLyrics {
    fn id(&self) -> &'static str {
        "synced-lyrics"
    }

    fn start(&self, app: &AppHandle, _config: &Value) {
        app.state::<Ipc>().handle(self.id(), "synced-lyrics:fetch", |_, args| async move {
            let url: String = arg(&args, 0)?;
            if !url.starts_with("https://") {
                return Err("synced-lyrics:fetch only allows https".into());
            }
            let init: Value = arg(&args, 1)?;
            let mut request = crate::net::client().request(
                init["method"].as_str().unwrap_or("GET").parse().map_err(|_| "bad method")?,
                &url,
            );
            for (name, value) in headers_of(&init) {
                request = request.header(name, value);
            }
            if let Some(body) = init["body"].as_str() {
                request = request.body(body.to_owned());
            }
            let response = request.send().await.map_err(|e| e.to_string())?;
            let status = response.status().as_u16();
            // Multiple values (e.g. set-cookie) are joined, like `Headers.entries()`
            let mut headers: HashMap<String, String> = HashMap::new();
            for (name, value) in response.headers() {
                let Ok(value) = value.to_str() else { continue };
                headers
                    .entry(name.to_string())
                    .and_modify(|existing| {
                        existing.push_str(", ");
                        existing.push_str(value);
                    })
                    .or_insert_with(|| value.to_owned());
            }
            let text = response.text().await.map_err(|e| e.to_string())?;
            Ok(json!([status, text, headers]))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_headers_from_object_or_pairs() {
        let object = headers_of(&json!({ "headers": { "Cookie": "a=1" } }));
        let pairs = headers_of(&json!({ "headers": [["Cookie", "a=1"]] }));
        assert_eq!(object.get("Cookie").map(String::as_str), Some("a=1"));
        assert_eq!(object, pairs);
    }
}
