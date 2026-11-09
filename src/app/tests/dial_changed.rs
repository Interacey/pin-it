//! `WM_DIAL_CHANGED` from the button size dial: it must take the same path as a menu command.

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::WPARAM;
use windows::Win32::UI::WindowsAndMessaging::SendMessageW;

use super::support::{Wins, options, post, pump_ms, temp_dir};
use crate::app::Runtime;
use crate::app::host::WM_DIAL_CHANGED;
use crate::app::settings_observers::SettingsEvent;
use crate::config::{Settings, load_from};
use crate::win32::to_hwnd;

type Seen = Rc<RefCell<Vec<(SettingsEvent, u32)>>>;

/// Posts the dial message and returns what the settings observers saw.
fn post_percent(rt: &Runtime, percent: usize) -> Seen {
    let seen: Seen = Rc::default();
    let sink = Rc::clone(&seen);
    rt.settings_events().subscribe_fn(move |event, app| {
        sink.borrow_mut()
            .push((*event, app.settings.button_scale_percent));
    });
    post(rt.hwnd(), WM_DIAL_CHANGED, percent, 0);
    pump_ms(100);
    seen
}

#[test]
fn a_dial_message_sets_the_scale_through_the_observers_and_saves_it() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("dial-changed");
    let opts = options(&dir, wins.0.clone(), &Settings::default());
    let path = opts.settings_path.clone();
    let rt = Runtime::start(&opts).unwrap();

    let seen = post_percent(&rt, 150);

    assert_eq!(*seen.borrow(), [(SettingsEvent::Changed, 150)]);
    assert_eq!(rt.settings().button_scale_percent, 150);
    assert_eq!(load_from(&path).button_scale_percent, 150, "not saved");
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn out_of_range_dial_values_are_clamped() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("dial-clamp");
    let rt = Runtime::start(&options(&dir, wins.0.clone(), &Settings::default())).unwrap();
    for (sent, expected) in [(5, 50), (500, 200), (usize::MAX, 200)] {
        post_percent(&rt, sent);
        assert_eq!(rt.settings().button_scale_percent, expected, "sent {sent}");
    }
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_dial_message_that_arrives_while_the_app_is_busy_is_posted_again_and_applied() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("dial-busy");
    let rt = Runtime::start(&options(&dir, wins.0.clone(), &Settings::default())).unwrap();

    // An `App` method is running: the message must not be handled nested, and not be lost.
    rt.while_busy(|| {
        // Dispatched synchronously, like a sent message during a cross-thread call.
        unsafe { SendMessageW(to_hwnd(rt.hwnd()), WM_DIAL_CHANGED, Some(WPARAM(130)), None) };
    });
    assert_eq!(
        rt.settings().button_scale_percent,
        100,
        "handled inside a running App method"
    );
    pump_ms(100);

    assert_eq!(rt.settings().button_scale_percent, 130, "message was lost");
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}
