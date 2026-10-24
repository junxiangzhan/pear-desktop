//! Interactive Windows toasts, port of `src/plugins/notifications/interactive.ts`:
//! the seven `toastStyle`s, with previous / play-pause / next buttons.
//!
//! Electron's buttons used protocol activation (`peard://next`, handled by
//! a second instance); here they activate in-process through the toast's
//! `Activated` event, which needs the app running. Like Electron, the toast
//! is hidden after 5 seconds, so it never waits in the Action Center.
//! Unlike Electron, song titles are XML-escaped (a `&` broke the toast).

use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use tauri::{AppHandle, Manager};
use windows::{
    core::{IInspectable, Interface, Ref, HSTRING},
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    UI::Notifications::{
        ToastActivatedEventArgs, ToastDismissedEventArgs, ToastNotification, ToastNotificationManager,
        ToastNotifier,
    },
    Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED},
};

use crate::song_info::SongInfo;

// `ToastStyles` (constants.ts)
const LOGO: u64 = 1;
const BANNER_CENTERED_TOP: u64 = 2;
const HERO: u64 = 3;
const BANNER_TOP_CUSTOM: u64 = 4;
const BANNER_CENTERED_BOTTOM: u64 = 5;
const BANNER_BOTTOM: u64 = 6;
const LEGACY: u64 = 7;

const HIDE_AFTER: Duration = Duration::from_secs(5);
/// Unpackaged runs (`target\debug`, `target\release`) have no registered
/// AppUserModelID; tauri-plugin-notification shows theirs as PowerShell's.
const POWERSHELL_APP_ID: &str = "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

/// What a button press asks for: `previous`, `play`, `pause`, `next`, or
/// empty for a click on the toast itself.
pub type OnAction = Arc<dyn Fn(&AppHandle, &str) + Send + Sync>;

#[derive(Default)]
pub struct Toaster {
    shown: Mutex<Option<(ToastNotifier, ToastNotification)>>,
    generation: AtomicU64,
    last_cover: Mutex<Option<PathBuf>>,
}

impl Toaster {
    pub fn is_shown(&self) -> bool {
        self.shown.lock().unwrap().is_some()
    }

    /// Like `sendNotification`: replaces the current toast.
    pub async fn show(self: &Arc<Self>, app: &AppHandle, song: SongInfo, style: u64, on_action: OnAction) {
        let image = self.image_file(app, &song, style).await;
        let icons = media_icons(app);
        let xml = toast_xml(&song, style, image.as_deref(), icons.as_deref());
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let toaster = self.clone();
        let app_for_toast = app.clone();
        let shown = tauri::async_runtime::spawn_blocking(move || {
            toaster.show_xml(&app_for_toast, &xml, generation, on_action)
        })
        .await;
        match shown {
            Ok(Ok(())) => {}
            Ok(Err(error)) => log::warn!("notifications: showing the toast failed: {error}"),
            Err(error) => log::warn!("notifications: showing the toast failed: {error}"),
        }
        // Electron's "fix the notification not closing"
        let toaster = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(HIDE_AFTER).await;
            if toaster.generation.load(Ordering::SeqCst) == generation {
                toaster.hide().await;
            }
        });
    }

    pub async fn hide(self: &Arc<Self>) {
        let toaster = self.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            init_com();
            if let Some((notifier, toast)) = toaster.shown.lock().unwrap().take() {
                let _ = notifier.Hide(&toast);
            }
        })
        .await;
    }

    fn show_xml(
        self: &Arc<Self>,
        app: &AppHandle,
        xml: &str,
        generation: u64,
        on_action: OnAction,
    ) -> windows::core::Result<()> {
        init_com();
        let document = XmlDocument::new()?;
        document.LoadXml(&HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&document)?;
        let app_id = app_id(&app.config().identifier);

        let app = app.clone();
        toast.Activated(&TypedEventHandler::new(
            move |_: Ref<ToastNotification>, args: Ref<IInspectable>| {
                let action = args
                    .as_ref()
                    .and_then(|args| args.cast::<ToastActivatedEventArgs>().ok())
                    .and_then(|args| args.Arguments().ok())
                    .map(|arguments| arguments.to_string())
                    .unwrap_or_default();
                on_action(&app, &action);
                Ok(())
            },
        ))?;
        // Like Electron's 'close' listener: a dismissed toast is gone
        let toaster = Arc::downgrade(self);
        toast.Dismissed(&TypedEventHandler::new(
            move |_: Ref<ToastNotification>, _: Ref<ToastDismissedEventArgs>| {
                if let Some(toaster) = toaster.upgrade() {
                    if toaster.generation.load(Ordering::SeqCst) == generation {
                        toaster.shown.lock().unwrap().take();
                    }
                }
                Ok(())
            },
        ))?;

        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))?;
        if let Some((previous_notifier, previous)) = self.shown.lock().unwrap().take() {
            let _ = previous_notifier.Hide(&previous);
        }
        notifier.Show(&toast)?;
        *self.shown.lock().unwrap() = Some((notifier, toast));
        Ok(())
    }

    /// Like `notificationImage`: the cover, cut to a 256 px square for the
    /// logo styles, or the app icon when there is none. Each song gets its
    /// own file, since Windows caches toast images by path.
    async fn image_file(&self, app: &AppHandle, song: &SongInfo, style: u64) -> Option<PathBuf> {
        let dir = app.path().app_cache_dir().ok()?.join("toasts");
        std::fs::create_dir_all(&dir).ok()?;
        let logo = matches!(style, LOGO | LEGACY);
        let cover = async {
            let src = song.image_src.as_deref().filter(|src| !src.is_empty())?;
            let bytes = crate::net::client().get(src).send().await.ok()?.bytes().await.ok()?;
            let path = dir.join(format!(
                "cover-{}-{}.png",
                song.video_id,
                if logo { "logo" } else { "banner" }
            ));
            let target = path.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let mut image = image::load_from_memory(&bytes).ok()?;
                if logo {
                    image = image.resize(u32::MAX, 256, image::imageops::FilterType::Triangle);
                    let margin = image.width().saturating_sub(256);
                    image = image.crop_imm(margin / 2, 0, image.width().min(256), image.height());
                }
                image.save_with_format(&target, image::ImageFormat::Png).ok()
            })
            .await
            .ok()??;
            Some(path)
        }
        .await;
        let Some(cover) = cover else {
            let icon = dir.join("icon.png");
            if !icon.exists() {
                std::fs::write(&icon, include_bytes!("../../../../../assets/icon.png")).ok()?;
            }
            return Some(icon);
        };
        let previous = self.last_cover.lock().unwrap().replace(cover.clone());
        if let Some(previous) = previous.filter(|previous| *previous != cover) {
            let _ = std::fs::remove_file(previous);
        }
        Some(cover)
    }
}

