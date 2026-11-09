//! The dial window with its own message loop: messages are posted to the dial window only, never
//! sent as input to other applications. The click sound goes through a test player that makes no
//! sound and counts the requests.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SPACE, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindow, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
};

use super::support::{CHANGED, Recorder, eventually, post, pump_ms, received};
use crate::Hwnd;
use crate::dial::{self, Request, probe};
use crate::win32::to_hwnd;

/// Closes the dial when a test ends, also when it fails.
struct CloseDial;

impl Drop for CloseDial {
    fn drop(&mut self) {
        let _ = dial::close();
        pump_ms(20);
    }
}

fn request(rec: &Recorder, percent: u32, click_sound: bool) -> Request {
    Request {
        notify: rec.0,
        message: CHANGED,
        percent,
        click_sound,
    }
}

fn open_dial(rec: &Recorder, percent: u32) -> Hwnd {
    dial::open(request(rec, percent, true)).unwrap()
}

fn is_open(h: Hwnd) -> bool {
    unsafe { IsWindow(Some(to_hwnd(h))).as_bool() }
}

fn lparam(x: i32, y: i32) -> isize {
    ((y as isize & 0xFFFF) << 16) | (x as isize & 0xFFFF)
}

fn key(h: Hwnd, vk: VIRTUAL_KEY) {
    post(h, WM_KEYDOWN, vk.0 as usize, 0);
    pump_ms(20);
}

/// A key press followed by a pause longer than the gap the click rate limit leaves.
fn key_then_wait(h: Hwnd, vk: VIRTUAL_KEY) {
    key(h, vk);
    pump_ms(MORE_THAN_CLICK_GAP_MS);
}

fn wheel(h: Hwnd, delta: i16) {
    post(h, WM_MOUSEWHEEL, (delta as u16 as usize) << 16, 0);
    pump_ms(20);
}

fn settle() {
    eventually("the knob settled", || probe::animating() == Some(false));
}

/// The click limit allows one click per 40 ms.
const MORE_THAN_CLICK_GAP_MS: u64 = 50;

/// Moves the pointer along the arc at 80 dp from `from` to `to` degrees in moves of
/// `step_deg`, like a hand turning the knob, and waits `pause_ms` after each move.
fn drag_arc(h: Hwnd, from: f32, to: f32, step_deg: f32, pause_ms: u64) {
    let at = |angle| probe::point_at(angle, 80.0).unwrap();
    let (x, y) = at(from);
    post(h, WM_LBUTTONDOWN, 1, lparam(x, y));
    pump_ms(10);
    let steps = ((to - from).abs() / step_deg).ceil() as i32;
    for i in 1..=steps {
        let (x, y) = at(from + (to - from) * i as f32 / steps as f32);
        post(h, WM_MOUSEMOVE, 1, lparam(x, y));
        pump_ms(pause_ms);
    }
    let (x, y) = at(to);
    post(h, WM_LBUTTONUP, 0, lparam(x, y));
    pump_ms(20);
}

#[test]
fn opening_twice_gives_one_window() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let first = open_dial(&rec, 100);
    let second = open_dial(&rec, 150);
    assert_eq!(first, second);
    assert_eq!(probe::percent(), Some(100), "the open dial keeps its value");
}

#[test]
fn dragging_snaps_to_a_detent_and_posts_each_change() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, 100);
    // 100 percent sits at 0 degrees (2.7 degrees per percent, 13.5 per step); a turn of 70
    // degrees passes 105 to 120 and ends at 125.9 percent, which snaps to 125.
    drag_arc(h, 0.0, 70.0, 4.0, 8);
    settle();
    pump_ms(20);
    assert_eq!(probe::percent(), Some(125));
    assert_eq!(received(), [105, 110, 115, 120, 125]);
}

#[test]
fn releasing_between_detents_snaps_to_the_nearest() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, 100);
    // 5 degrees is short of the midpoint to 105 (6.75 degrees) plus its margin: the knob
    // returns to 100 and no change is posted.
    drag_arc(h, 0.0, 5.0, 4.0, 8);
    settle();
    assert_eq!(probe::percent(), Some(100));
    assert!(received().is_empty());
    // 10 degrees is clearly past the boundary: 105.
    let h = open_dial(&rec, 100);
    drag_arc(h, 0.0, 10.0, 4.0, 8);
    settle();
    assert_eq!(probe::percent(), Some(105));
    assert_eq!(received(), [105]);
}

