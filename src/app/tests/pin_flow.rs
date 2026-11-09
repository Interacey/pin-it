//! Pin, unpin, order and button flows against the app's own test windows.

use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, HWND_TOP, IsWindow, IsWindowVisible, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, ShowWindow,
};

use super::support::{
    Wins, button_visible, click, dump, eventually, eventually_swept, options,
    overlay_directly_above, pos, post, pump_ms, sweep, temp_dir,
};
use crate::Hwnd;
use crate::app::Runtime;
use crate::app::host::{WM_PIN_CLICK, WM_SWEEP_NOW};
use crate::config::Settings;
use crate::events::WinEvent;
use crate::win32::to_hwnd;
use crate::zorder::{is_topmost, pin};

#[test]
fn pin_stack_order_overlays_and_exit() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(3);
    let w = wins.0.clone();
    let dir = temp_dir("flow");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    eventually("three buttons", || rt.button_count() == 3);
    let ov: Vec<Hwnd> = w.iter().map(|&t| rt.button_hwnd(t).unwrap()).collect();
    for &o in &ov {
        assert!(
            unsafe { IsWindowVisible(to_hwnd(o)) }.as_bool(),
            "button hidden"
        );
    }

    // Pin A, B, C in that order: C ends on top.
    for &o in &ov {
        click(o);
    }
    dump("after pinning A,B,C", &w, &ov);
    assert_eq!(rt.pinned(), w);
    eventually_swept(&rt, "A, B, C topmost with C on top", || {
        w.iter().all(|&h| is_topmost(h)) && pos(w[2]) < pos(w[1]) && pos(w[1]) < pos(w[0])
    });
    for i in 0..3 {
        assert!(
            overlay_directly_above(ov[i], w[i]),
            "overlay {i} not above its target"
        );
    }

    // Raise A above the others; the sweep (same code as the Foreground event) restores the order.
    unsafe {
        SetWindowPos(
            to_hwnd(w[0]),
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
        .unwrap();
    }
    dump("after raising A", &w, &ov);
    assert!(
        pos(w[0]) < pos(w[2]),
        "raising A had no effect, test is void"
    );
    eventually_swept(&rt, "sweep restores C > B > A", || {
        pos(w[2]) < pos(w[1]) && pos(w[1]) < pos(w[0])
    });
    dump("after sweep", &w, &ov);
    for i in 0..3 {
        assert!(overlay_directly_above(ov[i], w[i]));
    }

    // Unpin B: B drops out of the topmost band, C stays above A.
    click(ov[1]);
    dump("after unpinning B", &w, &ov);
    assert_eq!(rt.pinned(), vec![w[0], w[2]]);
    assert!(!is_topmost(w[1]) && is_topmost(w[0]) && is_topmost(w[2]));
    assert!(pos(w[2]) < pos(w[0]) && pos(w[0]) < pos(w[1]));
    assert!(overlay_directly_above(ov[1], w[1]));
    assert!(overlay_directly_above(ov[0], w[0]));

    // Exit: everything unpinned, buttons gone.
    drop(rt);
    assert!(w.iter().all(|&h| !is_topmost(h)));
    eventually("overlays destroyed", || {
        ov.iter()
            .all(|&o| !unsafe { IsWindow(Some(to_hwnd(o))) }.as_bool())
    });
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn destroyed_window_is_dropped_from_state() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    let dir = temp_dir("destroy");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(100);
    click(rt.button_hwnd(w[0]).unwrap());
    click(rt.button_hwnd(w[1]).unwrap());
    assert_eq!(rt.pinned(), w);
    unsafe { DestroyWindow(to_hwnd(w[0])).unwrap() };
    pump_ms(100);
    post(rt.hwnd(), WM_SWEEP_NOW, 0, 0);
    pump_ms(100);
    assert_eq!(rt.button_count(), 1);
    assert_eq!(rt.pinned(), vec![w[1]]);
    // The Destroyed event path. The real hook cannot be tested here: it skips this process's own
    // windows, so the event is injected.
    rt.inject_event(WinEvent::Destroyed(w[1]));
    assert_eq!(rt.button_count(), 0);
    assert!(rt.pinned().is_empty());
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

/// Pins three windows by plain `SetWindowPos(HWND_TOPMOST)`, with or without overlays,
/// and returns whether C ended above B above A.
fn raw_topmost_order(with_overlays: bool, passes: usize) -> bool {
    use crate::overlay::PinButton;
    use windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST;
    let wins = Wins::new(3);
    let w = wins.0.clone();
    let buttons: Vec<PinButton> = if with_overlays {
        w.iter()
            .map(|&t| {
                let mut b = PinButton::new(t, 0, 0).unwrap();
                b.set_slot(0, 0, 20);
                b.show();
                b
            })
            .collect()
    } else {
        Vec::new()
    };
    pump_ms(30);
    for _ in 0..passes {
        for &h in &w {
            unsafe {
                SetWindowPos(
                    to_hwnd(h),
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                )
                .unwrap();
            }
        }
    }
    let ok = pos(w[2]) < pos(w[1]) && pos(w[1]) < pos(w[0]);
    drop(buttons);
    drop(wins);
    ok
}

#[test]
fn second_topmost_pass_gives_requested_order_with_overlays() {
    let _g = crate::window_test_guard();
    let (mut plain, mut once, mut twice) = (0, 0, 0);
    for _ in 0..10 {
        plain += usize::from(raw_topmost_order(false, 1));
        once += usize::from(raw_topmost_order(true, 1));
        twice += usize::from(raw_topmost_order(true, 2));
    }
    eprintln!(
        "C>B>A, 10 runs: no overlays 1 pass {plain}, overlays 1 pass {once}, overlays 2 passes {twice}"
    );
    // Re-applying HWND_TOPMOST oldest first is what `zorder::apply_order` relies on. `once` is
    // only reported, not asserted.
    assert_eq!(plain, 10);
    assert_eq!(twice, 10);
}

#[test]
fn pinned_window_keeps_its_pin_while_it_is_not_eligible() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    let dir = temp_dir("keep");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(100);
    click(rt.button_hwnd(w[0]).unwrap());
    assert_eq!(rt.pinned(), vec![w[0]]);

    // Hidden stands in for cloaked / no caption: the window is not eligible for a button.
    unsafe {
        let _ = ShowWindow(to_hwnd(w[0]), SW_HIDE);
        let _ = ShowWindow(to_hwnd(w[1]), SW_HIDE);
    }
    sweep(&rt);
    assert_eq!(rt.pinned(), vec![w[0]], "pin lost while hidden");
    eventually_swept(&rt, "w[0] topmost", || is_topmost(w[0]));
    assert!(
        !button_visible(&rt, w[0]),
        "button of a hidden window is shown"
    );
    assert!(
        rt.button_hwnd(w[1]).is_none(),
        "unpinned hidden window kept its entry"
    );

    rt.inject_event(WinEvent::Hidden(w[0]));
    assert_eq!(rt.pinned(), vec![w[0]], "Hidden event dropped the pin");

    unsafe {
        let _ = ShowWindow(to_hwnd(w[0]), SW_SHOWNA);
        let _ = ShowWindow(to_hwnd(w[1]), SW_SHOWNA);
    }
    sweep(&rt);
    assert_eq!(rt.pinned(), vec![w[0]]);
    assert!(button_visible(&rt, w[0]), "button did not come back");
    eventually_swept(&rt, "w[0] topmost", || is_topmost(w[0]));
    drop(rt);
    assert!(!is_topmost(w[0]));
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn restored_window_without_entry_gets_a_button_at_once() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(1);
    let w = wins.0.clone();
    let dir = temp_dir("restored");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    unsafe {
        let _ = ShowWindow(to_hwnd(w[0]), SW_HIDE);
    }
    sweep(&rt);
    assert_eq!(rt.button_count(), 0);
    unsafe {
        let _ = ShowWindow(to_hwnd(w[0]), SW_SHOWNA);
    }
    rt.inject_event(WinEvent::Restored(w[0])); // no sweep in between
    assert_eq!(rt.button_count(), 1);
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn window_that_was_topmost_stays_topmost_after_unpin_and_exit() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    pin(w[0]).unwrap(); // its own "always on top"
    let dir = temp_dir("prior");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(100);
    click(rt.button_hwnd(w[0]).unwrap());
    click(rt.button_hwnd(w[1]).unwrap());
    assert_eq!(rt.pinned(), w);
    click(rt.button_hwnd(w[0]).unwrap());
    assert_eq!(rt.pinned(), vec![w[1]]);
    assert!(
        is_topmost(w[0]),
        "unpin removed a topmost flag that was not ours"
    );
    click(rt.button_hwnd(w[0]).unwrap());
    drop(rt);
    assert!(
        is_topmost(w[0]),
        "exit removed a topmost flag that was not ours"
    );
    assert!(!is_topmost(w[1]), "exit left our own pin");
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn spoofed_pin_click_is_ignored() {
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    let dir = temp_dir("spoof");
    // Only w[0] gets a button; w[1] is a valid window that was never offered.
    let rt = Runtime::start(&options(&dir, vec![w[0]], &Settings::default())).unwrap();
    pump_ms(100);
    post(rt.hwnd(), WM_PIN_CLICK, 0x1234_5678, 0);
    post(rt.hwnd(), WM_PIN_CLICK, w[1] as usize, 0);
    pump_ms(100);
    assert!(rt.pinned().is_empty(), "spoofed click pinned something");
    assert_eq!(rt.button_count(), 1, "spoofed click created an entry");
    assert!(!is_topmost(w[1]));
    post(rt.hwnd(), WM_PIN_CLICK, w[0] as usize, 0);
    pump_ms(100);
    assert_eq!(rt.pinned(), vec![w[0]]);
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn pinned_entry_that_lost_its_topmost_flag_is_pinned_again_by_the_sweep() {
    use windows::Win32::UI::WindowsAndMessaging::HWND_NOTOPMOST;
    let _g = crate::window_test_guard();
    let wins = Wins::new(2);
    let w = wins.0.clone();
    let dir = temp_dir("lost-topmost");
    let rt = Runtime::start(&options(&dir, w.clone(), &Settings::default())).unwrap();
    pump_ms(100);
    click(rt.button_hwnd(w[0]).unwrap());
    click(rt.button_hwnd(w[1]).unwrap());
    eventually_swept(&rt, "both topmost", || is_topmost(w[0]) && is_topmost(w[1]));
    // As if the system had refused or undone the topmost flag of the older pin.
    unsafe {
        SetWindowPos(
            to_hwnd(w[0]),
            Some(HWND_NOTOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )
        .unwrap();
    }
    assert!(!is_topmost(w[0]), "test is void: flag was not cleared");
    eventually_swept(&rt, "sweep pins the window again in order", || {
        is_topmost(w[0]) && is_topmost(w[1]) && pos(w[1]) < pos(w[0])
    });
    assert_eq!(rt.pinned(), w, "pin entry was dropped");
    drop(rt);
    drop(wins);
    let _ = std::fs::remove_dir_all(dir);
}
