//! Win32 glue: the hidden owner window, its window procedure and the re-entrancy guard.
//!
//! `with_app` is the only place that creates `&mut App`; `BUSY` keeps a second one from existing
//! while an `App` method runs. Messages that arrive in that time are posted again.
//!
//! Nothing here reads through the raw `App` pointer except `with_app`: values the window
//! procedure needs outside of it (the `TaskbarCreated` id) have their own thread-locals.

use std::cell::Cell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, ChangeWindowMessageFilterEx, CreateWindowExW,
    DefWindowProcW, DestroyWindow, MSGFLT_ALLOW, PostMessageW, PostQuitMessage, RegisterClassW,
    WM_APP, WM_CLOSE, WM_DESTROY, WM_ENDSESSION, WM_HOTKEY, WM_QUERYENDSESSION, WM_TIMER,
    WNDCLASSW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use super::logging::warn;
use super::open_settings::shell_open;
use super::state::App;
use super::system_change::{WM_DWM_COLORIZATION_CHANGED, is_color_change};
use crate::Hwnd;
use crate::color::WM_COLOR_READY;
use crate::tray;

/// Hook events are waiting in the queue of `EventHooks`.
pub(super) const WM_HOOK_EVENTS: u32 = WM_APP + 1;
/// Tray icon callback.
pub(super) const WM_TRAY: u32 = WM_APP + 2;
/// A pin button was clicked; wparam is the target window.
pub(super) const WM_PIN_CLICK: u32 = WM_APP + 3;
/// Run the housekeeping sweep now (the sweep timer sends the same work).
pub(super) const WM_SWEEP_NOW: u32 = WM_APP + 4;
/// Open the settings file with the default handler; handled outside of `with_app`.
/// (`WM_APP + 5` is `WM_COLOR_READY`.)
pub(super) const WM_OPEN_SETTINGS: u32 = WM_APP + 6;
/// The button size dial chose a size; wparam is the percent (clamped by the command, since any
/// process may post this).
pub(super) const WM_DIAL_CHANGED: u32 = WM_APP + 7;
/// The excluded apps window queued an action (`exclusions::drain_actions`); no parameters, so a
/// foreign process can only make the app look at an empty queue.
pub(super) const WM_EXCLUSIONS_CHANGED: u32 = WM_APP + 8;

pub(super) const SWEEP_TIMER: usize = 1;
pub(super) const SWEEP_INTERVAL_MS: u32 = 2000;
pub(super) const HOTKEY_ID: i32 = 1;
const MAIN_CLASS: PCWSTR = w!("PinItMain");
const WINDOW_TITLE: PCWSTR = w!("Pin It");
const QUERY_END_SESSION_ACCEPT: isize = 1;

thread_local! {
    static CURRENT: Cell<*mut App> = const { Cell::new(std::ptr::null_mut()) };
    /// An `App` method is running; see `with_app`.
    static BUSY: Cell<bool> = const { Cell::new(false) };
    /// The `TaskbarCreated` message id of the installed app; 0 when none is installed.
    static TASKBAR_MSG: Cell<u32> = const { Cell::new(0) };
    /// WM_QUERYENDSESSION was seen, so a following WM_ENDSESSION is genuine.
    static QUERY_SEEN: Cell<bool> = const { Cell::new(false) };
}

/// A window message as the window procedure received it.
#[derive(Clone, Copy)]
struct Message {
    hwnd: HWND,
    id: u32,
    wparam: WPARAM,
    lparam: LPARAM,
}

impl Message {
    fn repost(self) {
        // SAFETY: posts to our own window.
        let posted = unsafe { PostMessageW(Some(self.hwnd), self.id, self.wparam, self.lparam) };
        if let Err(e) = posted {
            warn(&format!(
                "message {:#x} could not be posted again: {e}",
                self.id
            ));
        }
    }
}

/// Makes `app` the one `with_app` reaches. The pointer must stay valid until `clear_app`.
/// `taskbar_msg` is the registered `TaskbarCreated` id.
pub(super) fn install_app(app: *mut App, taskbar_msg: u32) {
    CURRENT.with(|c| c.set(app));
    TASKBAR_MSG.with(|m| m.set(taskbar_msg));
}

pub(super) fn clear_app() {
    CURRENT.with(|c| c.set(std::ptr::null_mut()));
    TASKBAR_MSG.with(|m| m.set(0));
}

