//! Helpers shared by the tests that create real windows.

use std::cell::RefCell;
use std::sync::Once;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetSystemMetrics, MSG,
    PM_REMOVE, PeekMessageW, PostMessageW, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOWNA,
    ShowWindow, TranslateMessage, WM_USER, WNDCLASSW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use super::PinButton;
use crate::Hwnd;
use crate::win32::to_hwnd;

thread_local! {
    pub(super) static NOTIFIED: RefCell<Vec<(usize, u32)>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "system" fn notify_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if m == MSG_PIN {
        NOTIFIED.with(|n| n.borrow_mut().push((w.0, m)));
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(h, m, w, l) }
}

pub(super) fn make_window(class: PCWSTR, ex: u32, x: i32, y: i32, w: i32, h: i32) -> HWND {
    use windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let hinst = GetModuleHandleW(None).unwrap();
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(notify_proc),
            hInstance: hinst.into(),
            lpszClassName: w!("PinItTestWin"),
            ..Default::default()
        });
    });
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(ex),
            class,
            w!("pin-it-test"),
            WS_POPUP,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(GetModuleHandleW(None).unwrap().into()),
            None,
        )
        .unwrap()
    }
}

/// Small test window at the far bottom-right of the primary screen, shown without activation.
pub(super) fn corner_window(dx: i32) -> HWND {
    let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let h = make_window(w!("PinItTestWin"), 0, sw - 160 - dx, sh - 100, 120, 60);
    unsafe {
        let _ = ShowWindow(h, SW_SHOWNA);
    }
    h
}

pub(super) fn pump() {
    let mut msg = MSG::default();
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

pub(super) fn post(h: Hwnd, m: u32, x: i32, y: i32) {
    let lp = ((y as u32 & 0xffff) << 16) | (x as u32 & 0xffff);
    unsafe {
        PostMessageW(Some(to_hwnd(h)), m, WPARAM(0), LPARAM(lp as isize)).unwrap();
    }
    pump();
}

/// Z-order snapshot reduced to this process's windows, so foreign windows cannot disturb comparisons.
pub(super) fn own_snapshot() -> Vec<isize> {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let pid = std::process::id();
    crate::zorder::z_order_snapshot()
        .into_iter()
        .filter(|&h| {
            let mut p = 0u32;
            unsafe { GetWindowThreadProcessId(to_hwnd(h), Some(&mut p)) };
            p == pid
        })
        .collect()
}

pub(super) fn z_index(h: HWND) -> usize {
    own_snapshot()
        .iter()
        .position(|&x| x == h.0 as isize)
        .expect("window not in z-order")
}

pub(super) const MSG_PIN: u32 = WM_USER + 7;

pub(super) fn setup() -> (HWND, HWND, PinButton) {
    let target = corner_window(0);
    let notify = make_window(w!("PinItTestWin"), 0, 0, 0, 1, 1);
    let mut b = PinButton::new(target.0 as isize, notify.0 as isize, MSG_PIN).unwrap();
    let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    b.set_slot(sw - 60, sh - 90, 32);
    b.show();
    NOTIFIED.with(|n| n.borrow_mut().clear());
    (target, notify, b)
}

pub(super) fn teardown(target: HWND, notify: HWND, b: PinButton) {
    drop(b);
    unsafe {
        let _ = DestroyWindow(target);
        let _ = DestroyWindow(notify);
    }
}
