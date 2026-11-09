//! The window side the popups share: class registration, creation with rounded corners, the
//! single-instance check, raising, and the "user left" test of deactivation.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::SetActiveWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DROPSHADOW, CreateWindowExW, DestroyWindow, IDC_ARROW, IsWindow, LoadCursorW,
    RegisterClassW, SW_SHOW, SW_SHOWNA, SetForegroundWindow, ShowWindow, WA_INACTIVE, WNDCLASSW,
    WNDPROC, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::PCWSTR;

use crate::win32::to_hwnd;

/// Class and title of a popup and its window procedure.
pub(crate) struct PopupClass {
    pub(crate) name: PCWSTR,
    pub(crate) title: PCWSTR,
    pub(crate) proc: WNDPROC,
}

/// Creates a tool window without a taskbar button at `origin` with `size` pixels and rounded
/// corners. It is not shown yet; see `raise`.
pub(crate) fn create(
    class: &PopupClass,
    origin: (i32, i32),
    size: (u32, u32),
) -> windows::core::Result<HWND> {
    // SAFETY: module handle query and a class with a static procedure.
    let hinst = unsafe { GetModuleHandleW(None)? };
    // SAFETY: stock cursor; fails only without a desktop, then the class has none.
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    // SAFETY: the class data is valid for the call. A second registration fails harmlessly: the
    // class exists.
    let _ = unsafe {
        RegisterClassW(&WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: class.proc,
            hInstance: hinst.into(),
            hCursor: cursor,
            lpszClassName: class.name,
            ..Default::default()
        })
    };
    // SAFETY: class registered above; the procedure only uses the owner's thread-local state.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.name,
            class.title,
            WS_POPUP | WS_CLIPCHILDREN,
            origin.0,
            origin.1,
            size.0 as i32,
            size.1 as i32,
            None,
            None,
            Some(hinst.into()),
            None,
        )?
    };
    round_corners(hwnd);
    Ok(hwnd)
}

fn round_corners(hwnd: HWND) {
    let preference = DWMWCP_ROUND;
    // SAFETY: the attribute value is a DWM_WINDOW_CORNER_PREFERENCE of the size passed.
    let result = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const _,
            size_of_val(&preference) as u32,
        )
    };
    // Windows 10 has no rounded corners; square ones are the intended fallback.
    let _ = result;
}

/// The window of `handle` (0 when none) if it still exists.
pub(crate) fn live(handle: isize) -> Option<HWND> {
    let hwnd = to_hwnd(handle);
    // SAFETY: `IsWindow` accepts any handle value.
    (!hwnd.0.is_null() && unsafe { IsWindow(Some(hwnd)) }.as_bool()).then_some(hwnd)
}

/// Shows the window and takes the focus. This runs in the process that just handled a click on
/// the tray menu, which is allowed to. The system may still refuse (the user switched at that
/// moment); then a click on the window gives the focus. Tests only show it, to stay out of the
/// way of whatever the user is doing.
pub(crate) fn raise(hwnd: HWND) {
    // SAFETY: our own window.
    unsafe {
        if cfg!(test) {
            let _ = ShowWindow(hwnd, SW_SHOWNA);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
            // A refusal is the documented case above; nothing to report.
            let _ = SetForegroundWindow(hwnd);
            let _ = SetActiveWindow(hwnd);
        }
    }
}

pub(crate) fn invalidate(hwnd: HWND) {
    // SAFETY: our own window; null rect means the whole client area.
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

pub(crate) fn destroy(hwnd: HWND) {
    // SAFETY: our own window. A failure means it is gone already.
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

/// Destroys the window of `handle` if it is open.
pub(crate) fn close(handle: isize) -> windows::core::Result<()> {
    match live(handle) {
        // SAFETY: our own window on its own thread.
        Some(hwnd) => unsafe { DestroyWindow(hwnd) },
        None => Ok(()),
    }
}

/// Client coordinates of a mouse message (signed 16 bit words).
pub(crate) fn point_of(lparam: LPARAM) -> (i32, i32) {
    let x = (lparam.0 & 0xFFFF) as u16 as i16;
    let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16;
    (i32::from(x), i32::from(y))
}

/// How long after opening a deactivation is not the user leaving. Right after the tray menu
/// closes, Windows hands the foreground back to the window that had it before; that reaches a
/// popup that was just shown within a few milliseconds.
const OPENING_GRACE: Duration = Duration::from_millis(500);

/// Tells from `WM_ACTIVATE` when the user left the window. A deactivation before the first
/// activation (the system refused the focus at opening) or during the opening grace does not
/// count.
#[derive(Debug)]
pub(crate) struct Activation {
    opened: Instant,
    was_active: bool,
}

impl Activation {
    pub(crate) fn new() -> Self {
        Self {
            opened: Instant::now(),
            was_active: false,
        }
    }

    /// True when `wparam` of `WM_ACTIVATE` says the window lost its activation after having it.
    pub(crate) fn user_left(&mut self, wparam: WPARAM) -> bool {
        self.user_left_at(wparam, Instant::now())
    }

    fn user_left_at(&mut self, wparam: WPARAM, now: Instant) -> bool {
        let inactive = (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE;
        let in_grace = now.duration_since(self.opened) < OPENING_GRACE;
        let left = inactive && self.was_active && !in_grace;
        // A deactivation in the grace period leaves the window unfocused: it needs a real
        // activation (a click) before leaving it can close the window.
        self.was_active = !inactive;
        left
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INACTIVE: WPARAM = WPARAM(WA_INACTIVE as usize);
    const ACTIVE: WPARAM = WPARAM(1);

    #[test]
    fn leaving_counts_only_after_the_window_was_active() {
        let mut activation = Activation::new();
        let later = Instant::now() + OPENING_GRACE;
        assert!(
            !activation.user_left_at(INACTIVE, later),
            "refused focus at opening"
        );
        assert!(!activation.user_left_at(ACTIVE, later));
        assert!(activation.user_left_at(INACTIVE, later));
    }

    #[test]
    fn a_deactivation_in_the_opening_grace_is_not_leaving() {
        let mut activation = Activation::new();
        let opened = activation.opened;
        assert!(!activation.user_left_at(ACTIVE, opened));
        assert!(
            !activation.user_left_at(INACTIVE, opened + OPENING_GRACE / 2),
            "the focus handed back after the menu closed"
        );
        // Unfocused now: leaving needs a real activation first.
        assert!(!activation.user_left_at(INACTIVE, opened + OPENING_GRACE * 2));
        assert!(!activation.user_left_at(ACTIVE, opened + OPENING_GRACE * 2));
        assert!(activation.user_left_at(INACTIVE, opened + OPENING_GRACE * 3));
    }

    #[test]
    fn mouse_coordinates_are_signed_words() {
        assert_eq!(point_of(LPARAM(0x0014_000A)), (10, 20));
        assert_eq!(point_of(LPARAM(0xFFFF_FFF6u32 as isize)), (-10, -1));
    }

    #[test]
    fn a_zero_handle_is_no_window() {
        assert!(live(0).is_none());
        assert!(close(0).is_ok());
    }
}
