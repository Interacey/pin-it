//! Opacity of the overlay while Windows animates the target window.
//!
//! Minimize, maximize and restore animate a bitmap of the window; the real window rect jumps to its final
//! place at once (measured with a 0.1 ms poll: no intermediate rect is ever visible). An overlay that follows
//! the rect would sit at the final slot while the window is still flying there, so it fades out instead and
//! comes back once the animation is over. Pure state, stepped with the frame time like `ButtonAnim`.

/// Seconds the overlay takes to vanish when the target is minimized.
const FADE_OUT_SECS: f32 = 0.10;
/// Seconds the overlay stays invisible while Windows animates the target.
const HOLD_SECS: f32 = 0.20;
/// Seconds the overlay takes to return after the animation.
const FADE_IN_SECS: f32 = 0.15;

/// One ramp of the opacity: waits `delay` seconds at `from`, then moves to `to` in `secs` seconds, decelerating.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Fade {
    from: f32,
    to: f32,
    delay: f32,
    secs: f32,
    elapsed: f32,
}

impl Fade {
    /// Fully visible and idle.
    pub(super) fn new() -> Self {
        Self::ramp(1.0, 1.0, 0.0, 0.0)
    }

    fn ramp(from: f32, to: f32, delay: f32, secs: f32) -> Self {
        Self {
            from,
            to,
            delay,
            secs,
            elapsed: 0.0,
        }
    }

    /// Fades from the current opacity to nothing.
    pub(super) fn fade_out(&mut self) {
        *self = Self::ramp(self.opacity(), 0.0, 0.0, FADE_OUT_SECS);
    }

    /// Vanishes at once, waits for the window animation and fades in.
    pub(super) fn hold_then_fade_in(&mut self) {
        *self = Self::ramp(0.0, 1.0, HOLD_SECS, FADE_IN_SECS);
    }

    /// Makes an overlay that has faded out visible again; a ramp that is still running is left alone.
    pub(super) fn ensure_visible(&mut self) {
        if !self.is_active() && self.opacity() == 0.0 {
            *self = Self::new();
        }
    }

    /// Advances by `dt` seconds; a negative or NaN step changes nothing.
    pub(super) fn step(&mut self, dt: f32) {
        if dt > 0.0 {
            self.elapsed += dt;
        }
    }

    /// Opacity in 0..=1.
    pub(super) fn opacity(&self) -> f32 {
        let progress = if self.secs <= 0.0 {
            1.0
        } else {
            ((self.elapsed - self.delay) / self.secs).clamp(0.0, 1.0)
        };
        let inv = 1.0 - progress;
        self.from + (self.to - self.from) * (1.0 - inv * inv * inv)
    }

    /// Opacity as the constant alpha of `UpdateLayeredWindow`.
    pub(super) fn alpha(&self) -> u8 {
        (self.opacity() * f32::from(u8::MAX)).round() as u8
    }

    /// True while a ramp still runs, so the caller keeps its timer.
    pub(super) fn is_active(&self) -> bool {
        self.elapsed < self.delay + self.secs
    }

    /// True when the overlay is invisible and stays so.
    pub(super) fn is_hidden(&self) -> bool {
        self.alpha() == 0
    }

    /// True once a fade-out has finished.
    pub(super) fn has_faded_out(&self) -> bool {
        !self.is_active() && self.to == 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f32 = 1.0 / 60.0;

    fn run(fade: &mut Fade, secs: f32) {
        let mut left = secs;
        while left > 0.0 {
            fade.step(FRAME.min(left));
            left -= FRAME;
        }
    }

    #[test]
    fn a_new_fade_is_visible_and_idle() {
        let f = Fade::new();
        assert_eq!(
            (f.alpha(), f.is_active(), f.is_hidden()),
            (255, false, false)
        );
    }

    #[test]
    fn fade_out_decelerates_to_nothing_within_its_time() {
        let mut f = Fade::new();
        f.fade_out();
        f.step(0.03);
        let early = f.opacity();
        f.step(0.03);
        let later = f.opacity();
        assert!(early < 1.0 && later < early);
        assert!(early - later < 1.0 - early, "ease-out: the drop slows down");
        run(&mut f, FADE_OUT_SECS);
        assert!(f.is_hidden() && f.has_faded_out() && !f.is_active());
    }

    #[test]
    fn restore_vanishes_at_once_waits_and_fades_in() {
        let mut f = Fade::new();
        f.hold_then_fade_in();
        assert_eq!(f.alpha(), 0);
        run(&mut f, HOLD_SECS - 0.02);
        assert_eq!(f.alpha(), 0, "still invisible while Windows animates");
        run(&mut f, 0.06);
        assert!(f.alpha() > 0 && f.is_active());
        run(&mut f, FADE_IN_SECS);
        assert_eq!((f.alpha(), f.is_active()), (255, false));
    }

    #[test]
    fn a_fade_out_starts_from_the_current_opacity() {
        let mut f = Fade::new();
        f.hold_then_fade_in();
        run(&mut f, HOLD_SECS + 0.05);
        let before = f.opacity();
        f.fade_out();
        assert!((f.opacity() - before).abs() < 0.01, "no jump");
    }

    #[test]
    fn ensure_visible_revives_a_faded_out_overlay_but_not_a_running_hold() {
        let mut faded = Fade::new();
        faded.fade_out();
        run(&mut faded, FADE_OUT_SECS);
        faded.ensure_visible();
        assert_eq!(faded.alpha(), 255);
        let mut holding = Fade::new();
        holding.hold_then_fade_in();
        holding.ensure_visible();
        assert_eq!(holding.alpha(), 0);
        assert!(holding.is_active());
    }

    #[test]
    fn bad_steps_change_nothing() {
        let mut f = Fade::new();
        f.fade_out();
        f.step(-1.0);
        f.step(f32::NAN);
        assert_eq!(f.alpha(), 255);
    }
}
