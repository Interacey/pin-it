//! Tray icon and settings menu.
//!
//! The icon uses `NOTIFYICON_VERSION_4`. With that version the low word of the callback
//! `lparam` is the event (`WM_CONTEXTMENU`, `WM_LBUTTONUP`, ...) and the high word is the icon id,
//! and a right click arrives as `WM_CONTEXTMENU` only. If the shell refuses the version switch,
//! the icon falls back to the legacy protocol where `lparam` is the plain mouse message and the
//! right click arrives as `WM_RBUTTONUP`. `Tray` remembers which one is active, so exactly one
//! of the two opens the menu. Left clicks do nothing.
//!
//! The menu flows in one direction: `menu_model` (data) -> `hmenu` (Win32 handle) -> `command`
//! (what the chosen entry does to the settings). `icon` loads the app icon, `color_picker` wraps the
//! color dialog and `open_guard` keeps the nested message loops from re-entering.

mod color_picker;
mod command;
mod hmenu;
mod icon;
mod model;
mod open_guard;

pub(crate) use command::{Effect, TrayCommand, apply};
pub(crate) use model::Foreground;

use std::cell::Cell;

use crate::Hwnd;
use crate::config::Settings;
use crate::win32::to_hwnd;
use color_picker::pick_color;
use hmenu::{build_hmenu, command_for_id, destroy_menu};
use icon::TrayIcon;
use model::{MenuItem, menu_model};
use open_guard::{MENU_OPEN, OpenGuard, is_taken};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, HMENU, PostMessageW, RegisterWindowMessageW, SetForegroundWindow, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, WM_CONTEXTMENU, WM_NULL, WM_RBUTTONUP,
};
use windows::core::w;

const ICON_ID: u32 = 1;
const TOOLTIP: &str = "Pin It";
const EVENT_MASK: u32 = 0xFFFF;

/// Returns the message id the shell broadcasts after explorer restarts.
pub(crate) fn taskbar_created_message() -> u32 {
    // SAFETY: constant NUL-terminated string.
    unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) }
}

/// Whether the menu (or the color dialog it opened) is on screen; its nested message loop must
/// not be entered again.
pub(crate) fn menu_is_open() -> bool {
    is_taken(&MENU_OPEN)
}

/// The two messages the owner window must accept even when elevated (UIPI): the tray callback and
/// `TaskbarCreated`.
pub(crate) fn callback_message_ids(callback_msg: u32) -> [u32; 2] {
    [callback_msg, taskbar_created_message()]
}

/// The notification icon and its popup menu.
///
/// The object outlives a failed `add_icon` (the shell may not be ready at startup, or explorer
/// restarts): the owner retries until `is_added`.
pub(crate) struct Tray {
    owner: Hwnd,
    callback_msg: u32,
    icon: TrayIcon,
    /// Version 4 of the notification protocol is active; see the module comment.
    v4: Cell<bool>,
    /// The shell holds our icon, as far as the last add or remove told.
    added: Cell<bool>,
}

impl Tray {
    /// Loads the icon image for `owner`, which will receive events as `callback_msg`. The icon is
    /// not in the notification area until `add_icon` succeeds.
    pub(crate) fn new(owner_hwnd: Hwnd, callback_msg: u32) -> windows::core::Result<Tray> {
        Ok(Tray {
            owner: owner_hwnd,
            callback_msg,
            icon: TrayIcon::load()?,
            v4: Cell::new(false),
            added: Cell::new(false),
        })
    }

