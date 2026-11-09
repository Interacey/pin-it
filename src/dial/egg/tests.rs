use super::*;

/// Feeds `(percent, milliseconds since start)` pairs and returns the answer to each.
fn run(detector: &mut EggDetector, start: Instant, steps: &[(u32, u64)]) -> Vec<bool> {
    steps
        .iter()
        .map(|&(percent, ms)| detector.observe(percent, start + Duration::from_millis(ms)))
        .collect()
}

fn triggers(initial: u32, steps: &[(u32, u64)]) -> usize {
    let mut detector = EggDetector::new(initial);
    run(&mut detector, Instant::now(), steps)
        .iter()
        .filter(|&&t| t)
        .count()
}

/// A drag from `from` to `to` in steps of 5, one step every `gap_ms`, starting at `t0`.
fn ramp(from: u32, to: u32, t0: u64, gap_ms: u64) -> Vec<(u32, u64)> {
    let steps = from.abs_diff(to) / 5;
    (1..=steps as u64)
        .map(|i| {
            let value = if to > from {
                from + 5 * i as u32
            } else {
                from - 5 * i as u32
            };
            (value, t0 + i * gap_ms)
        })
        .collect()
}

#[test]
fn a_drag_up_and_back_in_under_three_seconds_triggers() {
    // 20 steps up and 20 down at 72 ms: 2.88 s from the first step away to the last step back.
    let mut steps = ramp(50, 150, 0, 72);
    steps.extend(ramp(150, 50, 20 * 72, 72));
    assert_eq!(triggers(50, &steps), 1);
}

#[test]
fn the_same_drag_over_three_seconds_does_not_trigger() {
    let mut steps = ramp(50, 150, 0, 78);
    steps.extend(ramp(150, 50, 20 * 78, 78));
    assert_eq!(triggers(50, &steps), 0);
}

#[test]
fn it_triggers_on_the_closing_value_and_not_before() {
    let mut detector = EggDetector::new(50);
    let mut steps = ramp(50, 150, 0, 50);
    steps.extend(ramp(150, 50, 1000, 50));
    let answers = run(&mut detector, Instant::now(), &steps);
    assert_eq!(answers.iter().position(|&t| t), Some(answers.len() - 1));
}

#[test]
fn exactly_three_seconds_still_counts_and_a_millisecond_more_does_not() {
    let sequence = |back: u64| [(55, 0), (150, 1000), (50, back)];
    assert_eq!(triggers(50, &sequence(3000)), 1);
    assert_eq!(triggers(50, &sequence(3001)), 0);
}

#[test]
fn going_up_without_coming_back_does_not_trigger() {
    assert_eq!(triggers(50, &ramp(50, 150, 0, 20)), 0);
}

#[test]
fn going_down_without_having_been_up_does_not_trigger() {
    assert_eq!(triggers(150, &ramp(150, 50, 0, 20)), 0);
}

#[test]
fn up_to_the_middle_and_back_does_not_trigger() {
    let mut steps = ramp(50, 100, 0, 20);
    steps.extend(ramp(100, 50, 400, 20));
    assert_eq!(triggers(50, &steps), 0);
}

#[test]
fn a_turn_back_short_of_the_top_does_not_trigger() {
    let mut steps = ramp(50, 145, 0, 20);
    steps.extend(ramp(145, 50, 400, 20));
    assert_eq!(triggers(50, &steps), 0);
}

#[test]
fn the_keyboard_fast_path_home_end_home_triggers() {
    assert_eq!(triggers(100, &[(50, 0), (150, 200), (50, 400)]), 1);
}

#[test]
fn end_home_from_a_dial_that_opened_on_the_smallest_value_triggers() {
    assert_eq!(triggers(50, &[(150, 0), (50, 300)]), 1);
}

#[test]
fn starting_on_the_largest_value_needs_home_end_home() {
    assert_eq!(triggers(150, &[(50, 0), (150, 100)]), 0);
    assert_eq!(triggers(150, &[(50, 0), (150, 100), (50, 200)]), 1);
    // The clock runs from leaving the smallest value: End at 0.5 s, Home at 3.6 s is too slow
    // even though the dial first reached 50 at 0.
    assert_eq!(triggers(150, &[(50, 0), (150, 500), (50, 3600)]), 0);
}

#[test]
fn the_wheel_in_steps_of_five_triggers() {
    let mut steps = ramp(50, 150, 0, 60);
    steps.extend(ramp(150, 50, 1200, 60));
    assert_eq!(steps.len(), 40);
    assert_eq!(triggers(50, &steps), 1);
}

#[test]
fn a_slow_start_is_forgotten_and_a_fast_round_trip_after_it_counts() {
    // Up slowly, down to 50 (not a trigger: too slow), then Home-End-Home at once.
    let steps = [(55, 0), (150, 5000), (50, 6000), (150, 6100), (50, 6200)];
    assert_eq!(triggers(50, &steps), 1);
}

#[test]
fn a_second_round_trip_inside_the_cooldown_is_ignored() {
    let mut detector = EggDetector::new(50);
    let start = Instant::now();
    let first = run(&mut detector, start, &[(150, 0), (50, 100)]);
    let second = run(&mut detector, start, &[(150, 200), (50, 300)]);
    assert_eq!((first, second), (vec![false, true], vec![false, false]));
}

#[test]
fn after_the_cooldown_the_pattern_triggers_again() {
    let mut detector = EggDetector::new(50);
    let start = Instant::now();
    assert!(run(&mut detector, start, &[(150, 0), (50, 100)])[1]);
    // 30 s after the trigger at 100 ms.
    let later = run(&mut detector, start, &[(150, 30_100), (50, 30_200)]);
    assert_eq!(later, [false, true]);
}

#[test]
fn just_before_the_cooldown_ends_it_is_still_ignored() {
    let mut detector = EggDetector::new(50);
    let start = Instant::now();
    assert!(run(&mut detector, start, &[(150, 0), (50, 100)])[1]);
    let early = run(&mut detector, start, &[(150, 29_000), (50, 30_099)]);
    assert_eq!(early, [false, false]);
}

#[test]
fn a_trigger_resets_the_history() {
    // Back on 50 after a trigger: a lone End and a later Home inside the cooldown are only the
    // start of a new pattern, and a half pattern from before does not carry over.
    let mut detector = EggDetector::new(50);
    let start = Instant::now();
    assert!(run(&mut detector, start, &[(150, 0), (50, 100)])[1]);
    let after = run(&mut detector, start, &[(100, 31_000), (50, 31_100)]);
    assert_eq!(after, [false, false], "100 -> 50 is not a pattern");
}

#[test]
fn a_cooled_down_trigger_does_not_start_the_cooldown_twice() {
    let mut detector = EggDetector::new(50);
    let start = Instant::now();
    run(&mut detector, start, &[(150, 0), (50, 100)]);
    // Ignored inside the cooldown: must not extend it.
    run(&mut detector, start, &[(150, 20_000), (50, 20_100)]);
    let at = run(&mut detector, start, &[(150, 30_100), (50, 30_200)]);
    assert_eq!(at, [false, true]);
}
