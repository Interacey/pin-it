use super::*;

const FRAME: f32 = 1.0 / 60.0;

fn run(a: &mut ButtonAnim, secs: f32) {
    let mut t = 0.0;
    while t < secs {
        a.step(FRAME);
        t += FRAME;
    }
}

/// Highest scale and the time it was settled again, stepping `dt` at a time for up to `secs`.
fn release_curve(mut a: ButtonAnim, dt: f32, secs: f32) -> (f32, f32) {
    a.on_release();
    let (mut peak, mut t, mut settled_at) = (f32::MIN, 0.0, secs);
    while t < secs {
        a.step(dt);
        t += dt;
        peak = peak.max(a.scale());
        if !a.is_active() {
            settled_at = t;
            break;
        }
    }
    (peak, settled_at)
}

fn pressed_down() -> ButtonAnim {
    let mut a = ButtonAnim::new(false);
    a.on_press();
    run(&mut a, 0.5);
    a
}

#[test]
fn fill_goes_up_in_150ms_with_ease_out() {
    let mut a = ButtonAnim::new(false);
    assert_eq!(a.fill(), 0.0);
    a.set_pinned(true);
    assert!(a.is_active());
    a.step(0.075);
    assert!(a.fill() > 0.5, "ease-out is past linear at half time");
    a.step(0.074);
    assert!(a.fill() < 1.0);
    a.step(0.002);
    assert_eq!(a.fill(), 1.0);
    assert!(!a.is_active());
}

#[test]
fn fill_reverses_when_unpinned() {
    let mut a = ButtonAnim::new(false);
    a.set_pinned(true);
    run(&mut a, 0.3);
    a.set_pinned(false);
    assert!(a.is_active());
    a.step(0.05);
    assert!(a.fill() > 0.0 && a.fill() < 1.0);
    run(&mut a, 0.3);
    assert_eq!(a.fill(), 0.0);
    assert!(!a.is_active());
}

#[test]
fn press_goes_down_to_088_without_undershoot() {
    let mut a = ButtonAnim::new(false);
    assert_eq!(a.scale(), 1.0);
    a.on_press();
    assert!(a.is_active());
    let mut lowest = 1.0f32;
    for _ in 0..40 {
        a.step(FRAME);
        lowest = lowest.min(a.scale());
    }
    assert!((a.scale() - PRESS_SCALE).abs() < 0.005, "{}", a.scale());
    assert!(
        lowest >= PRESS_SCALE - 0.01,
        "pressing undershoots: {lowest}"
    );
    assert!(!a.is_active());
}

#[test]
fn release_overshoots_once_by_three_to_eight_percent_and_settles() {
    let (peak, settled_at) = release_curve(pressed_down(), FRAME, 1.0);
    assert!((1.03..=1.08).contains(&peak), "peak {peak}");
    assert!(settled_at <= 0.4, "settled after {settled_at} s");
}

#[test]
fn release_settles_exactly_on_one_and_stops() {
    let mut a = pressed_down();
    a.on_release();
    run(&mut a, 0.6);
    assert_eq!(a.scale(), 1.0);
    assert!(!a.is_active());
}

#[test]
fn spring_is_stable_at_large_steps() {
    for dt in [0.016, 0.05, 0.2, 1.0, 5.0] {
        let (peak, settled_at) = release_curve(pressed_down(), dt, 20.0);
        assert!(peak <= 1.09, "dt {dt}: peak {peak}");
        assert!(settled_at < 20.0, "dt {dt}: never settled");
    }
}

#[test]
fn spring_is_not_sped_up_by_big_steps() {
    // One 50 ms step must land where fifty 1 ms steps do, within the integrator's error.
    let mut coarse = pressed_down();
    let mut fine = coarse.clone();
    coarse.on_release();
    fine.on_release();
    coarse.step(0.05);
    for _ in 0..50 {
        fine.step(0.001);
    }
    assert!((coarse.scale() - fine.scale()).abs() < 0.01);
}

#[test]
fn quick_tap_still_returns_to_one() {
    let mut a = ButtonAnim::new(false);
    a.on_press();
    a.step(0.03);
    assert!(a.scale() < 1.0);
    a.on_release();
    run(&mut a, 0.6);
    assert_eq!(a.scale(), 1.0);
    assert!(!a.is_active());
}

#[test]
fn hover_eases() {
    let mut a = ButtonAnim::new(false);
    a.on_hover(true);
    assert_eq!(a.hover(), 0.0);
    a.step(0.06);
    assert!(a.hover() > 0.0 && a.hover() < 1.0);
    run(&mut a, 0.2);
    assert_eq!(a.hover(), 1.0);
    a.on_hover(false);
    run(&mut a, 0.2);
    assert_eq!(a.hover(), 0.0);
}

#[test]
fn bad_dt_is_ignored() {
    let mut a = ButtonAnim::new(false);
    a.set_pinned(true);
    a.step(0.0);
    a.step(-1.0);
    a.step(f32::NAN);
    assert_eq!(a.fill(), 0.0);
}

#[test]
fn reduced_is_instant() {
    let mut a = ButtonAnim::new(true);
    a.on_hover(true);
    a.set_pinned(true);
    a.on_press();
    assert_eq!(a.hover(), 1.0);
    assert_eq!(a.fill(), 1.0);
    assert_eq!(a.scale(), 1.0);
    assert!(!a.is_active());
    a.on_release();
    assert_eq!(a.scale(), 1.0);
    assert!(!a.is_active());
}

#[test]
fn set_reduced_settles_running_animation() {
    let mut a = ButtonAnim::new(false);
    a.on_press();
    a.on_hover(true);
    a.set_pinned(true);
    a.step(0.02);
    a.set_reduced(true);
    assert!(!a.is_active());
    assert_eq!(a.fill(), 1.0);
    assert_eq!(a.scale(), 1.0);
}

#[test]
fn is_active_is_false_when_idle_and_after_settling() {
    let mut a = ButtonAnim::new(false);
    assert!(!a.is_active());
    a.on_press();
    assert!(a.is_active());
    run(&mut a, 0.5);
    assert!(!a.is_active(), "held at 0.88 is at rest");
    a.on_release();
    assert!(a.is_active());
    run(&mut a, 0.6);
    assert!(!a.is_active());
}
