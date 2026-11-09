use super::*;

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

fn angle_of(percent: u32) -> f32 {
    angle_of_percent(percent as f32)
}

#[test]
fn detents_are_every_five_percent_from_50_to_150() {
    assert_eq!(DETENTS.len(), 21);
    assert!(DETENTS.windows(2).all(|w| w[1] - w[0] == STEP_PERCENT));
    assert_eq!((DETENTS[0], DETENTS[20]), (MIN_PERCENT, MAX_PERCENT));
    assert!(DETENTS.contains(&CENTER_PERCENT));
}

#[test]
fn the_setting_range_still_covers_the_dial_range() {
    assert_eq!(crate::config::clamp_button_scale(0), MIN_PERCENT);
    assert!(crate::config::clamp_button_scale(u32::MAX) >= MAX_PERCENT);
}

#[test]
fn the_center_is_100_percent_at_twelve_oclock_and_the_arc_is_270_degrees() {
    assert!(near(angle_of(100), 0.0));
    assert!(near(angle_of(50), -135.0));
    assert!(near(angle_of(150), 135.0));
    assert!(near(STEP_DEG, 13.5));
    assert!(near(angle_of(105) - angle_of(100), 13.5));
    assert!(near(angle_of(125), 67.5));
}

#[test]
fn the_scale_is_symmetric_about_the_center() {
    for d in 1..=10u32 {
        assert!(near(angle_of(100 + 5 * d), -angle_of(100 - 5 * d)));
    }
}

#[test]
fn tick_classes_are_one_major_four_big_and_the_rest_small() {
    let count = |class| DETENTS.iter().filter(|&&d| tick_class(d) == class).count();
    assert_eq!(count(TickClass::Major), 1);
    assert_eq!(count(TickClass::Big), 4);
    assert_eq!(count(TickClass::Small), 16);
    assert_eq!(tick_class(100), TickClass::Major);
    assert_eq!(tick_class(75), TickClass::Big);
    assert_eq!(tick_class(50), TickClass::Big);
    assert_eq!(tick_class(150), TickClass::Big);
    assert_eq!(tick_class(125), TickClass::Big);
    assert_eq!(tick_class(105), TickClass::Small);
    assert_eq!(tick_class(60), TickClass::Small);
    assert_eq!(tick_class(140), TickClass::Small);
}

#[test]
fn angle_percent_round_trip_and_clamps() {
    for p in [50.0, 77.5, 100.0, 133.3, 150.0] {
        assert!(near(percent_of_angle(angle_of_percent(p)), p), "{p}");
    }
    assert!(near(angle_of_percent(10.0), MIN_ANGLE));
    assert!(near(angle_of_percent(900.0), MAX_ANGLE));
    assert!(near(percent_of_angle(-170.0), 50.0));
    assert!(near(percent_of_angle(170.0), 150.0));
}

#[test]
fn nearest_detent_picks_the_closest_and_the_lower_on_a_tie() {
    assert_eq!(nearest_detent(0.0), 50);
    assert_eq!(nearest_detent(62.0), 60);
    assert_eq!(nearest_detent(63.0), 65);
    assert_eq!(nearest_detent(62.5), 60);
    assert_eq!(nearest_detent(102.0), 100);
    assert_eq!(nearest_detent(103.0), 105);
    assert_eq!(nearest_detent(139.0), 140);
    assert_eq!(nearest_detent(900.0), 150);
}

#[test]
fn pointer_angle_is_clockwise_from_the_top() {
    assert!(near(pointer_angle(0.0, -10.0), 0.0));
    assert!(near(pointer_angle(10.0, 0.0), 90.0));
    assert!(near(pointer_angle(-10.0, 0.0), -90.0));
    assert!(near(pointer_angle(0.0, 10.0).abs(), 180.0));
    assert!(near(pointer_angle(10.0, -10.0), 45.0));
}

#[test]
fn step_angle_follows_small_moves() {
    assert!(near(step_angle(0.0, 10.0, 25.0), 15.0));
    assert!(near(step_angle(0.0, 25.0, 10.0), -15.0));
}

#[test]
fn step_angle_stops_at_the_ends() {
    assert!(near(step_angle(130.0, 130.0, 150.0), MAX_ANGLE));
    assert!(near(step_angle(-130.0, -130.0, -150.0), MIN_ANGLE));
}

#[test]
fn step_angle_does_not_jump_across_the_gap() {
    let at_max = step_angle(MAX_ANGLE, 170.0, -170.0);
    assert!(near(at_max, MAX_ANGLE), "{at_max}");
    let at_min = step_angle(MIN_ANGLE, -170.0, 170.0);
    assert!(near(at_min, MIN_ANGLE), "{at_min}");
    assert!(near(step_angle(10.0, 180.0, -179.0), 11.0));
}

