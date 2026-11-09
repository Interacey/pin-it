use std::cell::RefCell;
use std::rc::Rc;

use super::*;

/// Records each request with the address of the buffer it was given.
struct Recording(Rc<RefCell<Vec<usize>>>);

impl Player for Recording {
    fn play(&mut self, wav: &'static [u8]) {
        self.0.borrow_mut().push(wav.as_ptr() as usize);
    }

    fn play_long(&mut self, _wav: &'static [u8]) {}

    fn stop(&mut self) {}
}

fn clicker(enabled: bool) -> (Clicker, Rc<RefCell<Vec<usize>>>) {
    let log = Rc::new(RefCell::new(Vec::new()));
    (
        Clicker::new(enabled, 100, Box::new(Recording(log.clone()))),
        log,
    )
}

fn u16_at(wav: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([wav[at], wav[at + 1]])
}

fn u32_at(wav: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([wav[at], wav[at + 1], wav[at + 2], wav[at + 3]])
}

fn samples(wav: &[u8]) -> Vec<i16> {
    wav[WAV_HEADER_LEN..]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect()
}

#[test]
fn the_buffer_is_a_valid_riff_wave_header() {
    let wav = click_wav();
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..16], b"WAVEfmt ");
    assert_eq!(u32_at(wav, 4) as usize, wav.len() - 8, "RIFF size");
    assert_eq!(u32_at(wav, 16), 16, "fmt chunk size");
    assert_eq!(u16_at(wav, 20), 1, "PCM");
    assert_eq!(u16_at(wav, 22), 1, "mono");
    assert_eq!(u32_at(wav, 24), 22_050, "sample rate");
    assert_eq!(u32_at(wav, 28), 44_100, "byte rate");
    assert_eq!(u16_at(wav, 32), 2, "block align");
    assert_eq!(u16_at(wav, 34), 16, "bits");
    assert_eq!(&wav[36..40], b"data");
    assert_eq!(
        u32_at(wav, 40) as usize,
        wav.len() - WAV_HEADER_LEN,
        "data size"
    );
}

#[test]
fn the_click_lasts_about_three_milliseconds() {
    let n = samples(click_wav()).len() as f32;
    let ms = n / SAMPLE_RATE as f32 * 1000.0;
    assert!((2.9..=3.1).contains(&ms), "{ms} ms");
}

#[test]
fn the_click_starts_and_ends_near_zero_and_peaks_between_25_and_45_percent() {
    let s = samples(click_wav());
    let full = f32::from(i16::MAX);
    assert!(f32::from(s[0]).abs() < 0.02 * full, "first {}", s[0]);
    assert!(
        f32::from(s[s.len() - 1]).abs() < 0.02 * full,
        "last {}",
        s[s.len() - 1]
    );
    let peak = s.iter().map(|v| f32::from(*v).abs()).fold(0.0, f32::max) / full;
    assert!((0.25..=0.45).contains(&peak), "peak {peak}");
    assert!(
        s.iter().any(|&v| v > 0) && s.iter().any(|&v| v < 0),
        "no DC-only buffer"
    );
}

#[test]
fn the_buffer_is_built_once() {
    assert_eq!(click_wav().as_ptr(), click_wav().as_ptr());
}

#[test]
fn a_click_is_requested_with_the_shared_buffer() {
    let (mut c, log) = clicker(true);
    assert!(c.click(Instant::now()));
    assert_eq!(*log.borrow(), vec![click_wav().as_ptr() as usize]);
}

#[test]
fn clicks_spaced_by_the_gap_are_all_played_one_per_crossing() {
    let (mut c, log) = clicker(true);
    let start = Instant::now();
    for i in 0..5 {
        assert!(c.click(start + MIN_GAP * i));
    }
    assert_eq!(log.borrow().len(), 5);
}

#[test]
fn at_most_25_clicks_per_second() {
    let (mut c, log) = clicker(true);
    let start = Instant::now();
    // A crossing every millisecond for one second.
    let played = (0..1000u32)
        .filter(|&ms| c.click(start + Duration::from_millis(u64::from(ms))))
        .count();
    assert_eq!(played, 25);
    assert_eq!(log.borrow().len(), 25);
}

