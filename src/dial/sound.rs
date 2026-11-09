//! The click of a detent: a 3 ms burst synthesized once into an in-memory WAV buffer and played
//! through `PlaySoundW`. `Clicker` decides whether a crossing is heard (setting, closing, at most
//! 25 clicks per second, none while the easter egg jingle plays) and starts and stops that jingle;
//! the `Player` seam lets tests record the requests instead of playing.

use std::f32::consts::TAU;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT, SND_PURGE};
use windows::core::PCWSTR;

use super::egg::EggDetector;
use super::jingle::{LENGTH as JINGLE_LENGTH, jingle_wav};

pub(super) const SAMPLE_RATE: u32 = 22_050;
const CLICK_SECS: f32 = 0.003;
const TONE_HZ: f32 = 3200.0;
const TONE_SHARE: f32 = 0.6;
const NOISE_SHARE: f32 = 0.4;
/// Time constant of the decay: the burst has lost 98 percent after the 3 ms.
const DECAY_SECS: f32 = 0.0007;
/// Loudest sample as a share of full scale: a quiet click.
pub(super) const PEAK_SHARE: f32 = 0.35;
/// Fade in and out, so the buffer starts and ends at zero and cannot pop.
const ATTACK_SAMPLES: usize = 4;
const TAPER_SAMPLES: usize = 10;
const NOISE_SEED: u32 = 0x2545_F491;
const WAV_HEADER_LEN: usize = 44;
const BYTES_PER_SAMPLE: u32 = 2;
/// At most 25 clicks per second.
const MIN_GAP: Duration = Duration::from_millis(40);

/// Plays a prepared WAV buffer; the buffer stays valid for the program's lifetime, which the
/// asynchronous playback from memory needs.
pub(super) trait Player {
    fn play(&mut self, wav: &'static [u8]);
    /// Plays the long jingle of the easter egg; it can be cut short with `stop`.
    fn play_long(&mut self, wav: &'static [u8]);
    /// Stops whatever this player started.
    fn stop(&mut self);
}

/// The real thing: `PlaySoundW` from memory, without waiting.
struct SystemPlayer;

impl Player for SystemPlayer {
    fn play(&mut self, wav: &'static [u8]) {
        play_from_memory(wav);
    }

    fn play_long(&mut self, wav: &'static [u8]) {
        play_from_memory(wav);
    }

    fn stop(&mut self) {
        // SAFETY: a null sound with SND_PURGE only stops the waveform sounds of this thread's
        // process; there is no buffer to keep alive.
        let _ = unsafe { PlaySoundW(PCWSTR::null(), None, SND_PURGE) };
    }
}

fn play_from_memory(wav: &'static [u8]) {
    // SAFETY: `wav` is 'static, so it outlives the asynchronous playback from memory. A
    // missing audio device makes the call return false; a sound is not worth an error.
    let _ = unsafe {
        PlaySoundW(
            PCWSTR(wav.as_ptr().cast()),
            None,
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        )
    };
}

/// The player a window starts with: the system one, and in tests one that only counts.
#[cfg(not(test))]
pub(super) fn default_player() -> Box<dyn Player> {
    Box::new(SystemPlayer)
}

#[cfg(test)]
pub(super) fn default_player() -> Box<dyn Player> {
    Box::new(counting::Counting)
}

/// Test player of the window tests: no sound, a per-thread count of the requests.
#[cfg(test)]
pub(super) mod counting {
    use std::cell::Cell;

    use super::Player;

    thread_local! {
        static REQUESTS: Cell<usize> = const { Cell::new(0) };
        static EGGS: Cell<usize> = const { Cell::new(0) };
        static STOPS: Cell<usize> = const { Cell::new(0) };
    }

    fn bump(counter: &'static std::thread::LocalKey<Cell<usize>>) {
        counter.with(|n| n.set(n.get() + 1));
    }

    pub(in crate::dial) struct Counting;

    impl Player for Counting {
        fn play(&mut self, _wav: &'static [u8]) {
            bump(&REQUESTS);
        }

        fn play_long(&mut self, _wav: &'static [u8]) {
            bump(&EGGS);
        }

        fn stop(&mut self) {
            bump(&STOPS);
        }
    }

    /// Clicks requested on this thread since the last `reset`.
    pub(in crate::dial) fn requests() -> usize {
        REQUESTS.with(Cell::get)
    }

    /// Jingles requested on this thread since the last `reset`.
    pub(in crate::dial) fn egg_requests() -> usize {
        EGGS.with(Cell::get)
    }

    /// Stop requests on this thread since the last `reset`.
    pub(in crate::dial) fn stops() -> usize {
        STOPS.with(Cell::get)
    }

    pub(in crate::dial) fn reset() {
        for counter in [&REQUESTS, &EGGS, &STOPS] {
            counter.with(|n| n.set(0));
        }
    }
}

/// Decides which crossings are heard and hands them to the player.
pub(super) struct Clicker {
    enabled: bool,
    closing: bool,
    last: Option<Instant>,
    egg: EggDetector,
    /// When the jingle that was started ends.
    egg_until: Option<Instant>,
    player: Box<dyn Player>,
}

impl Clicker {
    /// `percent` is the value the dial opens on.
    pub(super) fn new(enabled: bool, percent: u32, player: Box<dyn Player>) -> Self {
        Self {
            enabled,
            closing: false,
            last: None,
            egg: EggDetector::new(percent),
            egg_until: None,
            player,
        }
    }

    /// The window is going away: no more clicks, and the jingle stops.
    pub(super) fn close(&mut self) {
        self.closing = true;
        self.stop_egg();
    }

    /// The dial committed `percent` at `now`; true when this completed the easter egg pattern and
    /// the jingle was started.
    pub(super) fn committed(&mut self, percent: u32, now: Instant) -> bool {
        if !self.enabled || self.closing || !self.egg.observe(percent, now) {
            return false;
        }
        self.player.play_long(jingle_wav());
        self.egg_until = Some(now + JINGLE_LENGTH);
        true
    }

    fn egg_playing(&self, now: Instant) -> bool {
        self.egg_until.is_some_and(|until| now < until)
    }

    /// Stops the jingle if it is still playing; a finished one is not ours to stop any more.
    fn stop_egg(&mut self) {
        if let Some(until) = self.egg_until.take()
            && Instant::now() < until
        {
            self.player.stop();
        }
    }

    /// One detent was crossed at `now`; true when the click was played.
    pub(super) fn click(&mut self, now: Instant) -> bool {
        if !self.enabled || self.closing || self.egg_playing(now) {
            return false;
        }
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < MIN_GAP)
        {
            return false;
        }
        self.last = Some(now);
        self.player.play(click_wav());
        true
    }
}

/// Safety net for a window that goes away without `close`.
impl Drop for Clicker {
    fn drop(&mut self) {
        self.stop_egg();
    }
}

/// The click as a complete WAV file, built on first use.
pub(super) fn click_wav() -> &'static [u8] {
    static WAV: OnceLock<Vec<u8>> = OnceLock::new();
    WAV.get_or_init(|| wav_file(&click_samples()))
}

