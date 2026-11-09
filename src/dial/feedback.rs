//! The short effects of a detent crossing: a rotational kick of the knob, a flash of the tick
//! and a pulse of the readout digits. Pure state advanced by the frame timer; no drawing.

/// The kick is an impulse that moves the knob ahead by about this many degrees and lets it fall
/// back; critically damped, so it never swings back through the detent.
const KICK_PEAK_DEG: f32 = 1.5;
/// Natural frequency of the kick: it has decayed to 2 percent after about 90 ms.
const KICK_OMEGA: f32 = 80.0;
/// Speed that makes a critically damped motion peak at `KICK_PEAK_DEG` (the peak of
/// `v * t * e^(-w t)` is `v / (w e)`).
const KICK_IMPULSE_DEG_PER_S: f32 = KICK_PEAK_DEG * KICK_OMEGA * std::f32::consts::E;
/// Crossings in quick succession add up, but not beyond this many impulses.
const KICK_MAX_IMPULSES: f32 = 2.0;
const KICK_REST_DEG: f32 = 0.01;
const KICK_REST_SPEED: f32 = 0.5;
/// How long the tick under the pointer glows.
const FLASH_SECS: f32 = 0.12;
/// The readout digits grow by `PULSE_PEAK` and return within this time.
const PULSE_SECS: f32 = 0.10;
const PULSE_PEAK: f32 = 0.08;

/// The tick that flashes and how bright it is right now (1 at the start, falling to 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Flash {
    pub(super) percent: u32,
    pub(super) level: f32,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Feedback {
    kick: f32,
    kick_speed: f32,
    flash: Option<(u32, f32)>,
    pulse_age: Option<f32>,
}

impl Feedback {
    /// A detent was crossed in `direction` (positive: clockwise) and `percent` is now chosen.
    pub(super) fn crossed(&mut self, direction: f32, percent: u32) {
        let impulse = KICK_IMPULSE_DEG_PER_S * direction.signum();
        let limit = KICK_IMPULSE_DEG_PER_S * KICK_MAX_IMPULSES;
        self.kick_speed = (self.kick_speed + impulse).clamp(-limit, limit);
        self.flash = Some((percent, 0.0));
        self.pulse_age = Some(0.0);
    }

    /// Extra knob rotation in degrees.
    pub(super) fn kick_deg(&self) -> f32 {
        self.kick
    }

    pub(super) fn flash(&self) -> Option<Flash> {
        self.flash.map(|(percent, age)| Flash {
            percent,
            level: (1.0 - age / FLASH_SECS).max(0.0).powi(2),
        })
    }

    /// Scale of the readout digits: 1 at rest, `1 + PULSE_PEAK` in the middle of a pulse.
    pub(super) fn pulse_scale(&self) -> f32 {
        let Some(age) = self.pulse_age else {
            return 1.0;
        };
        1.0 + PULSE_PEAK * (std::f32::consts::PI * age / PULSE_SECS).sin().max(0.0)
    }

    /// Some effect is still running, so the frame timer has to go on.
    pub(super) fn is_active(&self) -> bool {
        self.kick != 0.0
            || self.kick_speed != 0.0
            || self.flash.is_some()
            || self.pulse_age.is_some()
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn step(&mut self, dt: f32) {
        self.step_kick(dt);
        self.flash = self
            .flash
            .map(|(percent, age)| (percent, age + dt))
            .filter(|&(_, age)| age < FLASH_SECS);
        self.pulse_age = self
            .pulse_age
            .map(|age| age + dt)
            .filter(|&age| age < PULSE_SECS);
    }

    /// Exact solution of the critically damped motion over `dt`, stable for any frame time.
    fn step_kick(&mut self, dt: f32) {
        let decay = (-KICK_OMEGA * dt).exp();
        let carry = self.kick_speed + KICK_OMEGA * self.kick;
        self.kick = decay * (self.kick + carry * dt);
        self.kick_speed = decay * (self.kick_speed - KICK_OMEGA * carry * dt);
        if self.kick.abs() < KICK_REST_DEG && self.kick_speed.abs() < KICK_REST_SPEED {
            self.kick = 0.0;
            self.kick_speed = 0.0;
        }
    }
}

#[cfg(test)]
mod tests;