#[test]
fn a_click_inside_the_gap_is_dropped_and_does_not_move_the_gap() {
    let (mut c, _) = clicker(true);
    let t = Instant::now();
    assert!(c.click(t));
    assert!(!c.click(t + Duration::from_millis(39)));
    assert!(c.click(t + Duration::from_millis(40)));
}

#[test]
fn nothing_is_requested_when_the_setting_is_off() {
    let (mut c, log) = clicker(false);
    assert!(!c.click(Instant::now()));
    assert!(log.borrow().is_empty());
}

#[test]
fn nothing_is_requested_once_the_window_is_closing() {
    let (mut c, log) = clicker(true);
    c.close();
    assert!(!c.click(Instant::now()));
    assert!(log.borrow().is_empty());
}

#[test]
fn the_system_player_is_a_player() {
    let _player: Box<dyn Player> = Box::new(SystemPlayer);
}

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Click,
    Egg(usize),
    Stop,
}

/// Records clicks, jingles and stops in order.
struct Calls(Rc<RefCell<Vec<Call>>>);

impl Player for Calls {
    fn play(&mut self, _wav: &'static [u8]) {
        self.0.borrow_mut().push(Call::Click);
    }

    fn play_long(&mut self, wav: &'static [u8]) {
        self.0.borrow_mut().push(Call::Egg(wav.as_ptr() as usize));
    }

    fn stop(&mut self) {
        self.0.borrow_mut().push(Call::Stop);
    }
}

fn eggy(enabled: bool) -> (Clicker, Rc<RefCell<Vec<Call>>>) {
    let log = Rc::new(RefCell::new(Vec::new()));
    (
        Clicker::new(enabled, 100, Box::new(Calls(log.clone()))),
        log,
    )
}

/// Home, End, Home within a second, starting at `t`.
fn round_trip(c: &mut Clicker, t: Instant) -> Vec<bool> {
    [(50, 0), (150, 100), (50, 200)]
        .iter()
        .map(|&(p, ms)| c.committed(p, t + Duration::from_millis(ms)))
        .collect()
}

#[test]
fn the_round_trip_starts_the_jingle_once_with_its_buffer() {
    let (mut c, log) = eggy(true);
    assert_eq!(round_trip(&mut c, Instant::now()), [false, false, true]);
    assert_eq!(*log.borrow(), [Call::Egg(jingle_wav().as_ptr() as usize)]);
}

#[test]
fn no_jingle_when_the_setting_is_off() {
    let (mut c, log) = eggy(false);
    assert_eq!(round_trip(&mut c, Instant::now()), [false; 3]);
    assert!(log.borrow().is_empty());
}

#[test]
fn no_jingle_once_the_window_is_closing() {
    let (mut c, log) = eggy(true);
    c.close();
    assert_eq!(round_trip(&mut c, Instant::now()), [false; 3]);
    assert!(log.borrow().is_empty());
}

#[test]
fn clicks_are_suppressed_while_the_jingle_plays_and_come_back_after() {
    let (mut c, log) = eggy(true);
    let t = Instant::now();
    round_trip(&mut c, t);
    let jingle_end = t + Duration::from_millis(200) + JINGLE_LENGTH;
    assert!(!c.click(t + Duration::from_millis(300)));
    assert!(!c.click(jingle_end - Duration::from_millis(1)));
    assert!(c.click(jingle_end));
    assert_eq!(
        log.borrow().len(),
        2,
        "the jingle and the one click after it"
    );
    assert_eq!(log.borrow()[1], Call::Click);
}

#[test]
fn closing_while_the_jingle_plays_stops_it_once() {
    let (mut c, log) = eggy(true);
    round_trip(&mut c, Instant::now());
    c.close();
    c.close();
    drop(c);
    let stops = log
        .borrow()
        .iter()
        .filter(|call| **call == Call::Stop)
        .count();
    assert_eq!(stops, 1);
}

#[test]
fn closing_without_a_jingle_stops_nothing() {
    let (mut c, log) = eggy(true);
    c.close();
    drop(c);
    assert!(log.borrow().is_empty());
}

#[test]
fn a_jingle_that_has_ended_is_not_stopped() {
    let (mut c, log) = eggy(true);
    // The round trip happened more than five seconds ago.
    round_trip(&mut c, Instant::now() - Duration::from_secs(10));
    c.close();
    assert_eq!(
        log.borrow().len(),
        1,
        "only the jingle itself: {:?}",
        log.borrow()
    );
}
