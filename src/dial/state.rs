//! Interaction state of the dial: which detent is chosen, the knob angle with its settle spring,
//! the effects of a crossing, and what a drag, key or wheel step does. No Win32; the window feeds
//! events in and acts on the returned `Reaction`.

use super::feedback::{Feedback, Flash};
use super::model::{
    MAX_PERCENT, MIN_PERCENT, STEP_PERCENT, Step, angle_of_percent, nearest_detent, pointer_angle,
    ratchet, step_angle, step_detent,
};
use super::spring::Spring;

/// A pointer this close to the center has no stable angle, so it is ignored.
pub(super) const DEAD_ZONE_DP: f32 = 14.0;
/// One mouse wheel notch.
const WHEEL_NOTCH: i32 = 120;
/// On release the knob is flung this fast (degrees per second) past the detent, which the
/// spring turns into an overshoot of about 1.5 degrees.
const RELEASE_IMPULSE_DEG_PER_S: f32 = 135.0;
/// A knob closer than this to its detent at release goes on in the direction it was turned.
const RELEASE_MIN_OFFSET_DEG: f32 = 0.3;

/// A key the dial reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Key {
    Move(Step),
    Confirm,
    Cancel,
}

/// What the window has to do after an event.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Reaction {
    /// The chosen detent changed to this percent; apply it now.
    pub(super) committed: Option<u32>,
    /// Detents crossed by the change; one click each.
    pub(super) clicks: u32,
    /// Close the window.
    pub(super) close: bool,
}

/// A mouse drag in progress.
#[derive(Debug, Clone, Copy)]
struct Drag {
    /// Pointer angle at the previous move; `None` until the pointer has left the dead zone.
    last_pointer: Option<f32>,
    /// Sign of the last knob movement, for the push on release.
    heading: f32,
}

/// The dial: current percent, knob angle and drag.
#[derive(Debug)]
pub(super) struct Dial {
    percent: u32,
    knob: Spring,
    /// Knob angle during a drag, driven by the pointer before the ratchet shapes it.
    raw: f32,
    drag: Option<Drag>,
    dead_zone_px: f32,
    wheel_remainder: i32,
    feedback: Feedback,
}

impl Dial {
    /// A dial showing `percent` (a value outside the dial range, as the settings file allows, is
    /// shown at the nearest end; one between two detents is shown at the nearest detent but kept
    /// until a detent is chosen); `dead_zone_px` is the pointer distance from the center below
    /// which drag movement is ignored.
    pub(super) fn new(percent: u32, dead_zone_px: f32) -> Self {
        let percent = percent.clamp(MIN_PERCENT, MAX_PERCENT);
        let angle = angle_of_percent(nearest_detent(percent as f32) as f32);
        Self {
            percent,
            knob: Spring::at(angle),
            raw: angle,
            drag: None,
            dead_zone_px,
            wheel_remainder: 0,
            feedback: Feedback::default(),
        }
    }

    /// The percent chosen so far.
    pub(super) fn percent(&self) -> u32 {
        self.percent
    }

    /// The angle the knob is drawn at, with the kick of a crossing.
    pub(super) fn angle(&self) -> f32 {
        self.knob.value + self.feedback.kick_deg()
    }

    /// The tick that is flashing, if any.
    pub(super) fn flash(&self) -> Option<Flash> {
        self.feedback.flash()
    }

    /// Scale of the readout digits.
    pub(super) fn pulse_scale(&self) -> f32 {
        self.feedback.pulse_scale()
    }

    /// A drag is in progress.
    pub(super) fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The timer must keep running.
    pub(super) fn is_animating(&self) -> bool {
        !self.knob.is_at_rest() || self.feedback.is_active()
    }

    /// Stops the motion where it is going, for when no timer can drive it.
    pub(super) fn settle_now(&mut self) {
        self.knob.jump(self.knob.target);
        self.feedback.clear();
    }

