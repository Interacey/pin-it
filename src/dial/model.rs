//! Pure math of the dial: the scale with its detents and tick classes, the angle <-> percent
//! mapping, the ratchet profile (how far the knob is pulled into a detent and when the value
//! changes), the wrap protection at the gap and keyboard stepping. No Win32, no drawing.
//!
//! Angles are degrees from straight up, clockwise: 0 is 12 o'clock and 100 percent, -135 is the
//! minimum (lower left), +135 the maximum (lower right), and the 90 degrees around 180 form the
//! gap at the bottom.

/// Distance between two detents in percent.
pub(super) const STEP_PERCENT: u32 = 5;
/// The size a fresh install uses and the middle of the scale: 12 o'clock.
pub(super) const CENTER_PERCENT: u32 = 100;
/// Smallest and largest percent the dial can show.
pub(super) const MIN_PERCENT: u32 = 50;
pub(super) const MAX_PERCENT: u32 = 150;
/// Every 25 percent from the center is a big tick (the two ends are among them).
const BIG_TICK_EVERY_PERCENT: u32 = 25;

/// The snap points, ascending: every 5 percent from `MIN_PERCENT` to `MAX_PERCENT`. The settings
/// accept up to 200 percent, which the dial shows at its upper end.
pub(super) const DETENTS: [u32; 21] = [
    50, 55, 60, 65, 70, 75, 80, 85, 90, 95, 100, 105, 110, 115, 120, 125, 130, 135, 140, 145, 150,
];

/// Angle of `MIN_PERCENT`.
pub(super) const MIN_ANGLE: f32 = -135.0;
/// Angle of `MAX_PERCENT`; the arc is 270 degrees.
pub(super) const MAX_ANGLE: f32 = 135.0;
/// Angle of one percent: the center is 0 degrees and each end 50 percent away.
const DEG_PER_PERCENT: f32 = MAX_ANGLE / (MAX_PERCENT - CENTER_PERCENT) as f32;
/// Angle between two detents.
pub(super) const STEP_DEG: f32 = DEG_PER_PERCENT * STEP_PERCENT as f32;
const HALF_TURN: f32 = 180.0;
const FULL_TURN: f32 = 360.0;

/// Within this share of a step around a detent the shown knob is pulled into it.
const PULL_ZONE_STEPS: f32 = 0.35;
/// The value only changes once the knob is this share of a step beyond the midpoint between two
/// detents, so a hand resting on the midpoint cannot make it flicker.
const HYSTERESIS_STEPS: f32 = 0.10;

/// How prominent a tick of the scale is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TickClass {
    /// 100 percent, the middle: the largest tick.
    Major,
    /// Every 25 percent from 100: 50, 75, 125 and 150 on this scale.
    Big,
    /// The other multiples of 5.
    Small,
}

pub(super) fn tick_class(percent: u32) -> TickClass {
    let from_center = percent.abs_diff(CENTER_PERCENT);
    if from_center == 0 {
        TickClass::Major
    } else if from_center.is_multiple_of(BIG_TICK_EVERY_PERCENT) {
        TickClass::Big
    } else {
        TickClass::Small
    }
}

/// Angle of `percent`, linear over the arc; values outside the range are clamped.
pub(super) fn angle_of_percent(percent: f32) -> f32 {
    ((percent - CENTER_PERCENT as f32) * DEG_PER_PERCENT).clamp(MIN_ANGLE, MAX_ANGLE)
}

/// Percent at `angle`, the inverse of `angle_of_percent`.
pub(super) fn percent_of_angle(angle: f32) -> f32 {
    CENTER_PERCENT as f32 + angle.clamp(MIN_ANGLE, MAX_ANGLE) / DEG_PER_PERCENT
}

/// The detent closest to `percent`; the lower one on an exact tie.
pub(super) fn nearest_detent(percent: f32) -> u32 {
    let distance = |d: &u32| (*d as f32 - percent).abs();
    DETENTS
        .iter()
        .copied()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .unwrap_or(CENTER_PERCENT)
}

/// Where the pointer is around the dial center, in degrees from straight up, clockwise, in
/// (-180, 180]. `dx` and `dy` are screen offsets (y grows downwards).
pub(super) fn pointer_angle(dx: f32, dy: f32) -> f32 {
    dx.atan2(-dy).to_degrees()
}

/// An angle difference folded into [-180, 180), so a pointer passing the 6 o'clock position
/// moves by a few degrees and not by almost 360.
fn shortest_delta(delta: f32) -> f32 {
    (delta + HALF_TURN).rem_euclid(FULL_TURN) - HALF_TURN
}

/// New knob angle after the pointer moved from `previous` to `current`. The knob follows the
/// pointer's change, never its absolute position, and stops at the ends of the arc, so dragging
/// through the gap cannot jump from the minimum to the maximum.
pub(super) fn step_angle(knob: f32, previous: f32, current: f32) -> f32 {
    (knob + shortest_delta(current - previous)).clamp(MIN_ANGLE, MAX_ANGLE)
}

/// The angle the knob is shown at for the pointer-driven angle `raw`. Within `PULL_ZONE_STEPS`
/// of a detent the knob lags behind the pointer (the pull is strongest in the detent) and
/// catches up smoothly: the profile is `3t^3 - 2t^4` over the zone, which is monotonic and meets
/// the free part with the same slope, so the knob never jumps and never moves backwards.
fn pulled_angle(raw: f32) -> f32 {
    let detent = angle_of_percent(nearest_detent(percent_of_angle(raw)) as f32);
    let zone = PULL_ZONE_STEPS * STEP_DEG;
    let offset = raw - detent;
    let t = offset.abs() / zone;
    if t >= 1.0 {
        return raw;
    }
    let eased = t.powi(3) * (3.0 - 2.0 * t);
    detent + offset.signum() * zone * eased
}

/// The detent value for `raw` while `current` is held: it only changes when `raw` is beyond the
/// midpoint to a neighbour by the hysteresis margin.
fn held_detent(raw: f32, current: u32) -> u32 {
    let reach = (0.5 + HYSTERESIS_STEPS) * STEP_DEG;
    if (raw - angle_of_percent(current as f32)).abs() <= reach {
        current
    } else {
        nearest_detent(percent_of_angle(raw))
    }
}

/// The ratchet: for the pointer-driven angle `raw` and the detent `current` chosen so far, the
/// angle to show and the detent to choose.
pub(super) fn ratchet(raw: f32, current: u32) -> (f32, u32) {
    (pulled_angle(raw), held_detent(raw, current))
}

/// A keyboard or wheel move along the detents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Previous,
    Next,
    First,
    Last,
}

/// The detent `step` leads to from `percent`, which may lie between two detents (a value from the
/// settings file). At an end, `Previous` and `Next` stay where they are.
pub(super) fn step_detent(percent: u32, step: Step) -> u32 {
    match step {
        Step::First => MIN_PERCENT,
        Step::Last => MAX_PERCENT,
        Step::Next => DETENTS
            .iter()
            .copied()
            .find(|&d| d > percent)
            .unwrap_or(MAX_PERCENT),
        Step::Previous => DETENTS
            .iter()
            .rev()
            .copied()
            .find(|&d| d < percent)
            .unwrap_or(MIN_PERCENT),
    }
}

#[cfg(test)]
mod tests;
