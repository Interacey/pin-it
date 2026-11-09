//! Releases that the target applies later (queued unpin) and releases left over from a crashed run.

use std::path::PathBuf;

use super::support::{Wins, click, eventually, options, sweep, temp_dir};
use crate::Hwnd;
use crate::app::crash_recovery::{SavedPin, window_identity};
use crate::app::host::with_app;
use crate::app::{RunOptions, Runtime};
use crate::config::Settings;
use crate::zorder::{defer_unpin_of, fail_unpin_of, is_topmost, pin, unpin};

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

    fn pins_text(&self) -> String {
        std::fs::read_to_string(self.pins_file()).unwrap_or_default()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn mentions(text: &str, h: Hwnd) -> bool {
    text.contains(&format!("\"hwnd\":{h},"))
}

#[test]
fn quick_repin_after_a_queued_unpin_is_not_remembered_as_topmost_before() {
    let _g = crate::window_test_guard();
    let f = Fixture::new("v2-async", 1);
    let h = f.wins.0[0];
    let rt = f.start();
    let queued = defer_unpin_of(h);
    click(rt.button_hwnd(h).unwrap());
    click(rt.button_hwnd(h).unwrap());
    assert!(rt.pinned().is_empty());
    assert!(
        is_topmost(h),
        "the queued unpin must not have been applied yet"
    );

    click(rt.button_hwnd(h).unwrap());
    assert_eq!(with_app(|a| a.prior_topmost.contains(&h)), Some(false));

    // The busy target now applies the queued request; the sweep pins the window again.
    drop(queued);
    unpin(h).unwrap();
    sweep(&rt);
    assert!(is_topmost(h) && rt.pinned() == vec![h]);

    click(rt.button_hwnd(h).unwrap());
    assert!(!is_topmost(h), "the window was left topmost after unpin");
}

#[test]
fn a_queued_unpin_that_the_target_applied_is_forgotten_by_the_sweep() {
    let _g = crate::window_test_guard();
    let f = Fixture::new("v2-async-sweep", 1);
    let h = f.wins.0[0];
    let rt = f.start();
    let queued = defer_unpin_of(h);
    click(rt.button_hwnd(h).unwrap());
    click(rt.button_hwnd(h).unwrap());
    assert_eq!(with_app(|a| a.queued_release.contains(&h)), Some(true));

    drop(queued);
    unpin(h).unwrap();
    sweep(&rt);
    assert_eq!(with_app(|a| a.queued_release.contains(&h)), Some(false));
}

#[test]
fn a_release_left_by_a_crashed_run_survives_the_first_write_and_is_retried() {
    let _g = crate::window_test_guard();
    let f = Fixture::new("v2-leftover", 2);
    let (stuck, other) = (f.wins.0[0], f.wins.0[1]);
    pin(stuck).unwrap();
    let (pid, created) = window_identity(stuck).unwrap();
    let saved = vec![SavedPin {
        hwnd: stuck,
        pid,
        created,
        was_topmost: false,
        pending_release: false,
    }];
    std::fs::write(f.pins_file(), serde_json::to_string(&saved).unwrap()).unwrap();

    let failing = fail_unpin_of(stuck);
    let rt = f.start();
    assert!(is_topmost(stuck), "the release was supposed to fail");
    click(rt.button_hwnd(other).unwrap());
    assert!(
        mentions(&f.pins_text(), stuck),
        "the first write dropped the leftover: {}",
        f.pins_text()
    );

    sweep(&rt);
    assert!(
        is_topmost(stuck),
        "still failing, must be retried not forgotten"
    );
    drop(failing);
    sweep(&rt);
    assert!(!is_topmost(stuck), "the sweep did not retry the leftover");
    assert!(!mentions(&f.pins_text(), stuck));
    drop(rt);
}
