//! The pin follows Windows' window animations by fading: invisible while a restore, maximize or minimize
//! animation runs, back afterwards. Own windows only.

use windows::Win32::UI::WindowsAndMessaging::{
    IsWindowVisible, SW_MINIMIZE, SW_SHOWNOACTIVATE, ShowWindow,
};

use super::pb_common::{NativeWin, shown_slot};
use super::support::{options, pump_ms, temp_dir};
use crate::Hwnd;
use crate::app::Runtime;
use crate::app::host::with_app;
use crate::config::Settings;
use crate::events::WinEvent;
use crate::win32::to_hwnd;

/// Longer than the hold and the fade-in together.
const ANIMATION_WAIT_MS: u64 = 600;

fn alpha(h: Hwnd) -> u8 {
    with_app(|a| a.entries.get(&h).map(|e| e.button.alpha()))
        .flatten()
        .expect("entry")
}

fn overlay_visible(h: Hwnd) -> bool {
    let overlay = with_app(|a| a.entries.get(&h).map(|e| e.button.hwnd()))
        .flatten()
        .expect("entry");
    // SAFETY: plain query.
    unsafe { IsWindowVisible(to_hwnd(overlay)) }.as_bool()
}

fn run_with(animate: bool, body: impl FnOnce(&Runtime, &NativeWin)) {
    let _g = crate::window_test_guard();
    let w = NativeWin::new(0, None);
    let dir = temp_dir("x3-fade");
    let settings = Settings {
        animate,
        ..Settings::default()
    };
    let rt = Runtime::start(&options(&dir, vec![w.0], &settings)).unwrap();
    while shown_slot(w.0).is_none() {
        pump_ms(25);
    }
    body(&rt, &w);
    drop(rt);
    drop(w);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_maximize_hides_the_pin_at_once_and_it_fades_back_at_the_final_slot() {
    run_with(true, |rt, w| {
        assert_eq!(alpha(w.0), 255);
        w.maximize();
        rt.inject_event(WinEvent::Moved(w.0));
        assert_eq!(
            alpha(w.0),
            0,
            "the same event cycle that re-anchors the pin"
        );
        assert!(shown_slot(w.0).is_some(), "re-anchored while invisible");
        pump_ms(ANIMATION_WAIT_MS);
        assert_eq!(alpha(w.0), 255);
    });
}

#[test]
fn without_animation_a_maximize_keeps_the_pin_opaque() {
    run_with(false, |rt, w| {
        w.maximize();
        rt.inject_event(WinEvent::Moved(w.0));
        assert_eq!(alpha(w.0), 255);
        w.restore();
        rt.inject_event(WinEvent::Moved(w.0));
        assert_eq!(alpha(w.0), 255);
    });
}

#[test]
fn a_minimize_fades_the_pin_out_and_hides_it() {
    run_with(true, |rt, w| {
        // SAFETY: own window.
        let _ = unsafe { ShowWindow(to_hwnd(w.0), SW_MINIMIZE) };
        rt.inject_event(WinEvent::Minimized(w.0));
        assert!(overlay_visible(w.0), "still fading");
        pump_ms(ANIMATION_WAIT_MS);
        assert!(!overlay_visible(w.0));
    });
}

#[test]
fn without_animation_a_minimize_hides_the_pin_at_once() {
    run_with(false, |rt, w| {
        // SAFETY: own window.
        let _ = unsafe { ShowWindow(to_hwnd(w.0), SW_MINIMIZE) };
        rt.inject_event(WinEvent::Minimized(w.0));
        assert!(!overlay_visible(w.0));
    });
}

#[test]
fn a_restore_from_minimized_waits_invisible_and_fades_the_pin_in() {
    run_with(true, |rt, w| {
        // SAFETY: own window.
        let _ = unsafe { ShowWindow(to_hwnd(w.0), SW_MINIMIZE) };
        rt.inject_event(WinEvent::Minimized(w.0));
        pump_ms(ANIMATION_WAIT_MS);
        // SAFETY: own window; without activation, like `NativeWin::maximize`.
        let _ = unsafe { ShowWindow(to_hwnd(w.0), SW_SHOWNOACTIVATE) };
        rt.inject_event(WinEvent::Restored(w.0));
        assert_eq!(alpha(w.0), 0);
        pump_ms(ANIMATION_WAIT_MS);
        assert_eq!(alpha(w.0), 255);
        assert!(overlay_visible(w.0));
    });
}