#[test]
fn keys_wheel_and_home_end_step_five_percent() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, 100);
    key(h, VK_RIGHT);
    key(h, VK_UP);
    key(h, VK_LEFT);
    key(h, VK_DOWN);
    key(h, VK_END);
    key(h, VK_HOME);
    wheel(h, 120);
    wheel(h, -120);
    wheel(h, -120);
    assert_eq!(received(), [105, 110, 105, 100, 150, 50, 55, 50]);
    assert_eq!(probe::percent(), Some(50));
}

#[test]
fn escape_closes_without_a_change_and_the_dial_can_open_again() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, 100);
    key(h, VK_ESCAPE);
    eventually("window closed", || !is_open(h));
    assert!(received().is_empty());
    assert_eq!(probe::percent(), None);
    let again = open_dial(&rec, 120);
    assert!(is_open(again));
    assert_eq!(probe::percent(), Some(120));
}

#[test]
fn enter_and_space_confirm_and_close() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    for confirm in [VK_RETURN, VK_SPACE] {
        let h = open_dial(&rec, 100);
        key(h, VK_RIGHT);
        key(h, confirm);
        eventually("window closed", || !is_open(h));
    }
    assert_eq!(received(), [105, 105]);
}

#[test]
fn a_change_that_cannot_be_posted_closes_the_dial() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, 100);
    drop(rec);
    key(h, VK_RIGHT);
    eventually("window closed", || !is_open(h));
}

#[test]
fn a_percent_above_the_dial_range_is_shown_at_the_upper_end() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    open_dial(&rec, 200);
    assert_eq!(probe::percent(), Some(150));
    assert!(received().is_empty(), "opening must not change the setting");
}

#[test]
fn close_destroys_an_open_dial_and_is_fine_without_one() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let h = open_dial(&rec, 100);
    dial::close().unwrap();
    assert!(!is_open(h));
    dial::close().unwrap();
}

#[test]
fn a_detent_crossing_runs_its_effects_to_the_end_and_the_timer_stops() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, 100);
    assert_eq!(probe::flashing(), None);
    post(h, WM_KEYDOWN, VK_RIGHT.0 as usize, 0);
    eventually("the key was handled", || probe::percent() == Some(105));
    settle();
    assert_eq!(probe::flashing(), None);
}

#[test]
fn a_drag_over_five_detents_asks_for_five_clicks() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, 100);
    probe::reset_clicks();
    // One detent (13.5 degrees) per move, with a pause longer than the click rate limit.
    drag_arc(h, 0.0, 67.5, 13.5, MORE_THAN_CLICK_GAP_MS);
    assert_eq!(received(), [105, 110, 115, 120, 125]);
    assert_eq!(probe::clicks(), 5);
}

#[test]
fn keys_and_the_wheel_click_once_per_step() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, 100);
    probe::reset_clicks();
    key_then_wait(h, VK_RIGHT);
    key_then_wait(h, VK_LEFT);
    post(h, WM_MOUSEWHEEL, (120u16 as usize) << 16, 0);
    pump_ms(MORE_THAN_CLICK_GAP_MS);
    assert_eq!(probe::clicks(), 3);
    // A key that changes nothing is silent.
    key_then_wait(h, VK_HOME);
    let after_home = probe::clicks();
    key_then_wait(h, VK_HOME);
    assert_eq!(probe::clicks(), after_home);
}

#[test]
fn a_fast_burst_of_keys_is_limited_to_the_click_rate() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, 50);
    probe::reset_clicks();
    for _ in 0..10 {
        post(h, WM_KEYDOWN, VK_RIGHT.0 as usize, 0);
    }
    pump_ms(20);
    assert_eq!(received().len(), 10, "every step still changes the size");
    assert!(probe::clicks() <= 2, "{} clicks", probe::clicks());
}

#[test]
fn no_click_is_requested_when_the_setting_is_off() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = dial::open(request(&rec, 100, false)).unwrap();
    probe::reset_clicks();
    key_then_wait(h, VK_RIGHT);
    key_then_wait(h, VK_RIGHT);
    drag_arc(h, 0.0, 27.0, 13.5, MORE_THAN_CLICK_GAP_MS);
    assert!(!received().is_empty());
    assert_eq!(probe::clicks(), 0);
}

#[test]
fn no_click_is_requested_after_the_dial_started_closing() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, 100);
    probe::reset_clicks();
    // Confirm and a further step are queued together: the window is gone before the step.
    post(h, WM_KEYDOWN, VK_RETURN.0 as usize, 0);
    post(h, WM_KEYDOWN, VK_RIGHT.0 as usize, 0);
    eventually("window closed", || !is_open(h));
    pump_ms(MORE_THAN_CLICK_GAP_MS);
    assert_eq!(probe::clicks(), 0);
    assert!(received().is_empty());
}
