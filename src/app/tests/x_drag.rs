//! An unpinned window dragged or raised over a pinned one must never end up in front of it, and
//! a foreign topmost window above a pin is corrected. Own test windows only; the system events are
//! injected as `WinEvent`s, the moves are real `SetWindowPos` calls without activation.

use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_TOP, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
};
use windows::core::BOOL;

use super::support::{
    Wins, button_visible, click, eventually_swept, log_text, options, overlay_directly_above, pos,
    pump_ms, temp_dir,
};
use crate::Hwnd;
use crate::app::Runtime;
use crate::config::Settings;
use crate::events::WinEvent;
use crate::win32::to_hwnd;
use crate::zorder::is_topmost;

/// Horizontal distance between two samples of the emulated drag.
const DRAG_STEP: i32 = 60;
/// Time the product gets after each event of the drag.
const DRAG_PUMP_MS: u64 = 8;

fn monitor_rects() -> Vec<RECT> {
    unsafe extern "system" fn collect(_: HMONITOR, _: HDC, rect: *mut RECT, list: LPARAM) -> BOOL {
        // SAFETY: `list` is the Vec below and `rect` is valid for the call.
        unsafe { (*(list.0 as *mut Vec<RECT>)).push(*rect) };
        true.into()
    }
    let mut rects: Vec<RECT> = Vec::new();
    // SAFETY: the callback only uses `list` as the Vec above.
    let _ = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut rects as *mut _ as isize),
        )
    };
    rects
}

fn move_to(h: Hwnd, x: i32, y: i32) {
    // SAFETY: own test window.
    unsafe {
        SetWindowPos(
            to_hwnd(h),
            None,
            x,
            y,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
        )
        .unwrap();
    }
}

fn set_z(h: Hwnd, after: windows::Win32::Foundation::HWND) {
    // SAFETY: own test window.
    unsafe {
        SetWindowPos(
            to_hwnd(h),
            Some(after),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
        .unwrap();
    }
}

/// Pins the first of two windows through a click and returns the runtime.
fn runtime_with_first_pinned(wins: &Wins, tag: &str) -> Runtime {
    let dir = temp_dir(tag);
    let rt = Runtime::start(&options(&dir, wins.0.clone(), &Settings::default())).unwrap();
    eventually_swept(&rt, "two buttons", || rt.button_count() == 2);
    click(rt.button_hwnd(wins.0[0]).unwrap());
    eventually_swept(&rt, "first window topmost", || is_topmost(wins.0[0]));
    rt
}

#[test]
fn dragging_an_unpinned_window_across_all_monitors_never_gets_it_in_front_of_the_pin() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let (pinned, dragged) = (wins.0[0], wins.0[1]);
    let rt = runtime_with_first_pinned(&wins, "x-drag");
    let (pinned_ov, dragged_ov) = (
        rt.button_hwnd(pinned).unwrap(),
        rt.button_hwnd(dragged).unwrap(),
    );
    let monitors = monitor_rects();
    let left = monitors.iter().map(|r| r.left).min().unwrap();
    let right = monitors.iter().map(|r| r.right).max().unwrap();
    let y = monitors[0].bottom - 330;

    set_z(dragged, HWND_TOP);
    rt.inject_event(WinEvent::Foreground(dragged));
    let mut x = left;
    while x < right {
        move_to(dragged, x, y);
        rt.inject_event(WinEvent::Moved(dragged));
        pump_ms(DRAG_PUMP_MS);
        let at = format!("x={x}, monitors {}", monitors.len());
        assert!(!is_topmost(dragged), "dragged window topmost at {at}");
        assert!(
            pos(pinned) < pos(dragged),
            "dragged window in front of the pin at {at}"
        );
        assert!(!is_topmost(dragged_ov), "its overlay topmost at {at}");
        assert!(
            pos(pinned) < pos(dragged_ov),
            "its overlay in front of the pin at {at}"
        );
        x += DRAG_STEP;
    }
    assert!(is_topmost(pinned) && is_topmost(pinned_ov));
    assert!(overlay_directly_above(dragged_ov, dragged));
}

#[test]
fn a_topmost_window_that_lands_above_a_pin_is_corrected_on_the_next_move_event() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let (pinned, foreign) = (wins.0[0], wins.0[1]);
    let dir = temp_dir("x-intruder");
    let rt = Runtime::start(&options(&dir, wins.0.clone(), &Settings::default())).unwrap();
    eventually_swept(&rt, "two buttons", || rt.button_count() == 2);
    click(rt.button_hwnd(pinned).unwrap());
    eventually_swept(&rt, "pin topmost", || is_topmost(pinned));

    // An app's own always-on-top window shows up above the pin.
    set_z(foreign, HWND_TOPMOST);
    assert!(
        is_topmost(foreign) && pos(foreign) < pos(pinned),
        "setup: foreign window above the pin"
    );
    rt.inject_event(WinEvent::Moved(foreign));
    pump_ms(50);
    assert!(
        pos(pinned) < pos(foreign),
        "pin back above the foreign topmost window"
    );
    assert!(is_topmost(pinned));
    let log = log_text(&dir);
    assert!(log.contains("z-order anomaly: window"), "log: {log}");
    assert!(log.contains("re-asserting pins"), "log: {log}");
    assert!(overlay_directly_above(
        rt.button_hwnd(pinned).unwrap(),
        pinned
    ));
}

#[test]
fn cloak_events_keep_buttons_and_pin_in_place() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let rt = runtime_with_first_pinned(&wins, "x-cloak");
    for &h in &wins.0 {
        rt.inject_event(WinEvent::Cloaked(h));
        rt.inject_event(WinEvent::Uncloaked(h));
    }
    pump_ms(150);
    assert!(is_topmost(wins.0[0]) && !is_topmost(wins.0[1]));
    for &h in &wins.0 {
        assert!(button_visible(&rt, h));
        assert!(overlay_directly_above(rt.button_hwnd(h).unwrap(), h));
    }
}