    /// Advances the spring and the effects by `dt` seconds.
    pub(super) fn tick(&mut self, dt: f32) {
        if dt.is_finite() && dt > 0.0 {
            self.knob.step(dt);
            self.feedback.step(dt);
        }
    }

    /// Mouse down at (`dx`, `dy`) from the dial center: the knob is grabbed where it is.
    pub(super) fn press(&mut self, dx: f32, dy: f32) {
        self.knob.jump(self.knob.value);
        self.raw = self.knob.value;
        self.drag = Some(Drag {
            last_pointer: self.outside_dead_zone(dx, dy),
            heading: 0.0,
        });
    }

    /// Mouse move to (`dx`, `dy`) from the center; only acts during a drag.
    pub(super) fn drag_to(&mut self, dx: f32, dy: f32) -> Reaction {
        let Some(angle) = self.outside_dead_zone(dx, dy) else {
            return Reaction::default();
        };
        let Some(drag) = self.drag.as_mut() else {
            return Reaction::default();
        };
        let previous = drag.last_pointer.replace(angle);
        let moved = previous.map_or(0.0, |p| step_angle(self.raw, p, angle) - self.raw);
        if moved != 0.0 {
            drag.heading = moved.signum();
        }
        self.raw += moved;
        let (shown, detent) = ratchet(self.raw, nearest_detent(self.percent as f32));
        self.knob.jump(shown);
        self.commit(detent)
    }

    /// Mouse up (or lost capture): the knob settles into its detent with a small overshoot.
    pub(super) fn release(&mut self) -> Reaction {
        let Some(drag) = self.drag.take() else {
            return Reaction::default();
        };
        let detent = nearest_detent(self.percent as f32);
        let target = angle_of_percent(detent as f32);
        let offset = target - self.knob.value;
        let heading = if offset.abs() > RELEASE_MIN_OFFSET_DEG {
            offset.signum()
        } else {
            drag.heading
        };
        self.knob.aim(target);
        self.knob.velocity += heading * RELEASE_IMPULSE_DEG_PER_S;
        self.commit(detent)
    }

    /// A key press.
    pub(super) fn key(&mut self, key: Key) -> Reaction {
        match key {
            Key::Move(step) => self.go_to(step_detent(self.percent, step)),
            Key::Confirm | Key::Cancel => Reaction {
                close: true,
                ..Reaction::default()
            },
        }
    }

    /// Wheel movement in wheel units (120 per notch, positive is up): one detent per notch.
    pub(super) fn wheel(&mut self, delta: i32) -> Reaction {
        self.wheel_remainder += delta;
        let notches = self.wheel_remainder / WHEEL_NOTCH;
        self.wheel_remainder -= notches * WHEEL_NOTCH;
        let step = if notches > 0 {
            Step::Next
        } else {
            Step::Previous
        };
        let target = (0..notches.abs()).fold(self.percent, |p, _| step_detent(p, step));
        self.go_to(target)
    }

    fn outside_dead_zone(&self, dx: f32, dy: f32) -> Option<f32> {
        (dx.hypot(dy) >= self.dead_zone_px).then(|| pointer_angle(dx, dy))
    }

    /// Aims the knob at `percent` (a detent) and records it as chosen.
    fn go_to(&mut self, percent: u32) -> Reaction {
        self.knob.aim(angle_of_percent(percent as f32));
        self.commit(percent)
    }

    /// Chooses `percent`; a change starts the crossing effects and counts the detents passed.
    fn commit(&mut self, percent: u32) -> Reaction {
        if percent == self.percent {
            return Reaction::default();
        }
        let distance = percent.abs_diff(self.percent);
        let direction = if percent > self.percent { 1.0 } else { -1.0 };
        self.percent = percent;
        self.feedback.crossed(direction, percent);
        Reaction {
            committed: Some(percent),
            clicks: (distance / STEP_PERCENT).max(1),
            close: false,
        }
    }
}

#[cfg(test)]
mod tests;
