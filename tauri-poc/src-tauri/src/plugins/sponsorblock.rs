//! Port of the `backend` of `src/plugins/sponsorblock`: on every new video,
//! fetches its skip segments and hands them to the renderer part.

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use super::Plugin;
use crate::ipc::{send_to_page, Ipc};

pub struct SponsorBlock;

type Segment = [f64; 2];

/// Port of `sortSegments`: sorts, then merges overlapping segments.
fn sort_segments(mut segments: Vec<Segment>) -> Vec<Segment> {
    segments.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    let mut merged: Vec<Segment> = Vec::new();
    for segment in segments {
        match merged.last_mut() {
            Some(current) if current[1] >= segment[0] => current[1] = current[1].max(segment[1]),
            _ => merged.push(segment),
        }
    }
    merged
}

async fn fetch_segments(api_url: &str, categories: &Value, video_id: &str) -> Vec<Segment> {
    let Ok(mut url) = url::Url::parse(&format!("{}/api/skipSegments", api_url.trim_end_matches('/'))) else {
        return Vec::new();
    };
    url.query_pairs_mut()
        .append_pair("videoID", video_id)
        .append_pair("categories", &categories.to_string());
    let Ok(response) = crate::net::client().get(url).send().await else { return Vec::new() };
    if response.status() != 200 {
        return Vec::new();
    }
    let submissions: Vec<Value> = response.json().await.unwrap_or_default();
    sort_segments(
        submissions
            .iter()
            .filter_map(|s| Some([s["segment"][0].as_f64()?, s["segment"][1].as_f64()?]))
            .collect(),
    )
}

impl Plugin for SponsorBlock {
    fn id(&self) -> &'static str {
        "sponsorblock"
    }

    fn start(&self, app: &AppHandle, config: &Value) {
        // Read once, like the original (`restartNeeded: true`)
        let api_url = config["apiURL"].as_str().unwrap_or("https://sponsor.ajay.app").to_owned();
        let categories = config["categories"].clone();
        app.state::<Ipc>().on(self.id(), "peard:video-src-changed", move |app, args| {
            let Some(video_id) = args.first().and_then(|d| d["videoDetails"]["videoId"].as_str()) else {
                return;
            };
            let (app, api_url, categories, video_id) =
                (app.clone(), api_url.clone(), categories.clone(), video_id.to_owned());
            tauri::async_runtime::spawn(async move {
                let segments = fetch_segments(&api_url, &categories, &video_id).await;
                send_to_page(&app, "sponsorblock-skip", &[json!(segments)]);
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_overlapping_segments() {
        let segments = vec![[30.0, 40.0], [0.0, 10.0], [5.0, 12.0], [40.0, 45.0], [50.0, 60.0]];
        assert_eq!(sort_segments(segments), vec![[0.0, 12.0], [30.0, 45.0], [50.0, 60.0]]);
    }
}
