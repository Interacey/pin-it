//! A damped spring on the knob angle: it settles the knob into a detent with one small overshoot.

/// Spring constant per second squared: a natural frequency of 45 rad/s.
const STIFFNESS: f32 = 2025.0;
/// Damping ratio 0.6: an overshoot of about a tenth of the travel (1.6 degrees for one step),
/// settled to 2 percent in about 150 ms.
const DAMPING: f32 = 54.0;
/// Longest integration step; larger frame times are split, which keeps the spring stable.
const MAX_SUBSTEP_SECS: f32 = 1.0 / 240.0;
/// The knob counts as arrived within this many degrees ...
const REST_DISTANCE_DEG: f32 = 0.05;
/// ... and below this speed (degrees per second).
const REST_SPEED_DEG: f32 = 0.5;

#[derive(Debug, Clone, Copy)]
pub(super) struct Spring {
    pub(super) value: f32,
    pub(super) velocity: f32,
    pub(super) target: f32,
}

impl Spring {
    pub(super) fn at(value: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
        }
    }

    /// Moves to `value` at once and stays there.
    pub(super) fn jump(&mut self, value: f32) {
        *self = Self::at(value);
    }

    /// Aims at `target`; position and velocity carry over.
    pub(super) fn aim(&mut self, target: f32) {
        self.target = target;
    }

    pub(super) fn is_at_rest(&self) -> bool {
        (self.value - self.target).abs() < REST_DISTANCE_DEG && self.velocity.abs() < REST_SPEED_DEG
    }

    pub(super) fn step(&mut self, dt: f32) {
        let mut left = dt;
        while left > 0.0 && !self.is_at_rest() {
            let h = left.min(MAX_SUBSTEP_SECS);
            let accel = STIFFNESS * (self.target - self.value) - DAMPING * self.velocity;
            self.velocity += accel * h;
            self.value += self.velocity * h;
            left -= h;
        }
        if self.is_at_rest() {
            self.jump(self.target);
        }
    }
}
