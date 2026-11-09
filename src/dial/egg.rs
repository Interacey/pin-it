//! The easter egg of the dial: whoever turns the size from the smallest to the largest value and
//! back within three seconds gets five seconds of a jingle (`jingle`), sound only.
//!
//! `EggDetector` is a pure state machine over the committed values and their timestamps; the
//! clock is injected. After a trigger it ignores the pattern for 30 seconds.

use std::time::{Duration, Instant};

use super::model::{MAX_PERCENT, MIN_PERCENT};

/// From leaving the smallest value to coming back to it.
const PATTERN_WINDOW: Duration = Duration::from_secs(3);
/// No second trigger within this time after a trigger.
const COOLDOWN: Duration = Duration::from_secs(30);

/// Where the sequence of committed values stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Not on the pattern: the dial is somewhere between or went up too slowly.
    Idle,
    /// On the smallest value; the next step away starts the clock.
    AtMin,
    /// Left the smallest value at `left` and has not reached the largest yet.
    Climbing { left: Instant },
    /// Reached the largest value; the way back to the smallest decides.
    AtMax { left: Instant },
}

pub(super) struct EggDetector {
    stage: Stage,
    quiet_until: Option<Instant>,
}

impl EggDetector {
    /// A dial that opens on `percent`; opening on the smallest value counts as being there.
    pub(super) fn new(percent: u32) -> Self {
        let stage = if percent == MIN_PERCENT {
            Stage::AtMin
        } else {
            Stage::Idle
        };
        Self {
            stage,
            quiet_until: None,
        }
    }

    /// The dial committed `percent` at `now`; true when the pattern just completed and the
    /// cooldown allows the egg.
    pub(super) fn observe(&mut self, percent: u32, now: Instant) -> bool {
        self.drop_expired(now);
        let completed = self.advance(percent, now);
        if !completed {
            return false;
        }
        let allowed = self.quiet_until.is_none_or(|until| now >= until);
        if allowed {
            self.quiet_until = Some(now + COOLDOWN);
        }
        allowed
    }

    fn drop_expired(&mut self, now: Instant) {
        if let Stage::Climbing { left } | Stage::AtMax { left } = self.stage
            && now.saturating_duration_since(left) > PATTERN_WINDOW
        {
            self.stage = Stage::Idle;
        }
    }

    /// Moves to the next stage; true when this value completes the pattern. The history starts
    /// over at the smallest value, where the dial stands after a completed pattern.
    fn advance(&mut self, percent: u32, now: Instant) -> bool {
        let completed = percent == MIN_PERCENT && matches!(self.stage, Stage::AtMax { .. });
        self.stage = match (self.stage, percent) {
            (_, MIN_PERCENT) => Stage::AtMin,
            (Stage::AtMin, MAX_PERCENT) => Stage::AtMax { left: now },
            (Stage::AtMin, _) => Stage::Climbing { left: now },
            (Stage::Climbing { left }, MAX_PERCENT) => Stage::AtMax { left },
            (stage, _) => stage,
        };
        completed
    }
}

#[cfg(test)]
mod tests;
