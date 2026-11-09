use super::*;

const FRAME: f32 = 1.0 / 240.0;

fn run(fb: &mut Feedback, secs: f32) -> Vec<(f32, f32)> {
    let steps = (secs / FRAME).round() as usize;
    (0..steps)
        .map(|_| {
            fb.step(FRAME);
            (fb.kick_deg(), fb.pulse_scale())
        })
        .collect()
}

#[test]
fn a_fresh_state_is_idle_and_neutral() {
    let fb = Feedback::default();
    assert!(!fb.is_active());
    assert_eq!(fb.kick_deg(), 0.0);
    assert_eq!(fb.pulse_scale(), 1.0);
    assert_eq!(fb.flash(), None);
}

#[test]
fn the_kick_pushes_ahead_by_about_one_and_a_half_degrees_and_falls_back_without_swinging() {
    for direction in [1.0f32, -1.0] {
        let mut fb = Feedback::default();
        fb.crossed(direction, 105);
        let samples = run(&mut fb, 0.3);
        let peak = samples
            .iter()
            .map(|s| s.0 * direction)
            .fold(f32::MIN, f32::max);
        let trough = samples
            .iter()
            .map(|s| s.0 * direction)
            .fold(f32::MAX, f32::min);
        assert!((1.4..=1.6).contains(&peak), "peak {peak}");
        assert!(trough >= -1e-3, "swung back by {trough}");
        assert_eq!(fb.kick_deg(), 0.0);
    }
}

#[test]
fn the_kick_has_decayed_after_about_90_ms() {
    let mut fb = Feedback::default();
    fb.crossed(1.0, 105);
    let at_60 = run(&mut fb, 0.06).last().copied().unwrap().0;
    let at_120 = run(&mut fb, 0.06).last().copied().unwrap().0;
    assert!(at_60 > 0.05, "gone too early: {at_60}");
    assert!(at_120 < 0.05, "still there at 120 ms: {at_120}");
}

#[test]
fn the_flash_starts_at_full_level_for_the_crossed_tick_and_is_gone_after_120_ms() {
    let mut fb = Feedback::default();
    fb.crossed(1.0, 125);
    assert_eq!(
        fb.flash(),
        Some(Flash {
            percent: 125,
            level: 1.0
        })
    );
    fb.step(0.06);
    let mid = fb.flash().expect("still flashing");
    assert!(mid.level > 0.0 && mid.level < 0.5, "{}", mid.level);
    fb.step(0.07);
    assert_eq!(fb.flash(), None);
}

#[test]
fn the_pulse_grows_to_1_08_and_returns_to_1_within_100_ms() {
    let mut fb = Feedback::default();
    fb.crossed(1.0, 105);
    let scales: Vec<f32> = run(&mut fb, 0.2).iter().map(|s| s.1).collect();
    let peak = scales.iter().copied().fold(f32::MIN, f32::max);
    assert!((1.075..=1.081).contains(&peak), "{peak}");
    assert!(scales.iter().all(|&s| s >= 1.0));
    assert_eq!(*scales.last().unwrap(), 1.0);
    let back = scales[(0.105 / FRAME) as usize];
    assert_eq!(back, 1.0);
}

#[test]
fn everything_stops_so_the_frame_timer_can_stop() {
    let mut fb = Feedback::default();
    fb.crossed(-1.0, 95);
    assert!(fb.is_active());
    run(&mut fb, 0.5);
    assert!(!fb.is_active());
}

#[test]
fn quick_crossings_add_up_but_stay_bounded() {
    let mut fb = Feedback::default();
    for _ in 0..10 {
        fb.crossed(1.0, 105);
    }
    let peak = run(&mut fb, 0.3)
        .iter()
        .map(|s| s.0)
        .fold(f32::MIN, f32::max);
    assert!(peak > 2.5 && peak < 3.2, "{peak}");
}

#[test]
fn a_long_frame_gives_the_same_result_as_many_short_ones() {
    let mut fine = Feedback::default();
    let mut coarse = Feedback::default();
    fine.crossed(1.0, 105);
    coarse.crossed(1.0, 105);
    run(&mut fine, 0.05);
    coarse.step(0.05);
    assert!((fine.kick_deg() - coarse.kick_deg()).abs() < 0.02);
}

#[test]
fn clear_drops_every_effect() {
    let mut fb = Feedback::default();
    fb.crossed(1.0, 105);
    fb.clear();
    assert!(!fb.is_active());
}
