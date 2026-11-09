use super::*;

const R: f32 = 80.0;

fn at(angle_deg: f32) -> (f32, f32) {
    let a = angle_deg.to_radians();
    (R * a.sin(), -R * a.cos())
}

fn dial(percent: u32) -> Dial {
    Dial::new(percent, 14.0)
}

fn angle_of(percent: u32) -> f32 {
    angle_of_percent(percent as f32)
}

/// Presses at `from` degrees and moves the pointer to `to` in `steps` moves; the reactions that
/// committed a change.
fn drag(d: &mut Dial, from: f32, to: f32, steps: u32) -> Vec<Reaction> {
    let (x, y) = at(from);
    d.press(x, y);
    (1..=steps)
        .map(|i| {
            let (x, y) = at(from + (to - from) * i as f32 / steps as f32);
            d.drag_to(x, y)
        })
        .filter(|r| r.committed.is_some())
        .collect()
}

fn percents(reactions: &[Reaction]) -> Vec<u32> {
    reactions.iter().filter_map(|r| r.committed).collect()
}

fn settle(d: &mut Dial) {
    for _ in 0..600 {
        d.tick(1.0 / 60.0);
    }
}

#[test]
fn starts_at_the_angle_of_the_percent() {
    let d = dial(100);
    assert_eq!(d.percent(), 100);
    assert_eq!(d.angle(), 0.0);
    assert!(!d.is_animating());
}

#[test]
fn dragging_commits_each_detent_it_passes_with_one_click_each_and_snaps_on_release() {
    let mut d = dial(100);
    let reactions = drag(&mut d, 0.0, 70.0, 140);
    assert_eq!(percents(&reactions), vec![105, 110, 115, 120, 125]);
    assert!(reactions.iter().all(|r| r.clicks == 1));
    let released = d.release();
    assert_eq!(released, Reaction::default());
    assert!(d.is_animating());
    settle(&mut d);
    assert!((d.angle() - angle_of(125)).abs() < 0.1);
    assert_eq!(d.percent(), 125);
    assert!(!d.is_animating());
}

#[test]
fn a_fast_flick_over_several_detents_reports_one_click_per_detent() {
    let mut d = dial(100);
    let (x, y) = at(0.0);
    d.press(x, y);
    let (x, y) = at(60.0);
    let reaction = d.drag_to(x, y);
    assert_eq!(reaction.committed, Some(120));
    assert_eq!(reaction.clicks, 4);
}

#[test]
fn dragging_back_commits_downwards() {
    let mut d = dial(100);
    assert_eq!(percents(&drag(&mut d, 0.0, -20.0, 40)), vec![95]);
}

#[test]
fn a_hand_resting_near_a_midpoint_does_not_commit_back_and_forth() {
    let mut d = dial(100);
    let mid = angle_of(100) + 0.5 * 13.5;
    let (x, y) = at(0.0);
    d.press(x, y);
    let mut commits = 0;
    for i in 0..200 {
        let wobble = if i % 2 == 0 { 0.4 } else { -0.4 };
        let (x, y) = at(mid + wobble);
        commits += usize::from(d.drag_to(x, y).committed.is_some());
    }
    assert_eq!(commits, 0, "a 0.4 degree wobble is inside the hysteresis");
}

#[test]
fn dragging_does_not_wrap_through_the_gap() {
    let mut d = dial(145);
    let reactions = drag(&mut d, angle_of(145), 300.0, 300);
    assert_eq!(percents(&reactions), vec![150]);
    assert_eq!(d.percent(), 150);
    d.release();
    settle(&mut d);
    assert!((d.angle() - 135.0).abs() < 1e-3);
}

#[test]
fn release_without_moving_keeps_a_detent_value() {
    let mut d = dial(120);
    let (x, y) = at(angle_of(120));
    d.press(x, y);
    assert_eq!(d.release(), Reaction::default());
    assert_eq!(d.percent(), 120);
}

#[test]
fn a_value_between_detents_is_shown_at_the_nearest_detent_and_snaps_on_the_first_click() {
    let mut d = dial(103);
    assert_eq!(d.percent(), 103, "kept until a detent is chosen");
    assert!((d.angle() - angle_of(105)).abs() < 1e-4);
    let (x, y) = at(angle_of(105));
    d.press(x, y);
    let reaction = d.release();
    assert_eq!(reaction.committed, Some(105));
    assert_eq!(reaction.clicks, 1);
}

#[test]
fn movement_inside_the_dead_zone_is_ignored() {
    let mut d = dial(100);
    d.press(R, 0.0);
    assert_eq!(d.drag_to(3.0, 3.0), Reaction::default());
    assert_eq!(d.drag_to(-3.0, 3.0), Reaction::default());
    assert_eq!(d.angle(), 0.0);
}

#[test]
fn moving_without_a_press_does_nothing() {
    let mut d = dial(100);
    let (x, y) = at(90.0);
    assert_eq!(d.drag_to(x, y), Reaction::default());
    assert_eq!(d.release(), Reaction::default());
}

