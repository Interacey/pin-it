//! The pin and settings events seen from outside: what is announced, in which order, and what
//! state the observers find.

use std::cell::RefCell;
use std::rc::Rc;

use super::support::{Wins, click, eventually_swept, options, pump_ms, temp_dir};
use crate::app::Runtime;
use crate::app::pin_observers::PinEvent;
use crate::app::settings_observers::SettingsEvent;
use crate::config::Settings;
use crate::tray::TrayCommand;
use crate::zorder::is_topmost;

type Recorded<T> = Rc<RefCell<Vec<T>>>;

/// The events that change the pin list. The system may refuse the first pin of a test window; the
/// controller then announces `Refused` and, once the same toggle or a sweep applies the pin,
/// `Recovered` instead of `Pinned`. Both are the same settled outcome here, so `Refused` is left
/// out and `Recovered` reads as `Pinned`. The soft-failure path itself is tested in `pin_safety`.
fn pin_list_events(recorded: &Recorded<PinEvent>) -> Vec<PinEvent> {
    recorded
        .borrow()
        .iter()
        .filter_map(|event| match *event {
            PinEvent::OrderChanged | PinEvent::Refused(_) => None,
            PinEvent::Recovered(h) => Some(PinEvent::Pinned(h)),
            other => Some(other),
        })
        .collect()
}

#[test]
fn pin_and_unpin_are_announced_in_click_order() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    let dir = temp_dir("events-order");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(100);
    let seen: Recorded<PinEvent> = Rc::default();
    let sink = Rc::clone(&seen);
    rt.pin_events()
        .subscribe_fn(move |event, _| sink.borrow_mut().push(*event));

    click(rt.button_hwnd(w[0]).unwrap());
    click(rt.button_hwnd(w[1]).unwrap());
    eventually_swept(&rt, "A and B topmost", || {
        is_topmost(w[0]) && is_topmost(w[1])
    });
    click(rt.button_hwnd(w[0]).unwrap());

    assert_eq!(
        pin_list_events(&seen),
        [
            PinEvent::Pinning(w[0]),
            PinEvent::Pinned(w[0]),
            PinEvent::Pinning(w[1]),
            PinEvent::Pinned(w[1]),
            PinEvent::Unpinned(w[0])
        ]
    );
    assert!(
        seen.borrow().contains(&PinEvent::OrderChanged),
        "order enforcement was not announced"
    );
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn observers_find_the_new_state_and_the_written_pins_file() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let w = wins.0.clone();
    let dir = temp_dir("events-state");
    let pins_file = dir.join("pins.json");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(100);
    let seen: Recorded<(PinEvent, bool, bool)> = Rc::default();
    let sink = Rc::clone(&seen);
    // Subscribed last, so it runs after the built-in observers.
    rt.pin_events().subscribe_fn(move |event, app| {
        let in_list = !app.pinned_order().is_empty();
        sink.borrow_mut()
            .push((*event, in_list, pins_file.exists()));
    });

    click(rt.button_hwnd(w[0]).unwrap());
    eventually_swept(&rt, "window topmost", || is_topmost(w[0]));
    click(rt.button_hwnd(w[0]).unwrap());

    let seen = seen.borrow();
    let find = |wanted: fn(&PinEvent) -> bool| {
        seen.iter()
            .find(|(event, _, _)| wanted(event))
            .map(|&(_, in_list, file)| (in_list, file))
    };
    let pinned = find(|e| matches!(e, PinEvent::Pinned(_) | PinEvent::Recovered(_)));
    let unpinned = find(|e| matches!(e, PinEvent::Unpinned(_)));
    assert_eq!(pinned, Some((true, true)), "Pinned: list and file");
    assert_eq!(unpinned, Some((false, false)), "Unpinned: list and file");
    drop(seen);
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn applied_settings_are_announced_once_with_the_new_values() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let dir = temp_dir("events-settings");
    let rt = Runtime::start(&options(&dir, wins.0.clone(), &Settings::default())).unwrap();
    let seen: Recorded<(SettingsEvent, bool)> = Rc::default();
    let sink = Rc::clone(&seen);
    rt.settings_events()
        .subscribe_fn(move |event, app| sink.borrow_mut().push((*event, app.settings.animate)));

    rt.run_tray_command(&TrayCommand::ToggleAnimation);

    assert_eq!(*seen.borrow(), [(SettingsEvent::Changed, false)]);
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}
