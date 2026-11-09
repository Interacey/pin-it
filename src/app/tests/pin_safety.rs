//! Crash safety and soft failures of the pin lifecycle: write-ahead order, pending releases,
//! refused pins, the "recovered" announcement and activation on pin.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use windows::Win32::UI::WindowsAndMessaging::{
    HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
};

use super::support::{Wins, click, eventually, eventually_swept, options, sweep, temp_dir};
use crate::Hwnd;
use crate::app::host::with_app;
use crate::app::panic_registry;
use crate::app::pin_observers::PinEvent;
use crate::app::state::App;
use crate::app::{RunOptions, Runtime};
use crate::config::Settings;
use crate::win32::to_hwnd;
use crate::zorder::{
    self, activate_hooks, fail_unpin_of, is_topmost, refuse_next_pin_of, refuse_pin_of,
};

type Seen<T> = Rc<RefCell<Vec<T>>>;

/// Subscribes last, so the built-in observers have run when `probe` is called.
fn record<T: 'static>(rt: &Runtime, probe: impl Fn(&PinEvent, &mut App) -> T + 'static) -> Seen<T> {
    let seen: Seen<T> = Rc::default();
    let sink = Rc::clone(&seen);
    rt.pin_events()
        .subscribe_fn(move |event, app| sink.borrow_mut().push(probe(event, app)));
    seen
}

struct Fixture {
    dir: PathBuf,
    opts: RunOptions,
    wins: Wins,
}

impl Fixture {
    fn new(tag: &str, windows: usize) -> Fixture {
        let wins = Wins::new(windows);
        let dir = temp_dir(tag);
        let opts = options(&dir, wins.0.clone(), &Settings::default());
        Fixture { dir, opts, wins }
    }

    fn w(&self) -> Vec<Hwnd> {
        self.wins.0.clone()
    }

    fn pins_file(&self) -> PathBuf {
        self.dir.join("pins.json")
    }

