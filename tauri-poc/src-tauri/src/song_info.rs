//! Port of `src/providers/song-info.ts`: turns the player response the page
//! sends (`peard:video-src-changed`, see `src/providers/song-info-front.ts`)
//! into a [`SongInfo`], keeps it current on play/pause and time updates, and
//! notifies registered callbacks (the Rust side of `registerCallback`).
//!
//! Unlike Electron, the cover is not shipped as a `NativeImage`; consumers
//! that need the pixels fetch `image_src` themselves.

use std::sync::{Arc, Mutex, OnceLock};

use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::{
    config::Config,
    ipc::{self, Ipc},
};

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MediaType {
    /// Audio uploaded by the original artist
    #[default]
    Audio,
    /// Official music video uploaded by the original artist
    OriginalMusicVideo,
    /// Normal video uploaded by a user
    UserGeneratedContent,
    PodcastEpisode,
    OtherVideo,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SongInfo {
    pub title: String,
    pub alternative_title: Option<String>,
    pub artist: String,
    pub artist_url: Option<String>,
    pub views: u64,
    pub upload_date: Option<String>,
    pub image_src: Option<String>,
    pub is_paused: Option<bool>,
    pub song_duration: f64,
    pub elapsed_seconds: Option<f64>,
    pub url: Option<String>,
    pub album: Option<String>,
    pub video_id: String,
    pub playlist_id: Option<String>,
    pub media_type: MediaType,
    pub tags: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SongInfoEvent {
    VideoSrcChanged,
    PlayOrPaused,
    TimeChanged,
}

pub type Callback = Arc<dyn Fn(&AppHandle, &SongInfo, SongInfoEvent) + Send + Sync>;

#[derive(Default)]
pub struct SongInfoProvider {
    current: tokio::sync::Mutex<Option<SongInfo>>,
    callbacks: Mutex<Vec<(String, Callback)>>,
}

impl SongInfoProvider {
    /// Like `registerCallback`, with an owner so a stopped plugin's callbacks
    /// are dropped (Electron never unregistered them).
    pub fn register(
        &self,
        owner: &str,
        callback: impl Fn(&AppHandle, &SongInfo, SongInfoEvent) + Send + Sync + 'static,
    ) {
        self.callbacks
            .lock()
            .unwrap()
            .push((owner.to_owned(), Arc::new(callback)));
    }

    pub fn remove_owner(&self, owner: &str) {
        self.callbacks.lock().unwrap().retain(|(o, _)| o != owner);
    }

    fn notify(&self, app: &AppHandle, info: &SongInfo, event: SongInfoEvent) {
        let callbacks: Vec<Callback> = self
            .callbacks
            .lock()
            .unwrap()
            .iter()
            .map(|(_, c)| c.clone())
            .collect();
        for callback in callbacks {
            callback(app, info, event);
        }
    }
}

fn suffixes_to_remove() -> &'static [Regex] {
    static LIST: OnceLock<Vec<Regex>> = OnceLock::new();
    LIST.get_or_init(|| {
        [
            // Artist names
            r"(?i)\s*(- topic)$",
            r"(?i)\s*vevo$",
            // Video titles: (Official Music Video), [Official Visualizer], etc.
            r"(?i)\s*[(|\[]official(.*?)[)|\]]",
            r"(?i)\s*[(|\[]((lyrics?|visualizer|audio)\s*(video)?)[)|\]]",
            r"(?i)\s*[(|\[](performance video)[)|\]]",
            r"(?i)\s*[(|\[](clip official)[)|\]]",
            r"(?i)\s*[(|\[](video version)[)|\]]",
            r"(?i)\s*[(|\[](HD|HQ)\s*?(?:audio)?[)|\]]$",
            r"(?i)\s*[(|\[](live)[)|\]]$",
            r"(?i)\s*[(|\[]4K\s*?(?:upgrade)?[)|\]]$",
        ]
        .iter()
        .map(|pattern| Regex::new(pattern).expect("valid suffix pattern"))
        .collect()
    })
}

/// Port of `cleanupName`. Like JS `String.replace` with a non-global regex,
/// each pattern removes its first match only.
pub fn cleanup_name(name: &str) -> String {
    suffixes_to_remove()
        .iter()
        .fold(name.to_owned(), |name, suffix| suffix.replace(&name, "").into_owned())
}

fn str_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(value, |node, key| node.get(key))?.as_str()
}

