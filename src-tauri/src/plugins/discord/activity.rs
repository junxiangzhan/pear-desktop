//! The Rich Presence activity for a song: `buildActivityInfo` and the
//! string helpers of src/plugins/discord/utils.ts.

use serde_json::{json, Value};

use crate::song_info::SongInfo;

const HANGUL_FILLER: char = '\u{3164}';
const APPLICATION_NAME: &str = "\u{59}\u{6f}\u{75}\u{54}\u{75}\u{62}\u{65} \u{4d}\u{75}\u{73}\u{69}\u{63}";
const PLAY_ON_KEY: &str = "playOn\u{59}\u{6f}\u{75}\u{54}\u{75}\u{62}\u{65}\u{4d}\u{75}\u{73}\u{69}\u{63}";

/// Like `isSeek` in utils.ts.
pub(super) fn is_seek(old_seconds: f64, new_seconds: f64) -> bool {
    (new_seconds - old_seconds).abs() > 2.0
}

/// Like `truncateString` in utils.ts, counting UTF-16 units like JS.
fn truncate(text: &str, length: usize) -> String {
    if text.encode_utf16().count() <= length {
        return text.to_owned();
    }
    let mut units = 0;
    let kept: String = text
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= length - 3
        })
        .collect();
    format!("{kept}...")
}

/// Like `sanitizeActivityText` in utils.ts: Discord wants 2 to 128 characters.
fn sanitize(input: Option<&str>) -> String {
    let text = input.map(str::trim).filter(|t| !t.is_empty()).unwrap_or("undefined");
    let mut text = truncate(text, 128);
    if text.encode_utf16().count() < 2 {
        text.push('⠀');
    }
    text
}

/// Like `padHangulFields` in utils.ts.
fn pad_short(value: &str) -> String {
    match value.encode_utf16().count() {
        1 => format!("{value}{HANGUL_FILLER}"),
        _ => value.to_owned(),
    }
}

/// `buildActivityInfo` plus the RPC client's `setActivity` formatting.
pub(super) fn build_activity(song: &SongInfo, config: &Value, now_ms: f64) -> Value {
    let title = pad_short(&song.title);
    let artist = pad_short(&song.artist);
    let album = song.album.as_deref().map(pad_short);

    let mut activity = json!({
        "type": 2, // Listening
        "created_at": now_ms as u64,
        "instance": false,
        "details": sanitize(Some(song.alternative_title.as_deref().unwrap_or(&title))),
        "state": sanitize(Some(song.tags.first().map(String::as_str).unwrap_or(&artist))),
    });
    if let Some(display_type) = config.get("statusDisplayType").filter(|v| !v.is_null()) {
        activity["status_display_type"] = display_type.clone();
    }
    if let Some(url) = song.url.as_deref().filter(|u| !u.is_empty()) {
        activity["details_url"] = json!(url);
    }
    if let Some(url) = song.artist_url.as_deref().filter(|u| !u.is_empty()) {
        activity["state_url"] = json!(url);
    }

    let mut assets = serde_json::Map::new();
    if let Some(image) = song.image_src.as_deref().filter(|i| !i.is_empty()) {
        assets.insert("large_image".into(), json!(image));
    }
    let large_text = if song.is_paused == Some(true) {
        Some("⏸︎".to_owned())
    } else {
        album.filter(|a| !a.is_empty()).map(|a| sanitize(Some(&a)))
    };
    if let Some(text) = large_text {
        assets.insert("large_text".into(), json!(text));
    }
    if !assets.is_empty() {
        activity["assets"] = Value::Object(assets);
    }

    let hide_duration = config["hideDurationLeft"].as_bool().unwrap_or(false);
    if song.is_paused != Some(true) && !hide_duration && song.song_duration > 0.0 {
        if let Some(elapsed) = song.elapsed_seconds {
            let start = now_ms - elapsed * 1000.0;
            activity["timestamps"] = json!({
                "start": (start / 1000.0).floor() as i64,
                "end": ((start + song.song_duration * 1000.0) / 1000.0).floor() as i64,
            });
        }
    }

    let mut buttons = Vec::new();
    if config[PLAY_ON_KEY].as_bool().unwrap_or(true) {
        if let Some(url) = song.url.as_deref().filter(|u| !u.is_empty()) {
            buttons.push(json!({ "label": format!("Play on {APPLICATION_NAME}"), "url": url }));
        }
    }
    if !config["hideGitHubButton"].as_bool().unwrap_or(false) {
        buttons.push(json!({
            "label": "View App On GitHub",
            "url": "https://github.com/pear-devs/pear-desktop",
        }));
    }
    if !buttons.is_empty() {
        activity["buttons"] = json!(buttons);
    }
    activity
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> SongInfo {
        SongInfo {
            title: "Song".into(),
            artist: "Artist".into(),
            album: Some("A".into()),
            video_id: "abc".into(),
            url: Some("https://music.youtube.com/watch?v=abc".into()),
            image_src: Some("https://i.ytimg.com/x.jpg".into()),
            song_duration: 200.0,
            elapsed_seconds: Some(20.0),
            is_paused: Some(false),
            ..Default::default()
        }
    }

    #[test]
    fn activity_matches_the_electron_payload() {
        let config = json!({ "statusDisplayType": 2, PLAY_ON_KEY: true, "hideGitHubButton": false });
        let activity = build_activity(&song(), &config, 1_000_000.0);
        assert_eq!(activity["type"], 2);
        assert_eq!(activity["details"], "Song");
        assert_eq!(activity["state"], "Artist");
        assert_eq!(activity["status_display_type"], 2);
        assert_eq!(activity["assets"]["large_text"], format!("A{HANGUL_FILLER}"));
        assert_eq!(activity["timestamps"], json!({ "start": 980, "end": 1180 }));
        assert_eq!(activity["buttons"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn paused_activity_has_no_timestamps() {
        let mut song = song();
        song.is_paused = Some(true);
        let activity = build_activity(&song, &json!({ "hideGitHubButton": true, PLAY_ON_KEY: false }), 0.0);
        assert_eq!(activity["assets"]["large_text"], "⏸︎");
        assert!(activity.get("timestamps").is_none());
        assert!(activity.get("buttons").is_none());
    }

    #[test]
    fn text_is_sanitized_like_utils_ts() {
        assert_eq!(sanitize(Some("  ")), "undefined");
        assert_eq!(sanitize(Some("x")), "x⠀");
        let long = "a".repeat(200);
        assert_eq!(sanitize(Some(&long)), format!("{}...", "a".repeat(125)));
    }
}
