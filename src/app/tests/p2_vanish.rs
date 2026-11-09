//! The pin button must not vanish when the user presses, activates, moves or resizes a healthy
//! window. Own test windows only; the system events that need real input are injected.

use std::time::Instant;

use windows::Win32::UI::WindowsAndMessaging::{
    HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
};
use windows::Win32::{Foundation::RECT, UI::WindowsAndMessaging::GetWindowRect};

use super::support::{
    ForegroundOverride, Wins, button_visible, log_text, options, overlay_directly_above, pump_ms,
    temp_dir,
};
use crate::Hwnd;
use crate::app::Runtime;
use crate::app::host::with_app;
use crate::config::Settings;
use crate::events::WinEvent;
use crate::win32::to_hwnd;

/// Raises `h` like the activation or the press on the title bar of a window does, without any event
/// reaching the app.
fn raise(h: Hwnd) {
    // SAFETY: own test window.
    unsafe {
        SetWindowPos(
            to_hwnd(h),
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
        .unwrap();
    }
}

/// Whether the app considers the button of `h` shown.
fn entry_shown(h: Hwnd) -> bool {
    with_app(|a| a.entries.get(&h).is_some_and(|e| e.shown)).unwrap_or(false)
}

fn window_rect(h: Hwnd) -> RECT {
    let mut r = RECT::default();
    // SAFETY: own test window.
    unsafe { GetWindowRect(to_hwnd(h), &mut r) }.unwrap();
    r
}

#[test]
fn a_raise_of_the_foreground_window_without_any_event_does_not_leave_its_button_buried() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let w = wins.0.clone();
    let _fg = ForegroundOverride::set(w[0]);
    let dir = temp_dir("p2-watch");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(100);
    let overlay = rt.button_hwnd(w[0]).unwrap();
    assert!(overlay_directly_above(overlay, w[0]), "start state");

    // A press on the title bar of the window the user works in: the system raises it over the overlay,
    // and since it is the foreground window already, no foreground event follows.
    raise(w[0]);
    assert!(
        !overlay_directly_above(overlay, w[0]),
        "setup: the raise buries the overlay"
    );
    pump_ms(150);
    assert!(
        overlay_directly_above(overlay, w[0]),
        "the overlay stays buried until the next sweep"
    );
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_late_raise_after_the_foreground_event_is_corrected_within_a_blink() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    let dir = temp_dir("p2-blink");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(100);
    let overlay = rt.button_hwnd(w[0]).unwrap();

    rt.inject_event(WinEvent::Foreground(w[0]));
    raise(w[0]);
    assert!(!overlay_directly_above(overlay, w[0]), "setup");
    pump_ms(80);
    assert!(
        overlay_directly_above(overlay, w[0]),
        "still buried 80 ms after the activation"
    );
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn activation_moves_and_resizes_never_hide_a_healthy_windows_button() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    let dir = temp_dir("p2-storm");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(150);
    let target = w[0];
    let check = |step: &str| {
        assert!(entry_shown(target), "hidden after {step}");
        assert!(button_visible(&rt, target), "overlay hidden after {step}");
    };
    check("start");

    let r = window_rect(target);
    let (width, height) = (r.right - r.left, r.bottom - r.top);
    rt.inject_event(WinEvent::Foreground(target));
    check("foreground");
    rt.inject_event(WinEvent::MoveSizeStart(target));
    check("move-size start");
    for i in 0..40 {
        // A drag that also shrinks the window a little at the end.
        let shrink = if i > 30 { (i - 30) * 4 } else { 0 };
        // SAFETY: own test window.
        unsafe {
            SetWindowPos(
                to_hwnd(target),
                None,
                r.left - i * 3,
                r.top - i,
                width - shrink,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
            .unwrap();
        }
        rt.inject_event(WinEvent::Moved(target));
        check(&format!("move {i}"));
        if i % 10 == 0 {
            rt.inject_event(WinEvent::Foreground(w[1]));
            check("focus lost");
            rt.inject_event(WinEvent::Foreground(target));
            check("focus back");
        }
    }
    rt.inject_event(WinEvent::MoveSizeEnd(target));
    check("move-size end");
    pump_ms(400);
    check("rescan after the loop");
    rt.inject_event(WinEvent::Cloaked(target));
    rt.inject_event(WinEvent::Uncloaked(target));
    check("cloak flicker");

    let log = log_text(&dir);
    assert!(
        !log.contains("button hidden"),
        "a healthy window lost its button:\n{log}"
    );
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

/// CPU time (kernel + user) the calling thread has used, in 100 ns units.
fn thread_cpu_100ns() -> u64 {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: the pseudo handle of the calling thread and four valid out parameters.
    unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    }
    .unwrap();
    let as_u64 = |t: FILETIME| u64::from(t.dwHighDateTime) << 32 | u64::from(t.dwLowDateTime);
    as_u64(kernel) + as_u64(user)
}

#[test]
fn only_the_foreground_windows_overlay_runs_the_order_watch_and_it_costs_next_to_nothing() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    let _fg = ForegroundOverride::set(w[0]);
    let dir = temp_dir("p2-watch-cpu");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(150);
    let watching =
        |h: Hwnd| with_app(|a| a.entries.get_mut(&h).map(|e| e.button.watches_order())).flatten();
    assert_eq!(watching(w[0]), Some(true), "foreground window");
    assert_eq!(watching(w[1]), Some(false), "other window");

    let (cpu_before, wall) = (thread_cpu_100ns(), Instant::now());
    pump_ms(3000);
    let cpu_ms = (thread_cpu_100ns() - cpu_before) / 10_000;
    let wall_ms = wall.elapsed().as_millis();
    println!("order watch idle: {cpu_ms} ms CPU in {wall_ms} ms");
    assert!(
        cpu_ms * 20 < wall_ms as u64,
        "{cpu_ms} ms CPU in {wall_ms} ms is over 5%"
    );

    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}
