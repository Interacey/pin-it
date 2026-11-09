//! Style and ownership of a real overlay window.

use crate::win32::to_hwnd;
use windows::Win32::UI::WindowsAndMessaging::{
    GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongW, GetWindowRect, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

use super::test_support::{setup, teardown};

#[test]
fn overlay_has_expected_styles_and_does_not_take_focus() {
    let _guard = crate::window_test_guard();
    let (target, notify, b) = setup();
    let ex = unsafe { GetWindowLongW(to_hwnd(b.hwnd()), GWL_EXSTYLE) } as u32;
    assert_ne!(ex & WS_EX_LAYERED.0, 0);
    assert_ne!(ex & WS_EX_NOACTIVATE.0, 0);
    assert_ne!(ex & WS_EX_TOOLWINDOW.0, 0);
    let mut r = windows::Win32::Foundation::RECT::default();
    unsafe { GetWindowRect(to_hwnd(b.hwnd()), &mut r).unwrap() };
    assert_eq!((r.right - r.left, r.bottom - r.top), (32, 32));
    teardown(target, notify, b);
}

#[test]
fn overlay_is_unowned() {
    let _guard = crate::window_test_guard();
    let (target, notify, b) = setup();
    let owner = unsafe { GetWindow(to_hwnd(b.hwnd()), GW_OWNER) };
    assert!(owner.is_err() || owner.unwrap().0.is_null());
    teardown(target, notify, b);
}

/// Counts the pixels of the 32 px box that a hit test at the screen point maps to the overlay.
fn overlay_hit_pixels(overlay: windows::Win32::Foundation::HWND) -> usize {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::UI::WindowsAndMessaging::WindowFromPoint;
    let mut r = RECT::default();
    unsafe { GetWindowRect(overlay, &mut r).unwrap() };
    let mut hits = 0;
    for y in r.top..r.bottom {
        for x in r.left..r.right {
            if unsafe { WindowFromPoint(POINT { x, y }) } == overlay {
                hits += 1;
            }
        }
    }
    hits
}

#[test]
fn whole_button_box_is_clickable() {
    let _guard = crate::window_test_guard();
    let (target, notify, b) = setup();
    let overlay = to_hwnd(b.hwnd());
    // Topmost, so a foreign window in front of the corner cannot take the hits.
    unsafe {
        SetWindowPos(
            overlay,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
        .unwrap()
    };
    let hits = overlay_hit_pixels(overlay);
    println!("hit pixels on the overlay: {hits} of 1024");
    assert_eq!(hits, 32 * 32);
    teardown(target, notify, b);
}

// ----- fades while Windows animates the target -----

const FADE_OUT_WAIT_MS: u64 = 300;
const HOLD_AND_FADE_IN_WAIT_MS: u64 = 600;

fn pump_for(ms: u64) {
    let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < end {
        super::test_support::pump();
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
}

fn is_visible(b: &super::PinButton) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
    unsafe { IsWindowVisible(to_hwnd(b.hwnd())) }.as_bool()
}

fn hit_test(b: &super::PinButton) -> isize {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_NCHITTEST};
    unsafe {
        SendMessageW(
            to_hwnd(b.hwnd()),
            WM_NCHITTEST,
            Some(WPARAM(0)),
            Some(LPARAM(0)),
        )
    }
    .0
}

#[test]
fn a_fade_out_keeps_the_overlay_on_screen_until_it_is_over_and_then_hides_it() {
    let _guard = crate::window_test_guard();
    let (target, notify, mut b) = setup();
    b.show();
    b.fade_out_then_hide(true);
    assert!(is_visible(&b), "the fade has only started");
    assert_eq!(b.alpha(), 255);
    pump_for(FADE_OUT_WAIT_MS);
    assert!(!is_visible(&b), "hidden once the fade is over");
    teardown(target, notify, b);
}

#[test]
fn without_animation_the_overlay_is_hidden_at_once() {
    let _guard = crate::window_test_guard();
    let (target, notify, mut b) = setup();
    b.show();
    b.fade_out_then_hide(false);
    assert!(!is_visible(&b));
    b.hold_back(false);
    assert_eq!(b.alpha(), 255, "no animation: nothing is held back");
    teardown(target, notify, b);
}

#[test]
fn a_held_back_overlay_is_click_through_and_returns_after_the_window_animation() {
    let _guard = crate::window_test_guard();
    let (target, notify, mut b) = setup();
    b.show();
    b.hold_back(true);
    assert_eq!(b.alpha(), 0);
    assert_eq!(hit_test(&b), -1, "HTTRANSPARENT while invisible");
    pump_for(HOLD_AND_FADE_IN_WAIT_MS);
    assert_eq!(b.alpha(), 255);
    assert_ne!(hit_test(&b), -1);
    teardown(target, notify, b);
}

#[test]
fn showing_a_faded_out_overlay_again_makes_it_opaque() {
    let _guard = crate::window_test_guard();
    let (target, notify, mut b) = setup();
    b.show();
    b.fade_out_then_hide(true);
    pump_for(FADE_OUT_WAIT_MS);
    b.show();
    assert_eq!(b.alpha(), 255);
    assert!(is_visible(&b));
    teardown(target, notify, b);
}
