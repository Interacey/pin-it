//! The pin button must not vanish around activation and resizing, and every change of its
//! visibility must leave a log line with the reason. Own test windows only; the system events
//! that need real input are injected as `WinEvent`s.

use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, HWND_TOP, SW_HIDE, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos, ShowWindow,
};

use super::support::{
    Wins, button_visible, log_text, options, overlay_directly_above, pump_ms, sweep, temp_dir,
};
use crate::Hwnd;
use crate::app::Runtime;
use crate::config::Settings;
use crate::events::WinEvent;
use crate::win32::to_hwnd;

/// Raises `h` like the activation of a clicked window does, without activating it.
fn raise(h: Hwnd) {
    // SAFETY: own test window.
    unsafe {
        SetWindowPos(
            to_hwnd(h),
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
        .unwrap();
    }
}

#[test]
fn overlay_returns_above_its_target_when_the_raise_comes_after_the_foreground_event() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    let dir = temp_dir("x-raise");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(100);
    let overlay = rt.button_hwnd(w[0]).unwrap();
    assert!(overlay_directly_above(overlay, w[0]), "start state");

    // The event first, the raise right after it: the case a late system z-order change produces.
    rt.inject_event(WinEvent::Foreground(w[0]));
    raise(w[0]);
    assert!(
        !overlay_directly_above(overlay, w[0]),
        "raise buries the overlay"
    );
    pump_ms(400);
    assert!(
        overlay_directly_above(overlay, w[0]),
        "overlay not restacked without a sweep"
    );
    assert!(button_visible(&rt, w[0]));
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_resize_by_a_few_pixels_does_not_hide_the_button() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let w = wins.0.clone();
    let dir = temp_dir("x-resize");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(100);
    assert!(button_visible(&rt, w[0]));
    let mut rect = windows::Win32::Foundation::RECT::default();
    // SAFETY: own test window.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(to_hwnd(w[0]), &mut rect) }
        .unwrap();
    for grow in [1, 3, 0] {
        // SAFETY: own test window.
        unsafe {
            SetWindowPos(
                to_hwnd(w[0]),
                None,
                rect.left,
                rect.top - grow,
                rect.right - rect.left,
                rect.bottom - rect.top + grow,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
            .unwrap();
        }
        rt.inject_event(WinEvent::Moved(w[0]));
        assert!(
            button_visible(&rt, w[0]),
            "hidden after growing by {grow} px"
        );
    }
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn visibility_changes_are_logged_once_with_the_reason() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    let dir = temp_dir("x-log");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(100);
    let shown = format!("({:#x})", w[0]);
    let lines = |needle: &str| {
        log_text(&dir)
            .lines()
            .filter(|l| l.contains(needle) && l.contains(&shown))
            .count()
    };
    assert_eq!(lines("button shown at"), 1, "{}", log_text(&dir));

    // SAFETY: own test windows.
    let _ = unsafe { ShowWindow(to_hwnd(w[0]), SW_SHOWMINNOACTIVE) };
    rt.inject_event(WinEvent::Minimized(w[0]));
    rt.inject_event(WinEvent::Minimized(w[0]));
    assert_eq!(lines("button hidden: not eligible (minimized)"), 1);

    let _ = unsafe { ShowWindow(to_hwnd(w[0]), SW_SHOWNOACTIVATE) };
    rt.inject_event(WinEvent::Restored(w[0]));
    assert_eq!(lines("button shown at"), 2, "{}", log_text(&dir));

    let _ = unsafe { ShowWindow(to_hwnd(w[0]), SW_HIDE) };
    sweep(&rt);
    assert!(
        log_text(&dir).contains("hidden: not eligible"),
        "{}",
        log_text(&dir)
    );

    let doomed = format!("({:#x})", w[1]);
    unsafe { DestroyWindow(to_hwnd(w[1])).unwrap() };
    rt.inject_event(WinEvent::Destroyed(w[1]));
    let log = log_text(&dir);
    assert!(
        log.lines()
            .any(|l| l.contains("button hidden: window destroyed") && l.contains(&doomed)),
        "{log}"
    );
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}
