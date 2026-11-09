//! Crash recovery through pins.json and the panic hook.

use super::support::{Wins, click, eventually_swept, options, pump_ms, temp_dir};
use crate::app::{Runtime, release_pinned_after_panic};
use crate::config::Settings;
use crate::zorder::{is_topmost, pin};

#[test]
fn next_start_clears_pins_left_by_a_crashed_run() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    pin(w[1]).unwrap(); // topmost on its own
    let dir = temp_dir("crash");
    let opts = options(&dir, w.clone(), &Settings::default());
    let pins_file = dir.join("pins.json");
    let rt = Runtime::start(&opts).unwrap();
    pump_ms(100);
    click(rt.button_hwnd(w[0]).unwrap());
    click(rt.button_hwnd(w[1]).unwrap());
    assert!(pins_file.exists(), "pins.json was not written");
    let saved = std::fs::read_to_string(&pins_file).unwrap();
    rt.crash();
    pump_ms(50);
    assert!(
        is_topmost(w[0]) && is_topmost(w[1]),
        "crash simulation released the pins"
    );
    assert!(pins_file.exists());

    // A mismatching creation time must be left alone.
    let tampered = saved.replace("\"created\":", "\"created\":1");
    assert_ne!(tampered, saved);
    std::fs::write(&pins_file, &tampered).unwrap();
    let rt = Runtime::start(&opts).unwrap();
    assert!(is_topmost(w[0]), "acted on a pin of a different process");
    assert!(!pins_file.exists());
    rt.crash();

    // The real file: w[0] was ours and is cleared; w[1] was topmost before and stays so.
    std::fs::write(&pins_file, &saved).unwrap();
    let rt = Runtime::start(&opts).unwrap();
    assert!(!is_topmost(w[0]), "stale pin was not cleared");
    assert!(
        is_topmost(w[1]),
        "a window that was topmost before was unpinned"
    );
    assert!(!pins_file.exists());
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn panic_release_unpins_the_pinned_windows() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let w = wins.0.clone();
    let dir = temp_dir("panic");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(100);
    click(rt.button_hwnd(w[0]).unwrap());
    eventually_swept(&rt, "w[0] topmost", || is_topmost(w[0]));
    // The hook runs on the panicking thread, whose windows would block the worker; here the
    // test windows live on this thread, so the hook runs elsewhere while this thread pumps.
    let hook = std::thread::spawn(release_pinned_after_panic);
    while !hook.is_finished() {
        pump_ms(10);
    }
    hook.join().unwrap();
    assert!(!is_topmost(w[0]));
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}