fn click_samples() -> Vec<i16> {
    let count = (SAMPLE_RATE as f32 * CLICK_SECS).round() as usize;
    let mut noise = NOISE_SEED;
    let raw: Vec<f32> = (0..count)
        .map(|n| {
            let t = n as f32 / SAMPLE_RATE as f32;
            let tone = (TAU * TONE_HZ * t).sin();
            let burst = TONE_SHARE * tone + NOISE_SHARE * next_noise(&mut noise);
            burst * (-t / DECAY_SECS).exp() * fade(n, count)
        })
        .collect();
    let loudest = raw.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let gain = PEAK_SHARE * f32::from(i16::MAX) / loudest;
    raw.iter().map(|s| (s * gain).round() as i16).collect()
}

/// A white noise sample in [-1, 1) from a xorshift generator, so the click is the same every run.
fn next_noise(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// 0 at both ends of the buffer, 1 in the middle.
fn fade(n: usize, count: usize) -> f32 {
    let attack = (n as f32 / ATTACK_SAMPLES as f32).min(1.0);
    let taper = ((count - 1 - n) as f32 / TAPER_SAMPLES as f32).min(1.0);
    attack * taper
}

/// A mono 16-bit PCM WAV file around `samples`.
pub(super) fn wav_file(samples: &[i16]) -> Vec<u8> {
    let data_len = samples.len() as u32 * BYTES_PER_SAMPLE;
    let mut wav = Vec::with_capacity(WAV_HEADER_LEN + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(WAV_HEADER_LEN as u32 - 8 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * BYTES_PER_SAMPLE).to_le_bytes());
    wav.extend_from_slice(&(BYTES_PER_SAMPLE as u16).to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        wav.extend_from_slice(&s.to_le_bytes());
    }
    wav
}

#[cfg(test)]
mod tests;