#[test]
fn step_angle_knob_stays_at_max_when_pointer_circles_on() {
    let mut knob = MAX_ANGLE;
    let mut previous = 120.0;
    for pointer in [150.0, 179.0, -170.0, -140.0, -100.0] {
        knob = step_angle(knob, previous, pointer);
        previous = pointer;
    }
    assert!(knob > 0.0, "the knob jumped to the minimum side: {knob}");
}

#[test]
fn the_ratchet_leaves_a_knob_on_a_detent_where_it_is() {
    for d in DETENTS {
        let (shown, value) = ratchet(angle_of(d), d);
        assert!(near(shown, angle_of(d)), "{d}");
        assert_eq!(value, d);
    }
}

#[test]
fn the_ratchet_pulls_strongly_inside_the_zone_and_follows_beyond_it() {
    let detent = angle_of(100);
    let zone = PULL_ZONE_STEPS * STEP_DEG;
    let (shown, _) = ratchet(detent + 0.5 * zone, 100);
    assert!(shown - detent < 0.3 * zone, "weak pull: {}", shown - detent);
    assert!(shown > detent);
    let (shown, _) = ratchet(detent - 0.5 * zone, 100);
    assert!(detent - shown < 0.3 * zone && shown < detent);
    let outside = detent + 0.5 * STEP_DEG;
    let (shown, _) = ratchet(outside, 100);
    assert!(near(shown, outside));
}

#[test]
fn the_ratchet_is_continuous_and_never_moves_backwards() {
    let mut last = None;
    let mut raw = MIN_ANGLE;
    while raw <= MAX_ANGLE {
        let (shown, _) = ratchet(raw, nearest_detent(percent_of_angle(raw)));
        if let Some(previous) = last {
            assert!(shown >= previous - 1e-4, "backwards at {raw}");
            assert!(
                shown - previous < 0.03,
                "jump at {raw}: {}",
                shown - previous
            );
        }
        last = Some(shown);
        raw += 0.01;
    }
}

#[test]
fn the_value_changes_past_the_midpoint_plus_the_hysteresis_in_both_directions() {
    let d100 = angle_of(100);
    let mid = d100 + 0.5 * STEP_DEG;
    let margin = HYSTERESIS_STEPS * STEP_DEG;
    assert_eq!(
        ratchet(mid + 0.5 * margin, 100).1,
        100,
        "inside the margin going up"
    );
    assert_eq!(ratchet(mid + 1.5 * margin, 100).1, 105);
    assert_eq!(
        ratchet(mid - 0.5 * margin, 105).1,
        105,
        "inside the margin going down"
    );
    assert_eq!(ratchet(mid - 1.5 * margin, 105).1, 100);
}

#[test]
fn a_pointer_resting_on_the_midpoint_never_flickers() {
    let mid = angle_of(100) + 0.5 * STEP_DEG;
    let mut value = 100;
    let mut changes = 0;
    for i in 0..400 {
        let wobble = 0.04 * STEP_DEG * if i % 2 == 0 { 1.0 } else { -1.0 };
        let next = ratchet(mid + wobble, value).1;
        changes += u32::from(next != value);
        value = next;
    }
    assert_eq!(changes, 0);
}

#[test]
fn a_sweep_crosses_each_detent_exactly_once_each_way() {
    let sweep = |from: f32, to: f32, start: u32| {
        let mut value = start;
        let mut changes = 0;
        let steps = 2000;
        for i in 0..=steps {
            let raw = from + (to - from) * i as f32 / steps as f32;
            let next = ratchet(raw, value).1;
            changes += u32::from(next != value);
            value = next;
        }
        (value, changes)
    };
    assert_eq!(sweep(MIN_ANGLE, MAX_ANGLE, 50), (150, 20));
    assert_eq!(sweep(MAX_ANGLE, MIN_ANGLE, 150), (50, 20));
}

#[test]
fn step_detent_moves_one_detent() {
    assert_eq!(step_detent(100, Step::Next), 105);
    assert_eq!(step_detent(100, Step::Previous), 95);
    assert_eq!(step_detent(50, Step::Previous), 50);
    assert_eq!(step_detent(150, Step::Next), 150);
    assert_eq!(step_detent(87, Step::Next), 90);
    assert_eq!(step_detent(87, Step::Previous), 85);
    assert_eq!(step_detent(100, Step::First), 50);
    assert_eq!(step_detent(100, Step::Last), 150);
    assert_eq!(step_detent(200, Step::Previous), 150);
}