pub(super) fn reset_session_flags() {
    QUERY_SEEN.with(|q| q.set(false));
}

pub(super) fn is_busy() -> bool {
    BUSY.with(Cell::get)
}

/// Runs `f` on the app unless an `App` method is already running or no app exists.
pub(super) fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    let p = CURRENT.with(Cell::get);
    if p.is_null() || is_busy() {
        return None;
    }
    BUSY.with(|b| b.set(true));
    // SAFETY: the pointer is set by `Runtime::start` to a live Box and cleared before it is freed;
    // everything runs on this one thread and BUSY guarantees this is the only live `&mut App`.
    let result = f(unsafe { &mut *p });
    BUSY.with(|b| b.set(false));
    Some(result)
}

/// Like `with_app` for a window message: if an `App` method is running, the message is posted
/// again and handled later; after `shutdown` it is dropped.
fn dispatch(msg: Message, f: impl FnOnce(&mut App)) {
    if is_busy() {
        msg.repost();
        return;
    }
    dispatch_to_app(f);
}

/// Tray icon message. Holds no `App` reference while the menu (a nested message loop) is open;
/// `tray::menu_is_open` keeps a second menu from starting meanwhile.
fn on_tray(lparam: isize) {
    if tray::menu_is_open() {
        return;
    }
    let Some((tray, settings, foreground)) = with_app(|app| {
        if app.down {
            return None;
        }
        let (settings, foreground) = app.menu_inputs();
        Some((app.tray.clone()?, settings, foreground))
    })
    .flatten() else {
        return;
    };
    let Some(cmd) = tray.on_message(lparam, &settings, &foreground) else {
        return;
    };
    // The menu made this process the foreground one; handing that right on lets the editor that
    // "Open settings file" starts, and the popups, come to the front instead of flashing behind.
    allow_foreground_handoff();
    let ran = with_app(|app| {
        if !app.down {
            app.run_tray_command(&cmd);
        }
    });
    if ran.is_none() {
        warn(&format!("tray command {cmd:?} dropped: the app was busy"));
    }
}

fn allow_foreground_handoff() {
    // SAFETY: plain call; it only grants a right, to any process, until the next input.
    if let Err(e) = unsafe { AllowSetForegroundWindow(ASFW_ANY) } {
        warn(&format!("foreground right could not be handed on: {e}"));
    }
}

/// Opens the settings file. The `App` is only asked for the path; the shell call runs after
/// that borrow has ended (see `open_settings`).
fn on_open_settings(msg: Message) {
    if is_busy() {
        msg.repost();
        return;
    }
    if let Some(path) = with_app(|app| app.settings_file.path.clone()) {
        shell_open(&path);
    }
}

/// `with_app` that skips an app which is shutting down.
fn dispatch_to_app(f: impl FnOnce(&mut App)) {
    with_app(|app| {
        if !app.down {
            f(app);
        }
    });
}

fn on_end_session(msg: Message) {
    if msg.wparam.0 == 0 {
        QUERY_SEEN.with(|q| q.set(false)); // the session end was cancelled
    } else if QUERY_SEEN.with(Cell::get) {
        dispatch(msg, |app| {
            app.shutdown();
            post_quit();
        });
    }
}

pub(super) fn post_quit() {
    // SAFETY: posts WM_QUIT to this thread's queue.
    unsafe { PostQuitMessage(0) };
}

/// The `TaskbarCreated` message id the app registered, once an app is installed.
fn registered_taskbar_message() -> Option<u32> {
    Some(TASKBAR_MSG.with(Cell::get)).filter(|&id| id != 0)
}

fn on_other_message(msg: Message) -> LRESULT {
    if registered_taskbar_message() == Some(msg.id) {
        dispatch(msg, App::on_taskbar_created);
        return LRESULT(0);
    }
    if is_color_change(msg.id, msg.lparam) {
        // The string of a WM_SETTINGCHANGE is gone once this call returns, so a repost carries none.
        let again = Message {
            id: WM_DWM_COLORIZATION_CHANGED,
            wparam: WPARAM(0),
            lparam: LPARAM(0),
            ..msg
        };
        dispatch(again, App::on_system_colors_changed);
        return LRESULT(0);
    }
    // SAFETY: default handling.
    unsafe { DefWindowProcW(msg.hwnd, msg.id, msg.wparam, msg.lparam) }
}