/// The installed app's AppUserModelID (its identifier, set on the Start
/// menu shortcut by the installer); PowerShell's for unpackaged runs.
fn app_id(identifier: &str) -> String {
    let unpackaged = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .is_some_and(|dir| dir.ends_with("target\\debug") || dir.ends_with("target\\release"));
    if unpackaged || identifier.is_empty() {
        POWERSHELL_APP_ID.to_owned()
    } else {
        identifier.to_owned()
    }
}

/// WinRT calls from pool threads need COM; already-initialized is fine.
fn init_com() {
    // SAFETY: initializing COM on the current thread has no preconditions
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

/// The button icons (`assets/media-icons-black`), written once next to the
/// covers so the toast can load them.
fn media_icons(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_cache_dir().ok()?.join("toasts").join("media-icons");
    let icons: [(&str, &[u8]); 4] = [
        ("play.png", include_bytes!("../../../../../assets/media-icons-black/play.png")),
        ("pause.png", include_bytes!("../../../../../assets/media-icons-black/pause.png")),
        ("next.png", include_bytes!("../../../../../assets/media-icons-black/next.png")),
        ("previous.png", include_bytes!("../../../../../assets/media-icons-black/previous.png")),
    ];
    std::fs::create_dir_all(&dir).ok()?;
    for (name, bytes) in icons {
        let path = dir.join(name);
        if !path.exists() {
            std::fs::write(path, bytes).ok()?;
        }
    }
    Some(dir)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// `secondsToMinutes`
fn minutes(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn button(kind: &str, style: u64, icons: Option<&Path>) -> String {
    let display = if style == LEGACY || icons.is_none() {
        let icon = match kind {
            "play" => "\u{1405}",
            "pause" => "\u{2016}",
            "next" => "\u{1433}",
            _ => "\u{1438}",
        };
        format!(r#"content="{icon}""#)
    } else {
        let path = icons.unwrap_or(Path::new("")).join(format!("{kind}.png"));
        format!(r#"content="" imageUri="file:///{}""#, escape(&path.display().to_string()))
    };
    format!(r#"<action {display} activationType="foreground" arguments="{kind}"/>"#)
}

fn toast(content: &str, paused: bool, style: u64, icons: Option<&Path>) -> String {
    format!(
        r#"<toast>
    <audio silent="true" />
    <visual>
        <binding template="ToastGeneric">
            {content}
        </binding>
    </visual>
    <actions>
        {}
        {}
        {}
    </actions>
</toast>"#,
        button("previous", style, icons),
        button(if paused { "play" } else { "pause" }, style, icons),
        button("next", style, icons),
    )
}

/// `titleFontPicker`
fn title_style(title: &str) -> &'static str {
    match title.chars().count() {
        0..=13 => "Header",
        14..=22 => "Subheader",
        23..=26 => "Title",
        _ => "Subtitle",
    }
}

fn toast_xml(song: &SongInfo, style: u64, image: Option<&Path>, icons: Option<&Path>) -> String {
    let title = escape(&song.title);
    let artist = escape(&song.artist);
    let image = escape(&image.map(|path| path.display().to_string()).unwrap_or_default());
    let paused = song.is_paused.unwrap_or(false);
    let with_image = |placement: &str| {
        toast(
            &format!(
                r#"<image id="1" src="{image}" name="Image" {placement}/>
            <text id="1">{title}</text>
            <text id="2">{artist}</text>"#
            ),
            paused,
            style,
            icons,
        )
    };
    let centered = |title_style: &str| {
        format!(
            r#"<text>ㅤ</text>
            <group>
                <subgroup hint-weight="1" hint-textStacking="center">
                    <text hint-align="center" hint-style="{title_style}">{title}</text>
                    <text hint-align="center" hint-style="SubtitleSubtle">{artist}</text>
                </subgroup>
            </group>"#
        )
    };
    match style {
        HERO => with_image(r#"placement="hero""#),
        BANNER_BOTTOM => with_image(""),
        BANNER_TOP_CUSTOM => {
            let album = song
                .album
                .as_deref()
                .filter(|album| !album.is_empty())
                .map(|album| {
                    format!(
                        r#"<text hint-style="captionSubtle" hint-wrap="true" hint-align="right">{}</text>"#,
                        escape(album)
                    )
                })
                .unwrap_or_default();
            let time = format!(
                "{} / {}",
                minutes(song.elapsed_seconds.unwrap_or(0.0)),
                minutes(song.song_duration)
            );
            toast(
                &format!(
                    r#"<image id="1" src="{image}" name="Image" />
            <text>ㅤ</text>
            <group>
                <subgroup>
                    <text hint-style="body">{title}</text>
                    <text hint-style="captionSubtle">{artist}</text>
                </subgroup>
                <subgroup hint-textStacking="bottom">
                    {album}
                    <text hint-style="captionSubtle" hint-wrap="true" hint-align="right">{time}</text>
                </subgroup>
            </group>"#
                ),
                paused,
                style,
                icons,
            )
        }
        BANNER_CENTERED_BOTTOM => toast(
            &format!(
                r#"{}
            <image id="1" src="{image}" name="Image"  hint-removeMargin="true" />"#,
                centered(title_style(&song.title))
            ),
            paused,
            style,
            icons,
        ),
        BANNER_CENTERED_TOP => toast(
            &format!(
                r#"<image id="1" src="{image}" name="Image" />
            {}"#,
                centered(title_style(&song.title))
            ),
            paused,
            style,
            icons,
        ),
        // LOGO, LEGACY and unknown values
        _ => with_image(r#"placement="appLogoOverride""#),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_xml_is_escaped_and_parses() {
        let song = SongInfo {
            title: "Rock & Roll <Live>".into(),
            artist: "AC/DC".into(),
            album: Some("\"Best\"".into()),
            song_duration: 245.0,
            elapsed_seconds: Some(65.4),
            ..SongInfo::default()
        };
        for style in 1..=7 {
            let xml = toast_xml(&song, style, Some(Path::new("C:\\covers\\a&b.png")), Some(Path::new("C:\\icons")));
            assert!(xml.contains("Rock &amp; Roll &lt;Live&gt;"), "style {style}");
            assert!(!xml.contains("a&b"), "style {style}");
            XmlDocument::new().unwrap().LoadXml(&HSTRING::from(xml)).unwrap();
        }
        assert!(toast_xml(&song, BANNER_TOP_CUSTOM, None, None).contains("1:05 / 4:05"));
        assert!(toast_xml(&song, LEGACY, None, None).contains("content=\"\u{2016}\""));
    }

    #[test]
    fn title_sizes_follow_the_length() {
        assert_eq!(title_style("Short"), "Header");
        assert_eq!(title_style(&"a".repeat(20)), "Subheader");
        assert_eq!(title_style(&"a".repeat(30)), "Subtitle");
    }
}
