//! Small Win32 helpers shared by the whole crate: handle conversion, string encoding and window queries.

use std::ffi::c_void;
use std::mem::size_of;

use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWINDOWATTRIBUTE, DwmGetWindowAttribute};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetWindowThreadProcessId, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_NULL,
};

use crate::Hwnd;

mod dib;
pub(crate) use dib::Dib;

/// Dpi of 100 percent scaling; also used when a window reports no dpi (0).
pub(crate) const BASE_DPI: u32 = 96;
const CLASS_NAME_CAPACITY: usize = 256;

pub(crate) fn to_hwnd(hwnd: Hwnd) -> HWND {
    HWND(hwnd as *mut c_void)
}

/// UTF-16 with a terminating NUL, for `PCWSTR` arguments.
pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; CLASS_NAME_CAPACITY];
    // SAFETY: buf is a valid slice.
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

pub(crate) fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    // SAFETY: cloaked is a valid out buffer of the size we pass.
    let queried = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            size_of::<u32>() as u32,
        )
    };
    queried.is_ok() && cloaked != 0
}

pub(crate) fn process_id(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    // SAFETY: pid is a valid out pointer.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// A DWM rectangle attribute of the window; the rect may be empty.
pub(crate) fn dwm_rect(w: HWND, attribute: DWMWINDOWATTRIBUTE) -> Option<RECT> {
    let mut r = RECT::default();
    // SAFETY: r is a valid out buffer of the size we pass.
    unsafe {
        DwmGetWindowAttribute(
            w,
            attribute,
            &mut r as *mut RECT as *mut c_void,
            size_of::<RECT>() as u32,
        )
        .ok()?;
    }
    Some(r)
}

/// Dpi of the window, `BASE_DPI` when Windows reports none.
pub(crate) fn window_dpi(w: HWND) -> u32 {
    // SAFETY: plain query.
    match unsafe { GetDpiForWindow(w) } {
        0 => BASE_DPI,
        dpi => dpi,
    }
}

/// Windows COLORREF value (0x00BBGGRR) of an RGB color.
pub(crate) fn colorref([r, g, b]: [u8; 3]) -> u32 {
    u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16
}

/// True when the window's thread answers a `WM_NULL` within `timeout_ms`; false when it times out or is hung.
/// Used before calls that block on the target (`PrintWindow`), so one stuck app cannot stall a worker.
pub(crate) fn is_responsive(hwnd: Hwnd, timeout_ms: u32) -> bool {
    let mut result = 0usize;
    // SAFETY: `result` is a valid out pointer; SMTO_ABORTIFHUNG returns at once for a hung window.
    let answered = unsafe {
        SendMessageTimeoutW(
            to_hwnd(hwnd),
            WM_NULL,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            timeout_ms,
            Some(&mut result),
        )
    };
    answered.0 != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_is_bgr_ordered() {
        assert_eq!(colorref([1, 2, 3]), 0x0003_0201);
        assert_eq!(colorref([255; 3]), 0x00FF_FFFF);
        assert_eq!(colorref([0; 3]), 0);
    }

    #[test]
    fn a_dead_handle_is_not_responsive() {
        assert!(!is_responsive(0, 50));
    }
}