/// The percent a dial message carries; a value beyond `u32` (only a foreign process can send one)
/// becomes the largest, which the command then clamps.
fn percent_of(wparam: WPARAM) -> u32 {
    u32::try_from(wparam.0).unwrap_or(u32::MAX)
}

fn handled(f: impl FnOnce()) -> LRESULT {
    f();
    LRESULT(0)
}

unsafe extern "system" fn main_proc(
    hwnd: HWND,
    id: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let msg = Message {
        hwnd,
        id,
        wparam,
        lparam,
    };
    match id {
        WM_HOOK_EVENTS => handled(|| dispatch(msg, App::handle_hook_events)),
        WM_COLOR_READY => handled(|| dispatch(msg, App::on_color_ready)),
        WM_TRAY if is_busy() => handled(|| msg.repost()),
        WM_TRAY => handled(|| on_tray(lparam.0)),
        // Only windows that have a button can be clicked; other wparam values are spoofed.
        WM_PIN_CLICK => handled(|| dispatch(msg, |app| app.on_pin_click(wparam.0 as Hwnd))),
        WM_HOTKEY if wparam.0 as i32 == HOTKEY_ID => handled(|| dispatch(msg, App::on_hotkey)),
        WM_HOTKEY => LRESULT(0),
        WM_TIMER if wparam.0 == SWEEP_TIMER => handled(|| dispatch_to_app(App::sweep)),
        WM_SWEEP_NOW => handled(|| dispatch(msg, App::sweep)),
        WM_OPEN_SETTINGS => handled(|| on_open_settings(msg)),
        WM_DIAL_CHANGED => handled(|| dispatch(msg, |app| app.on_dial_changed(percent_of(wparam)))),
        WM_EXCLUSIONS_CHANGED => handled(|| dispatch(msg, App::on_exclusions_changed)),
        WM_QUERYENDSESSION => {
            QUERY_SEEN.with(|q| q.set(true));
            LRESULT(QUERY_END_SESSION_ACCEPT)
        }
        WM_ENDSESSION => handled(|| on_end_session(msg)),
        WM_CLOSE => handled(|| destroy_window(hwnd)),
        WM_DESTROY => handled(post_quit),
        _ => on_other_message(msg),
    }
}

fn destroy_window(hwnd: HWND) {
    // SAFETY: destroys our own window; WM_DESTROY posts the quit message.
    if let Err(e) = unsafe { DestroyWindow(hwnd) } {
        warn(&format!("cannot destroy the main window: {e}"));
    }
}

/// Creates the hidden top-level popup that owns tray, hooks, hotkey and buttons.
pub(super) fn create_main_window() -> windows::core::Result<HWND> {
    // SAFETY: registers a class with a static procedure and creates a never-shown popup.
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        // Registering twice (second runtime in one process) fails harmlessly: the class exists.
        let _ = RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(main_proc),
            hInstance: hinst.into(),
            lpszClassName: MAIN_CLASS,
            ..Default::default()
        });
        CreateWindowExW(
            Default::default(),
            MAIN_CLASS,
            WINDOW_TITLE,
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst.into()),
            None,
        )
    }
}

/// Lets the tray callback and `TaskbarCreated` through the UIPI filter, so they still arrive when
/// Pin It runs elevated.
pub(super) fn allow_tray_messages(hwnd: HWND) {
    for id in tray::callback_message_ids(WM_TRAY) {
        // SAFETY: our own window; no filter struct is requested.
        if let Err(e) = unsafe { ChangeWindowMessageFilterEx(hwnd, id, MSGFLT_ALLOW, None) } {
            warn(&format!("message filter for {id:#x} failed: {e}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_message_ids_are_distinct() {
        let mut ids = [
            WM_HOOK_EVENTS,
            WM_TRAY,
            WM_PIN_CLICK,
            WM_SWEEP_NOW,
            WM_OPEN_SETTINGS,
            WM_DIAL_CHANGED,
            WM_EXCLUSIONS_CHANGED,
            WM_COLOR_READY,
        ];
        ids.sort_unstable();
        assert!(ids.windows(2).all(|pair| pair[0] != pair[1]), "{ids:?}");
        let expected: Vec<u32> = (1..=8).map(|n| WM_APP + n).collect();
        assert_eq!(
            ids.to_vec(),
            expected,
            "all of WM_APP + 1..=8 are in use, none twice"
        );
    }
}