    fn start(&self) -> Runtime {
        let rt = Runtime::start(&self.opts).unwrap();
        let offered = self.opts.only.as_ref().map_or(0, Vec::len);
        eventually("a button for every test window", || {
            rt.button_count() == offered
        });
        rt
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn pins_json_has(f: &Fixture, h: Hwnd, needle: &str) -> bool {
    std::fs::read_to_string(f.pins_file())
        .is_ok_and(|t| t.contains(&format!("\"hwnd\":{h}")) && t.contains(needle))
}

fn pin_list_events(seen: &Seen<PinEvent>) -> Vec<PinEvent> {
    seen.borrow()
        .iter()
        .copied()
        .filter(|e| *e != PinEvent::OrderChanged)
        .collect()
}

#[test]
fn window_is_recorded_before_it_becomes_topmost() {
    let _g = crate::window_test_guard();
    let f = Fixture::new("wal", 1);
    let h = f.w()[0];
    let rt = f.start();
    let file = f.pins_file();
    let seen = record(&rt, move |event, _| match *event {
        PinEvent::Pinning(w) => Some((
            is_topmost(w),
            std::fs::read_to_string(&file).ok(),
            panic_registry::current().iter().any(|&(x, _)| x == w),
        )),
        _ => None,
    });
    click(rt.button_hwnd(h).unwrap());
    let at_pinning = seen.borrow().iter().flatten().next().cloned().unwrap();
    assert!(!at_pinning.0, "already topmost when announced");
    assert!(
        at_pinning
            .1
            .is_some_and(|t| t.contains(&format!("\"hwnd\":{h}")))
    );
    assert!(at_pinning.2, "panic registry did not know the window yet");
    eventually_swept(&rt, "window topmost", || is_topmost(h));
}

#[test]
fn crash_between_record_and_pin_is_handled_by_the_next_start() {
    let _g = crate::window_test_guard();
    let f = Fixture::new("wal-crash", 1);
    let h = f.w()[0];
    let rt = f.start();
    let file = f.pins_file();
    let seen = record(&rt, move |event, _| match *event {
        PinEvent::Pinning(_) => std::fs::read_to_string(&file).ok(),
        _ => None,
    });
    click(rt.button_hwnd(h).unwrap());
    let written_before_pin = seen.borrow().iter().flatten().next().cloned().unwrap();
    rt.crash();

    // Killed before the pin: the window is not topmost, the record must do no harm.
    zorder::unpin(h).unwrap();
    std::fs::write(f.pins_file(), &written_before_pin).unwrap();
    let rt = f.start();
    assert!(!is_topmost(h));
    assert!(!f.pins_file().exists(), "record was not consumed");
    rt.crash();

    // Killed right after the pin: the same record lets the next start release the window.
    zorder::pin(h).unwrap();
    std::fs::write(f.pins_file(), &written_before_pin).unwrap();
    let rt = f.start();
    assert!(
        !is_topmost(h),
        "record from before the pin did not release it"
    );
    drop(rt);
}

#[test]
fn refused_pin_keeps_its_entry_and_is_announced_once() {
    let _g = crate::window_test_guard();
    let f = Fixture::new("refused", 1);
    let h = f.w()[0];
    let rt = f.start();
    let seen = record(&rt, |event, _| *event);
    let refusing = refuse_pin_of(h);
    click(rt.button_hwnd(h).unwrap());
    for _ in 0..3 {
        sweep(&rt);
    }
    assert_eq!(rt.pinned(), vec![h], "soft failure dropped the pin");
    assert_eq!(
        pin_list_events(&seen),
        vec![PinEvent::Pinning(h), PinEvent::Refused(h)]
    );

    drop(refusing);
    eventually_swept(&rt, "pin applied once the refusal ends", || is_topmost(h));
    assert!(seen.borrow().contains(&PinEvent::Recovered(h)));
}

#[test]
fn recovered_is_announced_only_when_the_pin_was_applied() {
    let _g = crate::window_test_guard();
    let f = Fixture::new("recovered", 2);
    let w = f.w();
    let rt = f.start();
    click(rt.button_hwnd(w[0]).unwrap());
    click(rt.button_hwnd(w[1]).unwrap());
    let seen = record(&rt, |event, _| *event);
    with_app(|a| a.hung.insert(w[0]));

    sweep(&rt);
    assert!(
        !seen.borrow().contains(&PinEvent::Recovered(w[0])),
        "recovered although nothing was probed or pinned"
    );

    // Disturb the order so the sweep has to pin again.
    unsafe {
        SetWindowPos(
            to_hwnd(w[0]),
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    }
    .unwrap();
    sweep(&rt);
    assert!(seen.borrow().contains(&PinEvent::Recovered(w[0])));
    assert_eq!(with_app(|a| a.hung.contains(&w[0])), Some(false));
}

#[test]
fn failed_release_stays_recorded_and_is_retried_by_the_next_start() {
    let _g = crate::window_test_guard();
    let f = Fixture::new("pending", 1);
    let h = f.w()[0];
    let rt = f.start();
    let failing = fail_unpin_of(h);
    click(rt.button_hwnd(h).unwrap());
    click(rt.button_hwnd(h).unwrap());
    assert!(rt.pinned().is_empty());
    assert!(is_topmost(h), "release was not supposed to work");
    assert!(
        pins_json_has(&f, h, "\"pending_release\":true"),
        "pending release not persisted: {:?}",
        std::fs::read_to_string(f.pins_file())
    );
    assert!(panic_registry::current().iter().any(|&(x, _)| x == h));

    sweep(&rt);
    assert!(is_topmost(h) && f.pins_file().exists());

    // A crash now leaves the record; the next start retries the release.
    rt.crash();
    drop(failing);
    let rt = f.start();
    assert!(
        !is_topmost(h),
        "next start did not retry the pending release"
    );
    assert!(!f.pins_file().exists());
    drop(rt);
}

#[test]
fn sweep_settles_a_pending_release_once_the_window_takes_it() {
    let _g = crate::window_test_guard();
    let f = Fixture::new("pending-sweep", 1);
    let h = f.w()[0];
    let rt = f.start();
    let seen = record(&rt, |event, _| *event);
    let failing = fail_unpin_of(h);
    click(rt.button_hwnd(h).unwrap());
    click(rt.button_hwnd(h).unwrap());
    sweep(&rt);
    assert!(!seen.borrow().contains(&PinEvent::ReleaseSettled(h)));

    drop(failing);
    sweep(&rt);
    assert!(!is_topmost(h));
    assert!(seen.borrow().contains(&PinEvent::ReleaseSettled(h)));
    assert!(!f.pins_file().exists());
    assert_eq!(with_app(|a| a.pending_release.len()), Some(0));
}

#[test]
fn pinning_again_while_a_release_is_pending_does_not_count_as_own_topmost() {
    let _g = crate::window_test_guard();
    let f = Fixture::new("pending-repin", 1);
    let h = f.w()[0];
    let rt = f.start();
    let failing = fail_unpin_of(h);
    click(rt.button_hwnd(h).unwrap());
    click(rt.button_hwnd(h).unwrap());
    click(rt.button_hwnd(h).unwrap());
    assert_eq!(rt.pinned(), vec![h]);
    assert_eq!(with_app(|a| a.pending_release.len()), Some(0));

    drop(failing);
    click(rt.button_hwnd(h).unwrap());
    assert!(
        !is_topmost(h),
        "window was remembered as topmost before our pin and kept on top"
    );
}

#[test]
fn only_a_new_pin_takes_the_foreground() {
    let _g = crate::window_test_guard();
    let f = Fixture::new("activate", 1);
    let h = f.w()[0];
    let rt = f.start();
    let before = activate_hooks::calls();
    click(rt.button_hwnd(h).unwrap());
    assert_eq!(activate_hooks::calls(), before + 1, "pin did not activate");
    sweep(&rt);
    sweep(&rt);
    click(rt.button_hwnd(h).unwrap());
    assert_eq!(
        activate_hooks::calls(),
        before + 1,
        "sweep or unpin activated a window"
    );
    drop(rt);
}

#[test]
fn pin_recovered_in_the_same_toggle_takes_the_foreground_once() {
    let _g = crate::window_test_guard();
    let f = Fixture::new("activate-recovered", 1);
    let h = f.w()[0];
    let rt = f.start();
    let seen = record(&rt, |event, _| *event);
    let before = activate_hooks::calls();
    refuse_next_pin_of(h);
    click(rt.button_hwnd(h).unwrap());
    assert_eq!(
        pin_list_events(&seen),
        vec![
            PinEvent::Pinning(h),
            PinEvent::Refused(h),
            PinEvent::Recovered(h)
        ]
    );
    assert_eq!(
        activate_hooks::calls(),
        before + 1,
        "recovered pin not activated"
    );
    sweep(&rt);
    assert_eq!(
        activate_hooks::calls(),
        before + 1,
        "sweep activated a window"
    );
    drop(rt);
}

#[test]
fn pin_that_stays_refused_is_not_activated_and_the_later_settle_does_not_either() {
    let _g = crate::window_test_guard();
    let f = Fixture::new("activate-refused", 1);
    let h = f.w()[0];
    let rt = f.start();
    let before = activate_hooks::calls();
    let refusing = refuse_pin_of(h);
    click(rt.button_hwnd(h).unwrap());
    assert_eq!(activate_hooks::calls(), before, "refused pin was activated");

    drop(refusing);
    eventually_swept(&rt, "pin applied once the refusal ends", || is_topmost(h));
    assert_eq!(
        activate_hooks::calls(),
        before,
        "sweep activated the settled pin"
    );
    drop(rt);
}
