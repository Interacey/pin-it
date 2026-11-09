//! Captures of real windows: own test window, plus read-only samples of foreign ones when run with `--ignored`.

use super::capture::capture_points;
use super::math::median_color;
use super::plan::{CapturePlan, capture_plan};
use super::scan::sample_title_bar;
use crate::win32::to_hwnd;
use std::time::{Duration, Instant};
use windows::Win32::System::Threading::{GR_GDIOBJECTS, GetCurrentProcess, GetGuiResources};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::IsHungAppWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, GetSystemMetrics, PM_REMOVE, PeekMessageW,
    SM_CXSCREEN, SM_CYSCREEN, SW_SHOWNOACTIVATE, ShowWindow, TranslateMessage, WS_EX_NOACTIVATE,
    WS_OVERLAPPEDWINDOW,
};
use windows::core::w;

fn pump(ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        let mut msg = Default::default();
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn gdi_objects() -> u32 {
    unsafe { GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS) }
}

/// Compares the caption-rows-only capture with a window-sized one and counts GDI objects.
#[test]
fn caption_capture_matches_full_capture_and_does_not_leak() {
    let _guard = crate::window_test_guard();
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let win = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE,
            w!("STATIC"),
            w!("pin-it color test"),
            WS_OVERLAPPEDWINDOW,
            sw - 260,
            sh - 200,
            240,
            140,
            None,
            None,
            None,
            None,
        )
    }
    .unwrap();
    let _ = unsafe { ShowWindow(win, SW_SHOWNOACTIVATE) };
    pump(400);
    let hwnd = win.0 as isize;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let CapturePlan {
            width: w,
            window_size: (_, ht),
            caption_rows: cap_h,
            points: pts,
            ..
        } = capture_plan(win).expect("test window has caption buttons");
        assert!(
            cap_h < ht,
            "capture height {cap_h} is not smaller than the window {ht}"
        );
        let full = capture_points(win, w, ht, &pts).expect("full capture");
        let small = capture_points(win, w, cap_h, &pts).expect("caption capture");
        println!("full {full:?} caption-only {small:?} ({w}x{cap_h} instead of {w}x{ht})");
        assert_eq!(full, small);
        let sample_color = |h| sample_title_bar(h).and_then(|s| s.color);
        assert_eq!(sample_color(hwnd), median_color(&full));

        let before = gdi_objects();
        for _ in 0..200 {
            sample_color(hwnd).unwrap();
        }
        let after = gdi_objects();
        println!("GDI objects before {before}, after 200 samples {after}");
        assert!(
            after < before + 20,
            "GDI objects grew from {before} to {after}"
        );
    }));
    unsafe { DestroyWindow(win).unwrap() };
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

/// Read-only: PrintWindow on up to 2 visible foreign windows, full-size vs caption-only bitmap.
/// Run with `cargo test foreign_windows -- --ignored --nocapture`.
#[test]
#[ignore]
fn foreign_windows_full_vs_caption_capture() {
    let _guard = crate::window_test_guard();
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let mut done = 0;
    for h in crate::windows_filter::enumerate_pinnable() {
        let hw = to_hwnd(h);
        if done == 2 || unsafe { IsHungAppWindow(hw) }.as_bool() {
            continue;
        }
        let Some(CapturePlan {
            width: w,
            window_size: (_, ht),
            caption_rows: cap_h,
            points: pts,
            ..
        }) = capture_plan(hw)
        else {
            continue;
        };
        let (Some(full), Some(small)) = (
            capture_points(hw, w, ht, &pts),
            capture_points(hw, w, cap_h, &pts),
        ) else {
            continue;
        };
        println!(
            "window {h:#x}: {w}x{ht}, caption bitmap {w}x{cap_h}: full {:?} caption-only {:?}",
            median_color(&full),
            median_color(&small)
        );
        assert_eq!(full, small);
        done += 1;
    }
    assert!(done > 0, "no foreign window could be sampled");
}