/// Port of the pure part of `handleData`: everything except the network
/// check of the thumbnail and the side effects.
pub fn parse_player_response(data: &Value, use_podcast_participant_as_artist: bool) -> Option<SongInfo> {
    if data.is_null() {
        return None;
    }
    let mut info = SongInfo {
        alternative_title: Some(String::new()),
        artist_url: Some(String::new()),
        upload_date: Some(String::new()),
        image_src: Some(String::new()),
        elapsed_seconds: Some(0.0),
        url: Some(String::new()),
        playlist_id: Some(String::new()),
        ..Default::default()
    };

    let microformat = data.pointer("/microformat/microformatDataRenderer");
    if let Some(microformat) = microformat {
        info.upload_date = microformat["uploadDate"].as_str().map(str::to_owned);
        let canonical = microformat["urlCanonical"].as_str();
        info.url = canonical.and_then(|url| url.split('&').next()).map(str::to_owned);
        info.playlist_id = Some(
            canonical
                .and_then(|url| url::Url::parse(url).ok())
                .and_then(|url| url.query_pairs().find(|(k, _)| k == "list").map(|(_, v)| v.into_owned()))
                .unwrap_or_default(),
        );
        if let Some(channel) = str_at(microformat, &["pageOwnerDetails", "externalChannelId"]) {
            info.artist_url = Some(format!("https://music.youtube.com/channel/{channel}"));
        }
        info.alternative_title = microformat["linkAlternates"]
            .as_array()
            .and_then(|links| links.iter().find_map(|link| link["title"].as_str()))
            .map(str::to_owned);
        info.tags = microformat["tags"]
            .as_array()
            .map(|tags| tags.iter().filter_map(|t| t.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
    }

    let details = data.get("videoDetails").filter(|d| !d.is_null())?;
    let number = |value: &Value| match value {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    };
    info.title = cleanup_name(details["title"].as_str().unwrap_or_default());
    info.artist = cleanup_name(details["author"].as_str().unwrap_or_default());
    info.views = number(&details["viewCount"]) as u64;
    info.song_duration = number(&details["lengthSeconds"]);
    info.elapsed_seconds = details["elapsedSeconds"].as_f64();
    info.is_paused = details["isPaused"].as_bool();
    info.video_id = details["videoId"].as_str().unwrap_or_default().to_owned();
    info.album = details["album"].as_str().map(str::to_owned);

    let owner_name = || {
        microformat
            .and_then(|m| str_at(m, &["pageOwnerDetails", "name"]))
            .map(cleanup_name)
    };
    info.media_type = match details["musicVideoType"].as_str() {
        Some("MUSIC_VIDEO_TYPE_ATV") => MediaType::Audio,
        Some("MUSIC_VIDEO_TYPE_OMV") => MediaType::OriginalMusicVideo,
        Some("MUSIC_VIDEO_TYPE_UGC") => MediaType::UserGeneratedContent,
        Some("MUSIC_VIDEO_TYPE_PODCAST_EPISODE") => {
            // HACK: a podcast's participant is not the artist
            if !use_podcast_participant_as_artist {
                if let Some(name) = owner_name() {
                    info.artist = name;
                }
            }
            MediaType::PodcastEpisode
        }
        _ => {
            // HACK: "podcast" types where `musicVideoType` doesn't exist
            let ipcc = data
                .pointer("/responseContext/serviceTrackingParams/0/params")
                .and_then(Value::as_array)
                .and_then(|params| params.iter().find(|p| p["key"] == "ipcc"))
                .and_then(|p| p["value"].as_str())
                .unwrap_or("1");
            if !use_podcast_participant_as_artist && ipcc != "0" {
                if let Some(name) = owner_name() {
                    info.artist = name;
                }
            }
            MediaType::OtherVideo
        }
    };

    let last_thumbnail = details
        .pointer("/thumbnail/thumbnails")
        .and_then(Value::as_array)
        .and_then(|list| list.last())
        .and_then(|t| t["url"].as_str());
    info.image_src = last_thumbnail.map(|url| url.split('?').next().unwrap_or(url).to_owned());

    Some(info)
}

/// Port of `handleData`, including its side effects.
async fn handle_data(app: &AppHandle, data: &Value) -> Option<SongInfo> {
    let config = app.state::<Config>();
    let mut info = parse_player_response(data, config.get_bool("options.usePodcastParticipantAsArtist"))?;

    // Used for options.resumeOnStart
    if let Some(canonical) = data
        .pointer("/microformat/microformatDataRenderer/urlCanonical")
        .and_then(Value::as_str)
    {
        config.set(app, "url", json!(canonical));
    }

    // The query-less thumbnail is the larger one, when it exists
    if let Some(src) = info.image_src.clone().filter(|s| !s.is_empty()) {
        let ok = crate::net::client()
            .head(&src)
            .send()
            .await
            .is_ok_and(|response| response.status().is_success());
        if !ok {
            info.image_src = data
                .pointer("/videoDetails/thumbnail/thumbnails")
                .and_then(Value::as_array)
                .and_then(|list| list.last())
                .and_then(|t| t["url"].as_str())
                .map(str::to_owned);
        }
    }

    log::info!("song info: {} - {} ({})", info.artist, info.title, info.video_id);
    ipc::send_to_page(app, "peard:update-song-info", &[json!(info)]);
    Some(info)
}

/// Like `setupSongInfo(win)`: the provider's three IPC listeners.
pub fn register_ipc(ipc: &Ipc) {
    ipc.on("core", "peard:video-src-changed", |app, args| {
        let app = app.clone();
        let data = args.first().cloned().unwrap_or(Value::Null);
        tauri::async_runtime::spawn(async move {
            let provider = app.state::<SongInfoProvider>();
            // Held across the fetch, like the async-mutex in song-info.ts
            let mut current = provider.current.lock().await;
            let Some(info) = handle_data(&app, &data).await else { return };
            *current = Some(info.clone());
            drop(current);
            provider.notify(&app, &info, SongInfoEvent::VideoSrcChanged);
        });
    });

    ipc.on("core", "peard:play-or-paused", |app, args| {
        let app = app.clone();
        let state = args.first().cloned().unwrap_or(Value::Null);
        tauri::async_runtime::spawn(async move {
            let provider = app.state::<SongInfoProvider>();
            let mut current = provider.current.lock().await;
            let Some(info) = current.as_mut() else { return };
            info.is_paused = state["isPaused"].as_bool();
            info.elapsed_seconds = state["elapsedSeconds"].as_f64();
            let info = info.clone();
            drop(current);
            provider.notify(&app, &info, SongInfoEvent::PlayOrPaused);
        });
    });

    ipc.on("core", "peard:time-changed", |app, args| {
        let app = app.clone();
        let seconds = args.first().and_then(Value::as_f64);
        tauri::async_runtime::spawn(async move {
            let provider = app.state::<SongInfoProvider>();
            let mut current = provider.current.lock().await;
            let Some(info) = current.as_mut() else { return };
            info.elapsed_seconds = seconds;
            let info = info.clone();
            drop(current);
            provider.notify(&app, &info, SongInfoEvent::TimeChanged);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_name_strips_known_suffixes() {
        assert_eq!(cleanup_name("Artist - Topic"), "Artist");
        assert_eq!(cleanup_name("ArtistVEVO"), "Artist");
        assert_eq!(cleanup_name("Song (Official Music Video)"), "Song");
        assert_eq!(cleanup_name("Song [Lyric Video]"), "Song");
        assert_eq!(cleanup_name("Song (Visualizer)"), "Song");
        assert_eq!(cleanup_name("Song (HD Audio)"), "Song");
        assert_eq!(cleanup_name("Song (Live)"), "Song");
        assert_eq!(cleanup_name("Song (4K Upgrade)"), "Song");
        assert_eq!(cleanup_name("Live (Song)"), "Live (Song)");
    }

    fn response(music_video_type: Option<&str>, ipcc: &str) -> Value {
        json!({
            "responseContext": {
                "serviceTrackingParams": [{ "params": [{ "key": "ipcc", "value": ipcc }] }]
            },
            "microformat": { "microformatDataRenderer": {
                "uploadDate": "2009-10-25",
                "urlCanonical": "https://music.youtube.com/watch?v=dQw4w9WgXcQ&list=PL123",
                "pageOwnerDetails": { "name": "Owner - Topic", "externalChannelId": "UC42" },
                "linkAlternates": [{ "hrefUrl": "x" }, { "title": "Alt title" }],
                "tags": ["a", "b"]
            }},
            "videoDetails": {
                "title": "Never Gonna Give You Up (Official Music Video)",
                "author": "Rick Astley - Topic",
                "viewCount": "1800000000",
                "lengthSeconds": "213",
                "elapsedSeconds": 0,
                "isPaused": false,
                "videoId": "dQw4w9WgXcQ",
                "album": "Whenever You Need Somebody",
                "musicVideoType": music_video_type,
                "thumbnail": { "thumbnails": [
                    { "url": "https://i.ytimg.com/small.jpg?x=1" },
                    { "url": "https://i.ytimg.com/large.jpg?sqp=abc" }
                ]}
            }
        })
    }

    #[test]
    fn parses_a_music_track() {
        let info = parse_player_response(&response(Some("MUSIC_VIDEO_TYPE_ATV"), "1"), false).unwrap();
        assert_eq!(info.title, "Never Gonna Give You Up");
        assert_eq!(info.artist, "Rick Astley");
        assert_eq!(info.views, 1_800_000_000);
        assert_eq!(info.song_duration, 213.0);
        assert_eq!(info.media_type, MediaType::Audio);
        assert_eq!(info.album.as_deref(), Some("Whenever You Need Somebody"));
        assert_eq!(info.url.as_deref(), Some("https://music.youtube.com/watch?v=dQw4w9WgXcQ"));
        assert_eq!(info.playlist_id.as_deref(), Some("PL123"));
        assert_eq!(info.artist_url.as_deref(), Some("https://music.youtube.com/channel/UC42"));
        assert_eq!(info.alternative_title.as_deref(), Some("Alt title"));
        assert_eq!(info.image_src.as_deref(), Some("https://i.ytimg.com/large.jpg"));
        assert_eq!(info.tags, ["a", "b"]);
    }

    #[test]
    fn podcast_artist_is_the_page_owner_unless_configured() {
        let data = response(Some("MUSIC_VIDEO_TYPE_PODCAST_EPISODE"), "1");
        assert_eq!(parse_player_response(&data, false).unwrap().artist, "Owner");
        assert_eq!(parse_player_response(&data, true).unwrap().artist, "Rick Astley");
    }

    #[test]
    fn untyped_video_uses_owner_only_when_ipcc_is_not_zero() {
        let info = parse_player_response(&response(None, "0"), false).unwrap();
        assert_eq!(info.media_type, MediaType::OtherVideo);
        assert_eq!(info.artist, "Rick Astley");
        assert_eq!(parse_player_response(&response(None, "1"), false).unwrap().artist, "Owner");
    }

    #[test]
    fn missing_video_details_yields_nothing() {
        assert!(parse_player_response(&json!({ "microformat": {} }), false).is_none());
    }
}
