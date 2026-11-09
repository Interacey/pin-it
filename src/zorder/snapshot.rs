//! Z-order snapshot of all top-level windows.

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
use windows::core::BOOL;

use crate::Hwnd;

/// Lists all top-level windows from the top of the z-order to the bottom.
///
/// Uses `EnumWindows`, which copies the list first, so windows created or destroyed while it
/// runs cannot cut the walk short the way a `GetWindow(GW_HWNDNEXT)` chain can.
pub(crate) fn z_order_snapshot() -> Vec<Hwnd> {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam is the &mut Vec<Hwnd> passed below for the duration of EnumWindows.
        let list = unsafe { &mut *(lparam.0 as *mut Vec<Hwnd>) };
        list.push(hwnd.0 as isize);
        true.into()
    }
    let mut out: Vec<Hwnd> = Vec::new();
    // SAFETY: the callback only uses lparam as the Vec above.
    // A failed walk leaves the windows collected so far, the best answer available.
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut out as *mut _ as isize)) };
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };

    /// A window that is destroyed while the list is read must not cut the snapshot short.
    #[test]
    fn snapshot_is_complete_while_windows_come_and_go() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP,
        };
        use windows::core::w;
        let _guard = crate::window_test_guard();
        let make = || unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                0,
                0,
                10,
                10,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        };
        // Anchor at the bottom of the z-order: a chain walk that breaks early would miss it.
        let anchor = make();
        unsafe {
            let _ = SetWindowPos(
                anchor,
                Some(windows::Win32::UI::WindowsAndMessaging::HWND_BOTTOM),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            );
        }
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let churn = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                let h = make();
                unsafe { DestroyWindow(h).unwrap() };
            }
        });
        let mut missing = 0;
        for _ in 0..3000 {
            if !z_order_snapshot().contains(&(anchor.0 as isize)) {
                missing += 1;
            }
        }
        stop.store(true, Ordering::Relaxed);
        churn.join().unwrap();
        unsafe { DestroyWindow(anchor).unwrap() };
        assert_eq!(missing, 0, "snapshot missed the anchor window");
    }
}
