//! The jingle of the easter egg: five seconds of chiptune synthesized once into an in-memory WAV,
//! like the click. It follows the intro groove and the first vocal phrase of a famous 1987 pop
//! hook, in the key and tempo of the original (A-flat major, about 113 bpm). There is no
//! recording, sample or lyric in it: the melody exists only as the note data in `score`.
//! Time is counted in sixteenth notes.

mod percussion;
mod score;

use std::f32::consts::TAU;
use std::sync::OnceLock;
use std::time::Duration;

use super::sound::{SAMPLE_RATE, wav_file};
use score::Note;

/// How long the jingle plays.
pub(super) const LENGTH: Duration = Duration::from_secs(5);
const TEMPO_BPM: f32 = 113.0;
/// A sixteenth note.
const UNIT_SECS: f32 = 60.0 / (TEMPO_BPM * 4.0);
/// The sixteenths that fit completely into `LENGTH`.
const TOTAL_UNITS: u16 = (LENGTH.as_secs_f32() / UNIT_SECS) as u16;
/// Loudest sample as a share of full scale: quieter than the click.
pub(super) const PEAK_SHARE: f32 = 0.25;
/// The last stretch fades to silence, so the buffer ends at zero.
const FADE_OUT_SECS: f32 = 0.2;
const ATTACK_SECS: f32 = 0.005;
const DECAY_SECS: f32 = 0.04;
const SUSTAIN_LEVEL: f32 = 0.7;
const RELEASE_SECS: f32 = 0.03;
/// A long lead note starts to wobble after this time.
const VIBRATO_DELAY_SECS: f32 = 0.3;
const VIBRATO_RATE_HZ: f32 = 5.5;
/// Share of the pitch the vibrato swings up and down: about 10 cents.
const VIBRATO_DEPTH: f32 = 0.006;
const SAMPLE_COUNT: usize = SAMPLE_RATE as usize * LENGTH.as_secs() as usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Voice {
    Lead,
    Bass,
    Stab,
}

impl Voice {
    fn gain(self) -> f32 {
        match self {
            Voice::Lead => 1.0,
            Voice::Bass => 0.9,
            Voice::Stab => 0.3,
        }
    }

    /// The waveform at `phase` in [0, 1): a square wave, and a triangle for the bass.
    fn wave(self, phase: f32) -> f32 {
        match self {
            Voice::Bass => 4.0 * (phase - 0.5).abs() - 1.0,
            Voice::Lead | Voice::Stab if phase < 0.5 => 1.0,
            Voice::Lead | Voice::Stab => -1.0,
        }
    }
}

/// The jingle as a complete WAV file, built on first use.
pub(super) fn jingle_wav() -> &'static [u8] {
    static WAV: OnceLock<Vec<u8>> = OnceLock::new();
    WAV.get_or_init(|| wav_file(&samples()))
}

fn samples() -> Vec<i16> {
    let mut mix = vec![0.0f32; SAMPLE_COUNT];
    for note in &score::score() {
        add_note(note, &mut mix);
    }
    percussion::add_hits(&mut mix);
    fade_out(&mut mix);
    normalize(&mix)
}

/// Adds the note, release tail included, to `mix`; what runs past its end is cut.
fn add_note(note: &Note, mix: &mut [f32]) {
    let rate = SAMPLE_RATE as f32;
    let first = (f32::from(note.start) * UNIT_SECS * rate).round() as usize;
    let held_secs = f32::from(note.len) * UNIT_SECS;
    for (n, slot) in mix.iter_mut().enumerate().skip(first) {
        let t = (n - first) as f32 / rate;
        if t > held_secs + RELEASE_SECS {
            break;
        }
        let phase = phase(note.voice, note.hz(), t);
        *slot += note.voice.gain() * note.voice.wave(phase) * envelope(t, held_secs);
    }
}

/// Position in the wave cycle `t` seconds into a note. The vibrato integrates the pitch swing, so
/// the phase has no jump where it begins.
fn phase(voice: Voice, hz: f32, t: f32) -> f32 {
    let steady = hz * t;
    if voice != Voice::Lead || t <= VIBRATO_DELAY_SECS {
        return steady.fract();
    }
    let swing_rate = TAU * VIBRATO_RATE_HZ;
    let since = t - VIBRATO_DELAY_SECS;
    let extra = hz * VIBRATO_DEPTH * (1.0 - (swing_rate * since).cos()) / swing_rate;
    (steady + extra).fract()
}

/// Attack, decay to the sustain level, hold until `held_secs`, then release.
fn envelope(t: f32, held_secs: f32) -> f32 {
    let held = if t < ATTACK_SECS {
        t / ATTACK_SECS
    } else if t < ATTACK_SECS + DECAY_SECS {
        1.0 - (1.0 - SUSTAIN_LEVEL) * (t - ATTACK_SECS) / DECAY_SECS
    } else {
        SUSTAIN_LEVEL
    };
    if t <= held_secs {
        held
    } else {
        held * (1.0 - (t - held_secs) / RELEASE_SECS).max(0.0)
    }
}

fn fade_out(mix: &mut [f32]) {
    let fade = (FADE_OUT_SECS * SAMPLE_RATE as f32) as usize;
    let last = mix.len() - 1;
    for (n, slot) in mix.iter_mut().enumerate() {
        *slot *= ((last - n) as f32 / fade as f32).min(1.0);
    }
}

/// Scales the mix so its loudest sample is `PEAK_SHARE` of full scale.
fn normalize(mix: &[f32]) -> Vec<i16> {
    let loudest = mix.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let gain = PEAK_SHARE * f32::from(i16::MAX) / loudest;
    mix.iter().map(|s| (s * gain).round() as i16).collect()
}

#[cfg(test)]
mod tests;
