//! Fill, hover and press animation of the pin button.
//!
//! Pure state and math, no Win32. The overlay steps a [`ButtonAnim`] from a
//! 16 ms timer and stops the timer once [`ButtonAnim::is_active`] is false.
//! Fill and hover are eased transitions; the press is a damped spring.

/// Seconds for the fill to grow (and to empty again).
const FILL_SECS: f32 = 0.15;
/// Seconds for the hover tint.
const HOVER_SECS: f32 = 0.12;
/// Button scale while the mouse is held.
const PRESS_SCALE: f32 = 0.88;
/// Spring constant of the press, per second squared (natural frequency about 40 rad/s).
const SPRING_STIFFNESS: f32 = 1600.0;
/// Damping while pressing, about 0.9 of critical: goes down without bouncing.
const PRESS_DAMPING: f32 = 72.0;
/// Damping on release, about 0.3 of critical: overshoots 1.0 by roughly 4 percent once, then settles.
const RELEASE_DAMPING: f32 = 24.0;
/// Longest integration step; larger frame times are split, which keeps the spring stable.
const MAX_SUBSTEP_SECS: f32 = 1.0 / 240.0;
/// The spring counts as arrived within this distance of the target ...
const REST_DISTANCE: f32 = 0.002;
/// ... and below this speed (scale units per second).
const REST_SPEED: f32 = 0.05;

/// A 0..1 value moving linearly in time towards 0 or 1, read back with cubic ease-out.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Transition {
    t: f32,
    target: f32,
    duration: f32,
}

impl Transition {
    fn new(duration: f32) -> Self {
        Self {
            t: 0.0,
            target: 0.0,
            duration,
        }
    }

    fn set(&mut self, on: bool, instant: bool) {
        self.target = if on { 1.0 } else { 0.0 };
        if instant {
            self.settle();
        }
    }

    fn settle(&mut self) {
        self.t = self.target;
    }

    fn step(&mut self, dt: f32) {
        let progress = dt / self.duration;
        if self.t < self.target {
            self.t = (self.t + progress).min(self.target);
        } else {
            self.t = (self.t - progress).max(self.target);
        }
    }

    fn value(&self) -> f32 {
        let inv = 1.0 - self.t;
        1.0 - inv * inv * inv
    }

    fn is_done(&self) -> bool {
        self.t == self.target
    }
}

/// A damped spring on one value, stepped with semi-implicit Euler in small fixed sub-steps.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Spring {
    value: f32,
    velocity: f32,
    target: f32,
    damping: f32,
}

impl Spring {
    fn new(rest: f32) -> Self {
        Self {
            value: rest,
            velocity: 0.0,
            target: rest,
            damping: PRESS_DAMPING,
        }
    }

    /// Aims at `target` with `damping`; the current value and velocity carry over, so a change of
    /// direction mid-flight stays continuous. `instant` jumps there.
    fn set(&mut self, target: f32, damping: f32, instant: bool) {
        self.target = target;
        self.damping = damping;
        if instant {
            self.settle();
        }
    }

    fn settle(&mut self) {
        self.value = self.target;
        self.velocity = 0.0;
    }

    fn step(&mut self, dt: f32) {
        let mut left = dt;
        while left > 0.0 && !self.is_at_rest() {
            let h = left.min(MAX_SUBSTEP_SECS);
            let accel =
                SPRING_STIFFNESS * (self.target - self.value) - self.damping * self.velocity;
            self.velocity += accel * h;
            self.value += self.velocity * h;
            left -= h;
        }
        if self.is_at_rest() {
            self.settle();
        }
    }

    fn is_at_rest(&self) -> bool {
        (self.value - self.target).abs() < REST_DISTANCE && self.velocity.abs() < REST_SPEED
    }
}

/// All animated state of one pin button.
#[derive(Debug, Clone)]
pub(crate) struct ButtonAnim {
    reduced: bool,
    press: Spring,
    hover: Transition,
    fill: Transition,
}

impl ButtonAnim {
    /// Creates an idle button; `reduced` makes every change instant.
    pub(crate) fn new(reduced: bool) -> Self {
        Self {
            reduced,
            press: Spring::new(1.0),
            hover: Transition::new(HOVER_SECS),
            fill: Transition::new(FILL_SECS),
        }
    }

    /// Switches reduced mode; turning it on settles everything immediately.
    pub(crate) fn set_reduced(&mut self, reduced: bool) {
        self.reduced = reduced;
        if reduced {
            self.press.set(1.0, RELEASE_DAMPING, true);
            self.hover.settle();
            self.fill.settle();
        }
    }

    /// Starts or ends the hover transition.
    pub(crate) fn on_hover(&mut self, hovering: bool) {
        self.hover.set(hovering, self.reduced);
    }

    /// Mouse down: the button springs down to `PRESS_SCALE` (no effect in reduced mode).
    pub(crate) fn on_press(&mut self) {
        if !self.reduced {
            self.press.set(PRESS_SCALE, PRESS_DAMPING, false);
        }
    }

    /// Mouse up: the button springs back past full size once and settles.
    pub(crate) fn on_release(&mut self) {
        self.press.set(1.0, RELEASE_DAMPING, self.reduced);
    }

    /// Animates the body fill towards `pinned`.
    pub(crate) fn set_pinned(&mut self, pinned: bool) {
        self.fill.set(pinned, self.reduced);
    }

    /// Advances all animations by `dt` seconds.
    pub(crate) fn step(&mut self, dt: f32) {
        if dt.is_nan() || dt <= 0.0 {
            return;
        }
        self.press.step(dt);
        self.hover.step(dt);
        self.fill.step(dt);
    }

    /// Returns true while anything still moves; the caller may stop its timer when false.
    pub(crate) fn is_active(&self) -> bool {
        !(self.press.is_at_rest() && self.hover.is_done() && self.fill.is_done())
    }

    /// Button scale: 1.0 at rest, `PRESS_SCALE` while pressed, a little above 1.0 right after release.
    pub(crate) fn scale(&self) -> f32 {
        self.press.value
    }

    /// Eased hover amount in 0..=1.
    pub(crate) fn hover(&self) -> f32 {
        self.hover.value()
    }

    /// Eased fill amount in 0..=1: 0 is the outline, 1 the filled body.
    pub(crate) fn fill(&self) -> f32 {
        self.fill.value()
    }
}

#[cfg(test)]
mod tests;
