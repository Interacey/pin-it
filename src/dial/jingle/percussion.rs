//! Light percussion from filtered noise: a snare on the backbeat and a hat tick on the offbeats.

use super::UNIT_SECS;
use crate::dial::sound::SAMPLE_RATE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Snare,
    Hat,
}

impl Kind {
    pub(super) fn gain(self) -> f32 {
        match self {
            Kind::Snare => 0.4,
            Kind::Hat => 0.1,
        }
    }

    /// Time constant of the exponential decay.
    fn decay_secs(self) -> f32 {
        match self {
            Kind::Snare => 0.035,
            Kind::Hat => 0.006,
        }
    }

    pub(super) fn length_secs(self) -> f32 {
        match self {
            Kind::Snare => 0.12,
            Kind::Hat => 0.03,
        }
    }
}

/// A hit: start in sixteenth notes.
#[derive(Debug, Clone, Copy)]
pub(super) struct Hit {
    pub(super) start: u16,
    pub(super) kind: Kind,
}

/// Beats 2 and 4 of every bar.
const SNARE_STARTS: [u16; 5] = [4, 12, 20, 28, 36];
/// The "and" of every beat; none on the very first sample, which has to be silent.
const FIRST_HAT: u16 = 2;
const HAT_SPACING: usize = 4;
const LAST_HAT: u16 = 34;

pub(super) fn hits() -> Vec<Hit> {
    let snares = SNARE_STARTS.iter().map(|&start| Hit {
        start,
        kind: Kind::Snare,
    });
    let hats = (FIRST_HAT..=LAST_HAT)
        .step_by(HAT_SPACING)
        .map(|start| Hit {
            start,
            kind: Kind::Hat,
        });
    snares.chain(hats).collect()
}

pub(super) fn add_hits(mix: &mut [f32]) {
    for hit in hits() {
        add_hit(&hit, mix);
    }
}

/// Adds one burst: white noise from a fixed seed, high-passed by differencing, decaying.
pub(super) fn add_hit(hit: &Hit, mix: &mut [f32]) {
    let rate = SAMPLE_RATE as f32;
    let first = (f32::from(hit.start) * UNIT_SECS * rate).round() as usize;
    let count = (hit.kind.length_secs() * rate) as usize;
    let mut noise = Noise::seeded(hit.start);
    let mut previous = 0.0;
    for (i, slot) in mix.iter_mut().enumerate().skip(first).take(count) {
        let t = (i - first) as f32 / rate;
        let white = noise.next();
        let bright = (white - previous) / 2.0;
        previous = white;
        *slot += hit.kind.gain() * bright * (-t / hit.kind.decay_secs()).exp();
    }
}

/// A small xorshift generator: the same burst every run, no random-number dependency.
struct Noise(u32);

impl Noise {
    fn seeded(start: u16) -> Self {
        Noise(u32::from(start).wrapping_mul(2_654_435_761) | 1)
    }

    /// The next value in [-1, 1).
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / 2_147_483_648.0 - 1.0
    }
}
