//! A native window that is maximized, restored and resized keeps its pin right next to the caption buttons,
//! inside the visible caption. Own windows only; a "maximized" window fills a small area at the lower right.

use std::time::{Duration, Instant};

use super::pb_common::{NativeWin, snapshot};
use super::support::{log_text, options, pump_ms, temp_dir};
use crate::app::Runtime;
use crate::config::Settings;
use crate::events::WinEvent;

const SETTLE: Duration = Duration::from_secs(4);
const COLORED_BAR: [u8; 3] = [200, 60, 60];
/// The pin is checked this often, this far apart, after a change.
const HOLD_CHECKS: u64 = 12;
const HOLD_STEP_MS: u64 = 50;
const SHRINK_FROM: i32 = 440;
const SHRINK_TO: i32 = 300;
const SHRINK_STEP: usize = 20;

/// Pumps until the pin sits next to the buttons, or fails with what the placement saw.
fn settle(w: &NativeWin, step: &str) {
    let deadline = Instant::now() + SETTLE;
    loop {
        match w.pin_beside_buttons() {
            Ok(()) => return,
            Err(why) if Instant::now() >= deadline => {
                panic!("{step}: {why}\n{}", snapshot(w))
            }
            Err(_) => pump_ms(25),
        }
    }
}

/// Applies a change, injects the location event, and checks the pin right away and again while the
/// new scan arrives: the scan taken during a size change must not move the pin away.
fn change(rt: &Runtime, w: &NativeWin, step: &str, apply: impl Fn(&NativeWin)) {
    apply(w);
    rt.inject_event(WinEvent::Moved(w.0));
    settle(w, step);
    for _ in 0..HOLD_CHECKS {
        pump_ms(HOLD_STEP_MS);
        if let Err(why) = w.pin_beside_buttons() {
            panic!(
                "{step}, after the scan: {why}
{}",
                snapshot(w)
            );
        }
    }
}

fn run_with<R>(caption: Option<[u8; 3]>, body: impl FnOnce(&Runtime, &NativeWin) -> R) -> R {
    let _g = crate::window_test_guard();
    let w = NativeWin::new(0, caption);
    let dir = temp_dir("pb-max");
    let rt = Runtime::start(&options(&dir, vec![w.0], &Settings::default())).unwrap();
    settle(&w, "start");
    let result = body(&rt, &w);
    let log = log_text(&dir);
    drop(rt);
    drop(w);
    let _ = std::fs::remove_dir_all(dir);
    // a toggle of maximize and restore must not make the button flicker: no hide, one show
    assert_eq!(log.matches("button hidden").count(), 0, "{log}");
    assert_eq!(log.matches("button shown").count(), 1, "{log}");
    result
}

fn maximize_and_restore_twice(rt: &Runtime, w: &NativeWin) {
    for round in 1..=2 {
        change(
            rt,
            w,
            &format!("maximized, round {round}"),
            NativeWin::maximize,
        );
        assert!(w.is_zoomed());
        change(
            rt,
            w,
            &format!("restored, round {round}"),
            NativeWin::restore,
        );
        assert!(!w.is_zoomed());
    }
}

#[test]
fn a_maximized_plain_native_window_keeps_the_pin_next_to_its_buttons() {
    run_with(None, maximize_and_restore_twice);
}

#[test]
fn a_maximized_native_window_with_a_colored_caption_keeps_its_pin() {
    run_with(Some(COLORED_BAR), maximize_and_restore_twice);
}

#[test]
fn the_pin_is_beside_the_buttons_in_the_same_event_cycle_of_every_shrink_step() {
    for caption in [None, Some(COLORED_BAR)] {
        run_with(caption, |rt, w| {
            for width in (SHRINK_TO..=SHRINK_FROM).rev().step_by(SHRINK_STEP) {
                w.resize_to(width);
                rt.inject_event(WinEvent::Moved(w.0));
                // no pumping: the slot must be right as soon as the event is handled
                if let Err(why) = w.pin_beside_buttons() {
                    panic!("width {width}, caption {caption:?}: {why}\n{}", snapshot(w));
                }
            }
        });
    }
}
