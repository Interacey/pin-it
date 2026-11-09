//! Mouse input and animation timer of a real overlay window.

use windows::Win32::UI::WindowsAndMessaging::{WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE};

use super::ButtonStyle;
use super::test_support::{MSG_PIN, NOTIFIED, post, setup, teardown};

#[test]
fn click_inside_notifies_with_target() {
    let _guard = crate::window_test_guard();
    let (target, notify, b) = setup();
    let h = b.hwnd();
    post(h, WM_MOUSEMOVE, 16, 16);
    post(h, WM_LBUTTONDOWN, 16, 16);
    assert!(b.is_animating());
    post(h, WM_LBUTTONUP, 16, 16);
    let got = NOTIFIED.with(|n| n.borrow().clone());
    assert_eq!(got, vec![(target.0 as usize, MSG_PIN)]);
    teardown(target, notify, b);
}

#[test]
fn release_outside_does_not_notify() {
    let _guard = crate::window_test_guard();
    let (target, notify, b) = setup();
    let h = b.hwnd();
    post(h, WM_MOUSEMOVE, 16, 16);
    post(h, WM_LBUTTONDOWN, 16, 16);
    post(h, WM_LBUTTONUP, 200, 200);
    assert!(NOTIFIED.with(|n| n.borrow().is_empty()));
    // Release without a prior press does nothing either.
    post(h, WM_LBUTTONUP, 16, 16);
    assert!(NOTIFIED.with(|n| n.borrow().is_empty()));
    teardown(target, notify, b);
}

#[test]
fn timer_stops_after_click_animation() {
    let _guard = crate::window_test_guard();
    let (target, notify, mut b) = setup();
    let h = b.hwnd();
    post(h, WM_LBUTTONDOWN, 10, 10);
    assert!(b.tick(1.0 / 60.0));
    post(h, WM_LBUTTONUP, 10, 10);
    assert!(b.is_animating());
    let mut frames = 0;
    while b.tick(1.0 / 60.0) {
        frames += 1;
        assert!(frames < 300, "animation never settled");
    }
    assert!(!b.is_animating());
    teardown(target, notify, b);
}

#[test]
fn animate_off_needs_no_timer() {
    let _guard = crate::window_test_guard();
    let (target, notify, mut b) = setup();
    b.set_style(ButtonStyle {
        animate: false,
        pinned: true,
        ..Default::default()
    });
    assert!(!b.is_animating());
    let h = b.hwnd();
    post(h, WM_LBUTTONDOWN, 10, 10);
    post(h, WM_LBUTTONUP, 10, 10);
    assert!(!b.is_animating());
    teardown(target, notify, b);
}

#[test]
fn first_frame_already_shows_the_press_and_the_release() {
    let _guard = crate::window_test_guard();
    let (target, notify, mut b) = setup();
    let h = b.hwnd();
    post(h, WM_MOUSEMOVE, 16, 16);
    assert_eq!(b.st().anim.scale(), 1.0);
    post(h, WM_LBUTTONDOWN, 16, 16);
    let pressed = b.st().anim.scale();
    assert!(
        pressed < 0.999,
        "no motion in the handler's frame: {pressed}"
    );
    // Let it settle down, then the release must move at once as well.
    for _ in 0..30 {
        b.tick(0.016);
    }
    let down = b.st().anim.scale();
    post(h, WM_LBUTTONUP, 16, 16);
    assert!(b.st().anim.scale() > down, "no motion on release");
    teardown(target, notify, b);
}

/// Time spent in the mouse handlers (render and present included); printed, not asserted.
/// Run with `cargo test --release handler_time -- --nocapture`.
#[test]
fn handler_time() {
    use std::time::Instant;
    use windows::Win32::UI::WindowsAndMessaging::SendMessageW;
    let _guard = crate::window_test_guard();
    let (target, notify, mut b) = setup();
    let hwnd = crate::win32::to_hwnd(b.hwnd());
    let send = |msg: u32| {
        let start = Instant::now();
        unsafe {
            SendMessageW(
                hwnd,
                msg,
                None,
                Some(windows::Win32::Foundation::LPARAM(16 << 16 | 16)),
            )
        };
        start.elapsed().as_secs_f64() * 1000.0
    };
    for size in [20u32, 32, 48] {
        b.set_slot(0, 0, size);
        let (mut down, mut up) = (Vec::new(), Vec::new());
        for _ in 0..200 {
            down.push(send(WM_LBUTTONDOWN));
            up.push(send(WM_LBUTTONUP));
            for _ in 0..40 {
                b.tick(0.016);
            }
        }
        for (name, v) in [("down", &mut down), ("up", &mut up)] {
            v.sort_by(|a, b| a.total_cmp(b));
            println!(
                "size {size} {name}: median {:.3} ms, p95 {:.3} ms, max {:.3} ms",
                v[v.len() / 2],
                v[v.len() * 95 / 100],
                v[v.len() - 1]
            );
        }
    }
    teardown(target, notify, b);
}
