//! The easter egg of the dial in a real dial window: keys are posted to our own window only and
//! the sound goes through the counting test player, so nothing is ever played.

use windows::Win32::UI::Input::KeyboardAndMouse::{VIRTUAL_KEY, VK_END, VK_HOME};
use windows::Win32::UI::WindowsAndMessaging::WM_KEYDOWN;

use super::support::{CHANGED, Recorder, post, pump_ms};
use crate::Hwnd;
use crate::dial::{self, Request, probe};

/// Closes the dial when a test ends, also when it fails.
struct CloseDial;

impl Drop for CloseDial {
    fn drop(&mut self) {
        let _ = dial::close();
        pump_ms(20);
    }
}

/// Longer than the click rate limit (40 ms), so every key press would click on its own.
const KEY_GAP_MS: u64 = 60;

fn open_dial(rec: &Recorder, click_sound: bool) -> Hwnd {
    probe::reset_clicks();
    dial::open(Request {
        notify: rec.0,
        message: CHANGED,
        percent: 100,
        click_sound,
    })
    .unwrap()
}

fn key(h: Hwnd, vk: VIRTUAL_KEY) {
    post(h, WM_KEYDOWN, vk.0 as usize, 0);
    pump_ms(KEY_GAP_MS);
}

fn home_end_home(h: Hwnd) {
    key(h, VK_HOME);
    key(h, VK_END);
    key(h, VK_HOME);
}

#[test]
fn home_end_home_within_three_seconds_starts_the_jingle_once() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, true);
    home_end_home(h);
    assert_eq!(probe::percent(), Some(50));
    assert_eq!(probe::egg_requests(), 1);
}

#[test]
fn clicks_stop_while_the_jingle_plays() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, true);
    home_end_home(h);
    // Home and End clicked; the Home that completed the pattern and what follows are silent.
    assert_eq!(probe::clicks(), 2);
    key(h, VK_END);
    key(h, VK_HOME);
    assert_eq!(probe::clicks(), 2);
    assert_eq!(
        probe::egg_requests(),
        1,
        "the second round trip is in the cooldown"
    );
}

#[test]
fn nothing_plays_when_the_click_setting_is_off() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, false);
    home_end_home(h);
    assert_eq!(probe::egg_requests(), 0);
    assert_eq!(probe::clicks(), 0);
}

#[test]
fn a_slow_round_trip_is_not_an_egg() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, true);
    key(h, VK_HOME);
    key(h, VK_END);
    pump_ms(3_100);
    key(h, VK_HOME);
    assert_eq!(probe::egg_requests(), 0);
}

#[test]
fn closing_the_dial_stops_the_jingle() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, true);
    home_end_home(h);
    assert_eq!(probe::egg_stops(), 0);
    dial::close().unwrap();
    pump_ms(20);
    assert_eq!(probe::egg_stops(), 1);
}

#[test]
fn closing_without_a_jingle_stops_nothing() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, true);
    key(h, VK_HOME);
    dial::close().unwrap();
    pump_ms(20);
    assert_eq!(probe::egg_stops(), 0);
}

#[test]
fn a_new_dial_after_the_egg_does_not_start_it_again() {
    let _g = crate::window_test_guard();
    let rec = Recorder::new();
    let _close = CloseDial;
    let h = open_dial(&rec, true);
    home_end_home(h);
    dial::close().unwrap();
    pump_ms(20);
    let again = dial::open(Request {
        notify: rec.0,
        message: CHANGED,
        percent: 50,
        click_sound: true,
    })
    .unwrap();
    pump_ms(20);
    assert_eq!(probe::egg_requests(), 1, "opening alone plays nothing");
    key(again, VK_END);
    assert_eq!(probe::egg_requests(), 1);
}
