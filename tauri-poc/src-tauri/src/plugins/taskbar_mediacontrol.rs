//! Port of `src/plugins/taskbar-mediacontrol` (Windows): previous /
//! play-pause / next buttons in the taskbar thumbnail, via `ITaskbarList3`.
//! Clicks arrive as `WM_COMMAND` + `THBN_CLICKED` on the main window, caught
//! with a window subclass. All of it runs on the main thread.

use std::cell::RefCell;

use serde_json::Value;
use tauri::{AppHandle, Manager, Theme};
use windows::{
    core::w,
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
        UI::{
            Shell::{
                DefSubclassProc, ITaskbarList3, RemoveWindowSubclass, SetWindowSubclass, TaskbarList,
                THBF_ENABLED, THBF_HIDDEN, THB_FLAGS, THB_ICON, THB_TOOLTIP, THBN_CLICKED, THUMBBUTTON,
            },
            WindowsAndMessaging::{
                CreateIconFromResourceEx, DestroyIcon, RegisterWindowMessageW, HICON, LR_DEFAULTCOLOR,
                WM_COMMAND,
            },
        },
    },
};

use super::Plugin;
use crate::{
    ipc::MAIN_WEBVIEW,
    song_controls,
    song_info::SongInfoProvider,
};

const SUBCLASS_ID: usize = 0x5065_6172; // "Pear"
const PREVIOUS: u32 = 1;
const PLAY_PAUSE: u32 = 2;
const NEXT: u32 = 3;

struct Icons {
    previous: HICON,
    play: HICON,
    pause: HICON,
    next: HICON,
}

impl Drop for Icons {
    fn drop(&mut self) {
        for icon in [self.previous, self.play, self.pause, self.next] {
            let _ = unsafe { DestroyIcon(icon) };
        }
    }
}

struct State {
    app: AppHandle,
    hwnd: HWND,
    taskbar: Option<ITaskbarList3>,
    icons: Icons,
    paused: bool,
    added: bool,
}

thread_local! {
    // Main thread only: COM objects and window handles live here
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn icon(png: &[u8]) -> HICON {
    // Windows reads PNG-compressed icon images directly
    unsafe { CreateIconFromResourceEx(png, true, 0x0003_0000, 0, 0, LR_DEFAULTCOLOR) }.unwrap_or_default()
}

/// Same choice as Electron's `nativeTheme.shouldUseDarkColors`: white on dark.
fn load_icons(dark: bool) -> Icons {
    macro_rules! set {
        ($dir:literal) => {
            Icons {
                previous: icon(include_bytes!(concat!("../../../../assets/", $dir, "/previous.png"))),
                play: icon(include_bytes!(concat!("../../../../assets/", $dir, "/play.png"))),
                pause: icon(include_bytes!(concat!("../../../../assets/", $dir, "/pause.png"))),
                next: icon(include_bytes!(concat!("../../../../assets/", $dir, "/next.png"))),
            }
        };
    }
    if dark { set!("media-icons-white") } else { set!("media-icons-black") }
}

fn button(id: u32, icon: HICON, tip: &str, hidden: bool) -> THUMBBUTTON {
    let mut button = THUMBBUTTON {
        dwMask: THB_ICON | THB_TOOLTIP | THB_FLAGS,
        iId: id,
        hIcon: icon,
        dwFlags: if hidden { THBF_HIDDEN } else { THBF_ENABLED },
        ..Default::default()
    };
    for (slot, unit) in button.szTip.iter_mut().zip(tip.encode_utf16().take(259)) {
        *slot = unit;
    }
    button
}

fn buttons(state: &State, hidden: bool) -> [THUMBBUTTON; 3] {
    let play_pause = if state.paused { state.icons.play } else { state.icons.pause };
    [
        button(PREVIOUS, state.icons.previous, "Previous", hidden),
        button(PLAY_PAUSE, play_pause, "Play/Pause", hidden),
        button(NEXT, state.icons.next, "Next", hidden),
    ]
}

/// Adds the buttons once the window has a taskbar button (`ThumbBarAddButtons`
/// works once per taskbar button), otherwise updates them.
fn sync(state: &mut State, hidden: bool) {
    if state.taskbar.is_none() {
        state.taskbar = unsafe { CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER) }
            .ok()
            .filter(|taskbar| unsafe { taskbar.HrInit() }.is_ok());
    }
    let Some(taskbar) = &state.taskbar else { return };
    let list = buttons(state, hidden);
    let result = if state.added {
        unsafe { taskbar.ThumbBarUpdateButtons(state.hwnd, &list) }
    } else {
        unsafe { taskbar.ThumbBarAddButtons(state.hwnd, &list) }
    };
    match result {
        Ok(()) => state.added = true,
        Err(error) => log::debug!("taskbar-mediacontrol: thumbnail buttons not set yet: {error}"),
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarButtonCreated")) };
    if msg == taskbar_created {
        // A new taskbar button (first show, or Explorer restarted)
        STATE.with_borrow_mut(|state| {
            if let Some(state) = state {
                state.taskbar = None;
                state.added = false;
                sync(state, false);
            }
        });
    } else if msg == WM_COMMAND && ((wparam.0 >> 16) & 0xFFFF) as u32 == THBN_CLICKED {
        let app = STATE.with_borrow(|state| state.as_ref().map(|s| s.app.clone()));
        if let Some(app) = app {
            match (wparam.0 & 0xFFFF) as u32 {
                PREVIOUS => song_controls::previous(&app),
                PLAY_PAUSE => song_controls::play_pause(&app),
                NEXT => song_controls::next(&app),
                _ => {}
            }
        }
        return LRESULT(0);
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

pub struct TaskbarMediaControl;

impl Plugin for TaskbarMediaControl {
    fn id(&self) -> &'static str {
        "taskbar-mediacontrol"
    }

    fn start(&self, app: &AppHandle, _config: &Value) {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let Some(window) = handle.get_webview_window(MAIN_WEBVIEW) else { return };
            let Ok(hwnd) = window.hwnd() else { return };
            let hwnd = HWND(hwnd.0);
            let dark = window.theme().map(|t| t == Theme::Dark).unwrap_or(true);
            let mut state = State {
                app: handle.clone(),
                hwnd,
                taskbar: None,
                icons: load_icons(dark),
                paused: true,
                added: false,
            };
            sync(&mut state, false);
            STATE.with_borrow_mut(|slot| *slot = Some(state));
            if !unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0) }.as_bool() {
                log::warn!("taskbar-mediacontrol: subclassing the window failed");
            }
        });

        // Play/pause icon follows the song (`registerCallback` in the original)
        app.state::<SongInfoProvider>().register(self.id(), |app, song, _| {
            let paused = song.is_paused.unwrap_or(false);
            let _ = app.run_on_main_thread(move || {
                STATE.with_borrow_mut(|state| {
                    if let Some(state) = state {
                        if state.paused != paused {
                            state.paused = paused;
                            sync(state, false);
                        }
                    }
                });
            });
        });
    }

    fn stop(&self, app: &AppHandle) {
        let _ = app.run_on_main_thread(|| {
            STATE.with_borrow_mut(|slot| {
                if let Some(mut state) = slot.take() {
                    // Buttons can't be removed from a taskbar button, only hidden
                    sync(&mut state, true);
                    let _ = unsafe { RemoveWindowSubclass(state.hwnd, Some(subclass_proc), SUBCLASS_ID) };
                }
            });
        });
    }
}