    /// Whether the icon is in the notification area.
    pub(crate) fn is_added(&self) -> bool {
        self.added.get()
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: to_hwnd(self.owner),
            uID: ICON_ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: self.callback_msg,
            hIcon: self.icon.handle(),
            ..Default::default()
        };
        for (d, s) in nid.szTip.iter_mut().zip(TOOLTIP.encode_utf16()) {
            *d = s;
        }
        nid
    }

    /// Adds the icon (id 1) to the notification area and switches to version 4. Also used after
    /// `TaskbarCreated`, when the shell has forgotten the icon.
    pub(crate) fn add_icon(&self) -> windows::core::Result<()> {
        let mut nid = self.data();
        // SAFETY: nid is fully initialised and lives across both calls.
        let added = unsafe { Shell_NotifyIconW(NIM_ADD, &nid).ok() };
        self.added.set(added.is_ok());
        added?;
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        // SAFETY: as above.
        self.v4
            .set(unsafe { Shell_NotifyIconW(NIM_SETVERSION, &nid) }.as_bool());
        Ok(())
    }

    /// Removes the icon from the notification area; the shell drops stale icons itself, so a
    /// failure is not reported.
    pub(crate) fn remove_icon(&self) {
        let nid = self.data();
        // SAFETY: removes our own icon.
        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
        self.added.set(false);
    }

    /// Handles the callback message; returns the command chosen in the context menu, if any.
    ///
    /// `foreground` is the app window that was active before the menu.
    pub(crate) fn on_message(
        &self,
        lparam: isize,
        settings: &Settings,
        foreground: &Foreground,
    ) -> Option<TrayCommand> {
        if !self.opens_menu(lparam) {
            return None;
        }
        // The menu and the color dialog run nested message loops; a re-entrant callback is dropped.
        let _open = OpenGuard::acquire(&MENU_OPEN)?;
        let model = menu_model(settings, foreground);
        match self.show_menu(&model)? {
            TrayCommand::SetFixedColor(initial) => {
                pick_color(to_hwnd(self.owner), initial).map(TrayCommand::SetFixedColor)
            }
            other => Some(other),
        }
    }

    /// True for the one event that means "right click" in the active protocol.
    fn opens_menu(&self, lparam: isize) -> bool {
        let event = (lparam as u32) & EVENT_MASK;
        let wanted = if self.v4.get() {
            WM_CONTEXTMENU
        } else {
            WM_RBUTTONUP
        };
        event == wanted
    }

    fn show_menu(&self, model: &[MenuItem]) -> Option<TrayCommand> {
        let menu = build_hmenu(model).ok()?;
        let id = self.track_menu(menu);
        destroy_menu(menu);
        command_for_id(model, id)
    }

    /// Shows the menu at the cursor and returns the chosen id (0 if dismissed).
    fn track_menu(&self, menu: HMENU) -> i32 {
        let owner = to_hwnd(self.owner);
        let mut cursor = POINT::default();
        // SAFETY: cursor is a valid out pointer. On failure the menu opens at the origin.
        let _ = unsafe { GetCursorPos(&mut cursor) };
        // SAFETY: the owner window belongs to this process and the menu is valid for the call.
        unsafe {
            let _ = SetForegroundWindow(owner);
            let id = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                cursor.x,
                cursor.y,
                None,
                owner,
                None,
            );
            // Documented companion of TrackPopupMenu so the menu closes when focus moves away.
            let _ = PostMessageW(Some(owner), WM_NULL, Default::default(), Default::default());
            id.0
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.remove_icon();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
    };

    /// A message-only window that shows nothing; destroyed on drop even when an assert fails.
    struct TestWindow(HWND);

    impl TestWindow {
        fn new() -> Self {
            // SAFETY: STATIC is a predefined class; a message-only window shows nothing.
            let hwnd = unsafe {
                let inst = GetModuleHandleW(None).unwrap();
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!("pin-it-test"),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    0,
                    0,
                    Some(HWND_MESSAGE),
                    None,
                    Some(inst.into()),
                    None,
                )
                .unwrap()
            };
            TestWindow(hwnd)
        }

        fn handle(&self) -> Hwnd {
            self.0.0 as isize
        }
    }

    impl Drop for TestWindow {
        fn drop(&mut self) {
            // SAFETY: window was created by this test on this thread.
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }

    #[test]
    fn callback_ids_contain_both_messages() {
        let ids = callback_message_ids(0x8001);
        assert_eq!(ids[0], 0x8001);
        assert_eq!(ids[1], taskbar_created_message());
        assert_ne!(ids[1], 0);
    }

    /// Adds and removes a real icon; Drop removes it even when asserts fail.
    #[test]
    fn tray_add_and_drop() {
        let _guard = crate::window_test_guard();
        let win = TestWindow::new();
        let tray = Tray::new(win.handle(), 0x8001).expect("create icon");
        assert!(!tray.is_added(), "new must not touch the shell");
        tray.add_icon().expect("add icon");
        assert!(tray.is_added());
        // events other than the right click are ignored without showing a menu
        assert_eq!(
            tray.on_message(0x0202, &Settings::default(), &Foreground::default()),
            None
        );
    }

    /// A failed add keeps the object, and a later add succeeds: the retry path of the app.
    #[test]
    fn failed_add_can_be_retried() {
        let _guard = crate::window_test_guard();
        let win = TestWindow::new();
        let tray = Tray::new(win.handle(), 0x8001).expect("create icon");
        tray.add_icon().expect("first add");
        // The shell refuses a second icon with the same id.
        assert!(tray.add_icon().is_err());
        assert!(!tray.is_added());
        tray.remove_icon();
        tray.add_icon().expect("retry after the icon is gone");
        assert!(tray.is_added());
    }

    #[test]
    fn menu_open_flag_follows_the_guard() {
        assert!(!menu_is_open());
        let guard = OpenGuard::acquire(&MENU_OPEN).expect("acquire");
        assert!(menu_is_open());
        drop(guard);
        assert!(!menu_is_open());
    }
}
