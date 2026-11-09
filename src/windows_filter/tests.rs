//! Smoke tests across the submodules and the read-only real-window report.

use super::*;
use crate::win32::{class_name, dwm_rect, to_hwnd};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Dwm::{DWMWA_CAPTION_BUTTON_BOUNDS, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsZoomed};

#[test]
fn invalid_handle_is_not_pinnable_and_enumeration_runs() {
    assert!(!is_pinnable(0));
    let _ = enumerate_pinnable();
    assert!(default_placement(0x7FFF_FFF0).is_none());
    assert_eq!(exe_name(0x7FFF_FFF0), None);
}

/// Read-only: prints exe names (UWP windows resolved) and compares fallback with native slots.
/// Run with `cargo test real_windows_report -- --ignored --nocapture`.
#[test]
#[ignore]
fn real_windows_report() {
    let _ = unsafe {
        windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
    };
    for hwnd in enumerate_pinnable() {
        let w = to_hwnd(hwnd);
        let class = class_name(w);
        let dpi = unsafe { GetDpiForWindow(w) };
        let native = dwm_rect(w, DWMWA_CAPTION_BUTTON_BOUNDS).and_then(|b| {
            let mut wr = RECT::default();
            unsafe { GetWindowRect(w, &mut wr).ok()? };
            compute_slot((wr.left, wr.top), (b.left, b.top, b.right, b.bottom), dpi)
        });
        let f = dwm_rect(w, DWMWA_EXTENDED_FRAME_BOUNDS).unwrap();
        let fallback = compute_fallback_slot(
            (f.left, f.top, f.right, f.bottom),
            dpi,
            unsafe { IsZoomed(w) }.as_bool(),
        );
        println!(
            "{hwnd:#x} {class} exe={:?} elevated={} native={native:?} fallback={fallback:?} final={:?}",
            exe_name(hwnd),
            is_elevated_target(hwnd),
            default_placement(hwnd).map(|p| p.slot)
        );
    }
}
