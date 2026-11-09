//! Z-order of real overlay windows relative to their targets.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, GW_HWNDPREV, GetWindow, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST, IsWindowVisible,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
};

use super::PinButton;
use super::stacking::is_topmost;
use super::test_support::{MSG_PIN, corner_window, own_snapshot, pump, setup, teardown, z_index};
use crate::win32::to_hwnd;

const KEEP: windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS =
    windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS(
        SWP_NOACTIVATE.0 | SWP_NOMOVE.0 | SWP_NOSIZE.0,
    );

fn place(h: HWND, after: HWND) {
    unsafe { SetWindowPos(h, Some(after), 0, 0, 0, 0, KEEP).unwrap() };
}

/// Makes `h` topmost; the system sometimes ignores the request for a while (seen about
/// 1 in 15 runs on the dev machine), so retry briefly. False when it never worked.
fn place_topmost(h: HWND) -> bool {
    for _ in 0..20 {
        place(h, HWND_TOPMOST);
        if is_topmost(h) {
            return true;
        }
        pump();
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    eprintln!("system refused HWND_TOPMOST for {h:?}; topmost part of the test skipped");
    false
}

fn prev_of(h: HWND) -> Option<HWND> {
    unsafe { GetWindow(h, GW_HWNDPREV).ok() }
}

/// The overlay is the immediate predecessor of its target, in the real z-order and in our snapshot.
fn assert_adjacent(ov: HWND, target: HWND, why: &str) {
    assert_eq!(prev_of(target), Some(ov), "{why}");
    assert_eq!(z_index(ov) + 1, z_index(target), "{why}");
    assert_eq!(is_topmost(ov), is_topmost(target), "{why}: topmost flag");
}

#[test]
fn restack_tracks_target_through_topmost_and_other_windows() {
    let _guard = crate::window_test_guard();
    let (target, notify, mut b) = setup();
    let ov = to_hwnd(b.hwnd());
    b.restack();
    assert_adjacent(ov, target, "initial");
    // Idempotent: a second call keeps the order.
    let before = own_snapshot();
    b.restack();
    assert_eq!(before, own_snapshot());

    if !place_topmost(target) {
        teardown(target, notify, b);
        return;
    }
    b.restack();
    assert!(is_topmost(target) && is_topmost(ov));
    assert_adjacent(ov, target, "target topmost");

    // Another own window raised above the target lands between overlay and target's neighbours.
    let other = corner_window(40);
    let other_top = place_topmost(other);
    b.restack();
    assert_adjacent(ov, target, &format!("other raised (topmost={other_top})"));

    // Target back to non-topmost: overlay must drop out of the topmost band too.
    place(target, HWND_NOTOPMOST);
    place(other, HWND_NOTOPMOST);
    place(other, HWND_TOP);
    b.restack();
    assert!(!is_topmost(target) && !is_topmost(ov));
    assert_adjacent(ov, target, "target non-topmost, other raised above");
    unsafe {
        let _ = DestroyWindow(other);
    }
    teardown(target, notify, b);
}

#[test]
fn restack_with_two_targets_and_two_overlays() {
    let _guard = crate::window_test_guard();
    let (a, notify, mut ba) = setup();
    let b = corner_window(10);
    let mut bb = PinButton::new(b.0 as isize, notify.0 as isize, MSG_PIN).unwrap();
    bb.set_slot(0, 0, 20);
    bb.show();
    let (oa, ob) = (to_hwnd(ba.hwnd()), to_hwnd(bb.hwnd()));
    for round in 0..3 {
        // Scramble: raise A over everything, then B's overlay over A.
        place(a, HWND_TOP);
        place(ob, HWND_TOP);
        ba.restack();
        bb.restack();
        // One more pass settles the case where the window above a target was the other overlay.
        ba.restack();
        bb.restack();
        assert_adjacent(oa, a, &format!("round {round} A"));
        assert_adjacent(ob, b, &format!("round {round} B"));
        if round == 1 && !(place_topmost(a) && place_topmost(b)) {
            break;
        }
    }
    drop(bb);
    unsafe {
        let _ = DestroyWindow(b);
    }
    teardown(a, notify, ba);
}

#[test]
fn restack_hides_overlay_when_target_is_gone() {
    let _guard = crate::window_test_guard();
    let (target, notify, mut b) = setup();
    let ov = to_hwnd(b.hwnd());
    assert!(unsafe { IsWindowVisible(ov) }.as_bool());
    unsafe {
        let _ = DestroyWindow(target);
    }
    pump();
    b.restack();
    assert!(!unsafe { IsWindowVisible(ov) }.as_bool());
    b.restack(); // second call on a hidden overlay is a no-op
    assert_ne!(b.hwnd(), 0);
    drop(b);
    unsafe {
        let _ = DestroyWindow(notify);
    }
}

/// A non-topmost target that is the highest of its band has a topmost window above it (the
/// pinned one). The overlay must still stay out of the topmost band and sit directly above it.
#[test]
fn restack_below_a_pinned_window_keeps_overlay_out_of_the_topmost_band() {
    let _guard = crate::window_test_guard();
    let (pinned, notify, mut pinned_button) = setup();
    let plain = corner_window(60);
    let mut plain_button = PinButton::new(plain.0 as isize, notify.0 as isize, MSG_PIN).unwrap();
    plain_button.set_slot(0, 0, 20);
    plain_button.show();
    let (pinned_ov, plain_ov) = (to_hwnd(pinned_button.hwnd()), to_hwnd(plain_button.hwnd()));
    if crate::zorder::pin(pinned.0 as isize).is_err() || !place_topmost(pinned) {
        teardown(pinned, notify, pinned_button);
        return;
    }
    pinned_button.restack();
    // The plain window is the highest non-topmost window: its neighbour above is topmost.
    place(plain, HWND_TOP);
    // Like `place_topmost`: a system that ignores the raise (seen while a foreign window is in
    // front) leaves nothing to check.
    if !prev_of(plain).is_some_and(is_topmost) {
        eprintln!("system ignored HWND_TOP for {plain:?}; part of the test skipped");
        drop(plain_button);
        unsafe {
            let _ = DestroyWindow(plain);
        }
        teardown(pinned, notify, pinned_button);
        return;
    }
    for round in 0..3 {
        plain_button.restack();
        pinned_button.restack();
        assert!(
            !is_topmost(plain_ov),
            "round {round}: overlay of the plain window topmost"
        );
        assert!(is_topmost(pinned_ov) && is_topmost(pinned));
        assert_adjacent(plain_ov, plain, &format!("round {round}"));
        assert!(
            z_index(pinned) < z_index(plain_ov),
            "round {round}: pinned window below the plain overlay"
        );
    }
    let settled = own_snapshot();
    plain_button.restack();
    assert_eq!(settled, own_snapshot(), "restack is idempotent");
    drop(plain_button);
    unsafe {
        let _ = DestroyWindow(plain);
    }
    teardown(pinned, notify, pinned_button);
}

/// How long the overlay stays below its target after the target was raised over it, in ms.
/// Polls without sleeping, so the resolution is a few microseconds.
fn buried_ms(target: HWND, overlay: HWND) -> f64 {
    place(target, HWND_TOP);
    let start = std::time::Instant::now();
    while prev_of(target) != Some(overlay) && start.elapsed().as_millis() < 200 {
        pump();
    }
    start.elapsed().as_secs_f64() * 1000.0
}

fn settle(ms: u64) {
    let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < end {
        pump();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// Buried time with the 10 ms watch alone and with the pulse that an activation starts.
/// The pulse also stops again once the burst is over and no mouse button is down.
/// Timing depends on the load of the machine, so it runs on request:
/// `cargo test --release pulse_restacks -- --ignored --nocapture`.
#[test]
#[ignore]
fn pulse_restacks_a_raised_target_within_a_few_milliseconds() {
    let _guard = crate::window_test_guard();
    let (target, notify, mut b) = setup();
    let ov = to_hwnd(b.hwnd());
    b.watch_order(true);
    b.restack();
    let (mut watch_only, mut with_pulse) = (Vec::new(), Vec::new());
    for i in 0..30u64 {
        // Varies the phase against the 10 ms watch timer from trial to trial.
        settle(130 + i * 3 % 10);
        b.restack();
        watch_only.push(buried_ms(target, ov));
        settle(130);
        b.restack();
        b.restack_soon();
        with_pulse.push(buried_ms(target, ov));
    }
    println!(
        "buried: watch only median {:.2} ms max {:.2} ms; with pulse median {:.2} ms max {:.2} ms",
        median(watch_only.clone()),
        watch_only.iter().copied().fold(0.0, f64::max),
        median(with_pulse.clone()),
        with_pulse.iter().copied().fold(0.0, f64::max)
    );
    settle(400);
    assert!(
        b.st().pulse.is_none() || super::pulse::left_button_down(),
        "pulse still running"
    );
    with_pulse.sort_by(|a, b| a.total_cmp(b));
    let p75 = with_pulse[with_pulse.len() * 3 / 4];
    assert!(p75 < 5.0, "pulse too slow: p75 {p75:.2} ms");
    teardown(target, notify, b);
}

#[test]
fn pulse_runs_only_for_the_watched_overlay_and_dies_with_it() {
    let _guard = crate::window_test_guard();
    let (target, notify, mut b) = setup();
    b.restack_soon();
    assert!(
        b.st().pulse.is_none(),
        "an unwatched overlay needs no pulse"
    );
    b.watch_order(true);
    b.restack_soon();
    assert!(
        b.st().pulse.is_some(),
        "the foreground overlay pulses after an activation"
    );
    b.watch_order(false);
    assert!(
        b.st().pulse.is_none(),
        "leaving the foreground ends the pulse"
    );
    b.watch_order(true);
    b.restack_soon();
    assert!(b.st().pulse.is_some());
    // Dropped while the timer runs: its last callback must not touch freed memory.
    teardown(target, notify, b);
    pump();
}