#[test]
fn keys_step_five_percent_and_stop_at_the_ends() {
    let mut d = dial(100);
    assert_eq!(d.key(Key::Move(Step::Next)).committed, Some(105));
    assert_eq!(d.key(Key::Move(Step::Previous)).committed, Some(100));
    let last = d.key(Key::Move(Step::Last));
    assert_eq!((last.committed, last.clicks), (Some(150), 10));
    assert_eq!(d.key(Key::Move(Step::Next)), Reaction::default());
    assert_eq!(d.key(Key::Move(Step::First)).committed, Some(50));
    assert_eq!(d.key(Key::Move(Step::Previous)), Reaction::default());
    assert!(d.is_animating());
    settle(&mut d);
    assert!((d.angle() - -135.0).abs() < 0.1);
}

#[test]
fn confirm_and_cancel_close_without_a_change() {
    let mut d = dial(100);
    let closed = Reaction {
        close: true,
        ..Reaction::default()
    };
    assert_eq!(d.key(Key::Confirm), closed);
    assert_eq!(d.key(Key::Cancel), closed);
    assert_eq!(d.percent(), 100);
}

#[test]
fn wheel_steps_five_percent_per_notch() {
    let mut d = dial(100);
    assert_eq!(d.wheel(120).committed, Some(105));
    assert_eq!(d.wheel(-120).committed, Some(100));
    let two = d.wheel(240);
    assert_eq!((two.committed, two.clicks), (Some(110), 2));
    assert_eq!(d.wheel(-360).committed, Some(95));
}

#[test]
fn fine_wheel_deltas_add_up_to_a_notch() {
    let mut d = dial(100);
    assert_eq!(d.wheel(40), Reaction::default());
    assert_eq!(d.wheel(40), Reaction::default());
    assert_eq!(d.wheel(40).committed, Some(105));
}

/// Largest value the spring alone (without the kick) reaches while it settles.
fn spring_peak(d: &mut Dial, frames: u32) -> f32 {
    let mut peak = f32::MIN;
    for _ in 0..frames {
        d.tick(1.0 / 240.0);
        peak = peak.max(d.knob.value);
    }
    peak
}

#[test]
fn a_step_overshoots_by_about_one_and_a_half_degrees_and_ends_exactly() {
    let mut d = dial(100);
    d.key(Key::Move(Step::Next));
    let overshoot = spring_peak(&mut d, 120) - angle_of(105);
    assert!((1.0..=2.2).contains(&overshoot), "overshoot {overshoot}");
    settle(&mut d);
    assert_eq!(d.angle(), angle_of(105));
}

#[test]
fn releasing_after_a_drag_overshoots_the_detent_slightly_and_settles_within_250_ms() {
    let mut d = dial(100);
    // The pointer stops 2 degrees short of the next detent: inside the pull zone.
    let reactions = drag(&mut d, 0.0, angle_of(105) - 2.0, 40);
    assert_eq!(percents(&reactions), vec![105]);
    for _ in 0..30 {
        d.tick(1.0 / 60.0);
    }
    d.release();
    let target = angle_of(105);
    let mut peak = f32::MIN;
    let mut frames = 0;
    while d.is_animating() && frames < 120 {
        d.tick(1.0 / 240.0);
        peak = peak.max(d.knob.value);
        frames += 1;
    }
    let overshoot = peak - target;
    assert!((0.8..=2.5).contains(&overshoot), "overshoot {overshoot}");
    assert!(frames as f32 / 240.0 < 0.25, "took {frames} frames");
    assert_eq!(d.angle(), target);
}

#[test]
fn a_crossing_starts_kick_flash_and_pulse_and_they_end() {
    let mut d = dial(100);
    d.key(Key::Move(Step::Next));
    let flash = d.flash().expect("flash");
    assert_eq!((flash.percent, flash.level), (105, 1.0));
    d.tick(0.025);
    assert!(d.pulse_scale() > 1.0);
    assert!(
        d.angle() > d.knob.value,
        "the kick pushes ahead of the spring"
    );
    settle(&mut d);
    assert_eq!(d.flash(), None);
    assert_eq!(d.pulse_scale(), 1.0);
    assert!(!d.is_animating());
}

#[test]
fn a_key_that_changes_nothing_starts_no_effects() {
    let mut d = dial(150);
    d.key(Key::Move(Step::Next));
    assert_eq!(d.flash(), None);
    assert!(!d.is_animating());
}

#[test]
fn a_percent_beyond_the_dial_range_is_shown_at_the_nearest_end() {
    let high = dial(200);
    assert_eq!(high.percent(), 150);
    assert!((high.angle() - 135.0).abs() < 1e-4);
    let low = dial(10);
    assert_eq!(low.percent(), 50);
    assert!((low.angle() + 135.0).abs() < 1e-4);
}

#[test]
fn settle_now_stops_the_animation() {
    let mut d = dial(50);
    d.key(Key::Move(Step::Last));
    d.settle_now();
    assert!(!d.is_animating());
    assert_eq!(d.angle(), 135.0);
}
