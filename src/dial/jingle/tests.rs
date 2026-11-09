use super::percussion::{Hit, Kind, add_hit, hits};
use super::score::{CHORDS, LEAD, midi_hz, note, score};
use super::*;

const FULL: f32 = i16::MAX as f32;
const HEADER_LEN: usize = 44;
const LEAD_BAND: (f32, f32) = (350.0, 1000.0);
const LOW_BAND: (f32, f32) = (60.0, 350.0);
const WINDOW_SECS: f32 = 0.25;

fn u32_at(wav: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([wav[at], wav[at + 1], wav[at + 2], wav[at + 3]])
}

fn pcm() -> Vec<i16> {
    jingle_wav()[HEADER_LEN..]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect()
}

fn peak_of(samples: &[i16]) -> f32 {
    samples
        .iter()
        .map(|&v| f32::from(v).abs())
        .fold(0.0, f32::max)
}

fn rendered(note: Note) -> Vec<f32> {
    let mut mix = vec![0.0; SAMPLE_COUNT];
    add_note(&note, &mut mix);
    mix
}

fn rendered_hit(kind: Kind, start: u16) -> Vec<f32> {
    let mut mix = vec![0.0; SAMPLE_COUNT];
    add_hit(&Hit { start, kind }, &mut mix);
    mix
}

fn sign_changes(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|w| (w[0] > 0.0) != (w[1] > 0.0))
        .count()
}

fn peak_f32(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |m, s| m.max(s.abs()))
}

fn at_sample(secs: f32) -> usize {
    (secs * SAMPLE_RATE as f32).round() as usize
}

fn unit_secs(units: u16) -> f32 {
    f32::from(units) * UNIT_SECS
}

/// Pitch of a steady note from its sign changes between 0.1 s and 0.9 s.
fn measured_hz(mix: &[f32]) -> f32 {
    let from = SAMPLE_RATE as usize / 10;
    let to = SAMPLE_RATE as usize * 9 / 10;
    sign_changes(&mix[from..to]) as f32 / 2.0 / 0.8
}

/// The frequency in `band` (1 Hz steps) with the most energy in `samples`, by a plain DFT.
fn dominant_hz(samples: &[f32], band: (f32, f32)) -> f32 {
    let rate = SAMPLE_RATE as f32;
    let strength = |hz: f32| {
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (n, s) in samples.iter().enumerate() {
            let angle = TAU * hz * n as f32 / rate;
            re += s * angle.cos();
            im += s * angle.sin();
        }
        re * re + im * im
    };
    (band.0 as u32..=band.1 as u32)
        .map(|hz| hz as f32)
        .max_by(|a, b| strength(*a).total_cmp(&strength(*b)))
        .unwrap()
}

fn pcm_f32() -> Vec<f32> {
    pcm().iter().map(|&v| f32::from(v)).collect()
}

fn overlap(a: &Note, b: &Note) -> bool {
    a.start < b.start + b.len && b.start < a.start + a.len
}

#[test]
fn the_buffer_is_a_valid_riff_wave_of_five_seconds() {
    let wav = jingle_wav();
    let data_len = 5 * 22_050 * 2;
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..16], b"WAVEfmt ");
    assert_eq!(u32_at(wav, 4) as usize, wav.len() - 8, "RIFF size");
    assert_eq!(u32_at(wav, 24), 22_050, "sample rate");
    assert_eq!(&wav[36..40], b"data");
    assert_eq!(u32_at(wav, 40) as usize, data_len, "data size");
    assert_eq!(wav.len(), HEADER_LEN + data_len);
    assert_eq!(pcm().len() as f32 / SAMPLE_RATE as f32, 5.0);
}

#[test]
fn the_peak_is_a_quarter_of_full_scale() {
    let share = peak_of(&pcm()) / FULL;
    assert!((0.20..=0.30).contains(&share), "peak {share}");
}

#[test]
fn it_starts_and_ends_at_zero() {
    let s = pcm();
    assert_eq!(s[0], 0);
    assert!(s[s.len() - 1].abs() <= 1, "last {}", s[s.len() - 1]);
}

#[test]
fn the_last_fifth_of_a_second_fades_out() {
    let s = pcm();
    let tail = |from_end_ms: usize| {
        let n = from_end_ms * SAMPLE_RATE as usize / 1000;
        peak_of(&s[s.len() - n..])
    };
    let peak = peak_of(&s);
    assert!(tail(10) < 0.06 * peak, "last 10 ms {}", tail(10));
    assert!(tail(100) < 0.55 * peak, "last 100 ms {}", tail(100));
    // Right before the fade starts a note is still at full strength.
    let before = peak_of(
        &s[s.len() - 400 * SAMPLE_RATE as usize / 1000
            ..s.len() - 200 * SAMPLE_RATE as usize / 1000],
    );
    assert!(before > 0.5 * peak, "before the fade {before}");
}

#[test]
fn the_buffer_is_built_once() {
    assert_eq!(jingle_wav().as_ptr(), jingle_wav().as_ptr());
}

#[test]
fn the_unit_is_a_sixteenth_at_the_tempo_and_the_units_fill_the_length() {
    let beats_per_minute = 60.0 / (UNIT_SECS * 4.0);
    assert!(
        (beats_per_minute - TEMPO_BPM).abs() < 1e-3,
        "{beats_per_minute}"
    );
    assert!((112.0..=114.0).contains(&TEMPO_BPM));
    let secs = LENGTH.as_secs_f32();
    assert!(unit_secs(TOTAL_UNITS) <= secs);
    assert!(
        unit_secs(TOTAL_UNITS + 1) > secs,
        "one more unit would still fit"
    );
}

#[test]
fn the_score_has_a_lead_a_bass_and_stabs_inside_the_length() {
    let notes = score();
    for voice in [Voice::Lead, Voice::Bass, Voice::Stab] {
        assert!(notes.iter().any(|n| n.voice == voice), "no {voice:?}");
    }
    for n in &notes {
        assert!(
            n.start + n.len <= TOTAL_UNITS,
            "{n:?} ends after the jingle"
        );
        assert!((70.0..=1000.0).contains(&n.hz()), "{n:?} out of range");
        assert!(
            unit_secs(n.len) > ATTACK_SECS + DECAY_SECS,
            "{n:?} is shorter than its attack and decay"
        );
    }
}

#[test]
fn lead_and_bass_play_one_note_at_a_time_and_no_pitch_overlaps_itself() {
    let notes = score();
    for (i, a) in notes.iter().enumerate() {
        for b in &notes[i + 1..] {
            let same_voice = a.voice == b.voice;
            let single = a.voice != Voice::Stab;
            if same_voice && (single || a.midi == b.midi) {
                assert!(!overlap(a, b), "{a:?} overlaps {b:?}");
            }
        }
    }
}

#[test]
fn the_lead_is_the_phrase_one_two_four_two_six_six_five_of_a_flat_major() {
    let pitches: Vec<u8> = LEAD.iter().map(|n| n.midi).collect();
    assert_eq!(pitches, [68, 70, 73, 70, 77, 77, 75]);
    let lengths: Vec<u16> = LEAD.iter().take(6).map(|n| n.len).collect();
    assert_eq!(lengths, [2, 2, 2, 2, 3, 3]);
    assert!((midi_hz(69) - 440.0).abs() < 1e-3);
    assert!((midi_hz(81) - 880.0).abs() < 1e-2);
    assert_eq!(CHORDS[0].start, 0);
}

#[test]
fn a_lead_note_has_the_pitch_it_is_written_at() {
    for midi in [68, 73, 77] {
        let hz = midi_hz(midi);
        let measured = measured_hz(&rendered(note(Voice::Lead, midi, 0, 8)));
        assert!(
            (measured - hz).abs() / hz < 0.01,
            "{hz} Hz measured as {measured}"
        );
    }
}

#[test]
fn a_bass_note_has_the_pitch_it_is_written_at() {
    let hz = midi_hz(46);
    let measured = measured_hz(&rendered(note(Voice::Bass, 46, 0, 8)));
    assert!((measured - hz).abs() / hz < 0.01, "measured {measured}");
}

#[test]
fn a_stab_note_has_the_pitch_it_is_written_at() {
    let hz = midi_hz(60);
    let measured = measured_hz(&rendered(note(Voice::Stab, 60, 0, 8)));
    assert!((measured - hz).abs() / hz < 0.01, "measured {measured}");
}

#[test]
fn the_mixed_jingle_sounds_every_lead_note_at_its_pitch() {
    let mix = pcm_f32();
    for n in &LEAD {
        let from = at_sample(unit_secs(n.start) + 0.05);
        let to = at_sample(unit_secs(n.start + n.len.min(3)) - 0.03);
        let measured = dominant_hz(&mix[from..to], LEAD_BAND);
        assert!(
            (measured - n.hz()).abs() / n.hz() < 0.02,
            "{n:?}: {measured} Hz"
        );
    }
}

#[test]
fn the_vibrato_starts_after_the_delay_without_a_jump() {
    let hz = midi_hz(75);
    let steady = |t: f32| (hz * t).fract();
    assert_eq!(phase(Voice::Lead, hz, 0.2), steady(0.2));
    let dt = 1e-4;
    let at_delay = phase(Voice::Lead, hz, VIBRATO_DELAY_SECS);
    let just_after = phase(Voice::Lead, hz, VIBRATO_DELAY_SECS + dt);
    let step = (just_after - at_delay).rem_euclid(1.0);
    assert!(
        (step - hz * dt).abs() < 0.02,
        "phase jumps by {step} instead of {}",
        hz * dt
    );
    let bends = (0..100).any(|i| {
        let t = VIBRATO_DELAY_SECS + 0.01 * i as f32;
        (phase(Voice::Lead, hz, t) - steady(t)).abs() > 1e-3
    });
    assert!(bends, "the pitch never moves after the delay");
    assert_eq!(
        phase(Voice::Bass, hz, 1.0),
        steady(1.0),
        "only the lead wobbles"
    );
}

#[test]
fn the_vibrato_stays_within_its_depth() {
    let hz = midi_hz(75);
    let dt = 1e-4;
    for i in 0..10_000 {
        let t = VIBRATO_DELAY_SECS + dt * i as f32;
        let mut step = phase(Voice::Lead, hz, t + dt) - phase(Voice::Lead, hz, t);
        step = step.rem_euclid(1.0);
        let local_hz = step / dt;
        assert!(
            (local_hz - hz).abs() <= hz * VIBRATO_DEPTH * 1.2 + 5.0,
            "{local_hz} Hz at {t}"
        );
    }
}

#[test]
fn a_note_sounds_for_its_length_plus_the_release() {
    let mix = rendered(note(Voice::Lead, 69, 8, 4));
    let first = mix.iter().position(|&s| s != 0.0).unwrap();
    let last = mix.iter().rposition(|&s| s != 0.0).unwrap();
    let rate = SAMPLE_RATE as f32;
    let start = 8.0 * UNIT_SECS * rate;
    let end = (8.0 + 4.0) * UNIT_SECS * rate + RELEASE_SECS * rate;
    assert!((first as f32 - start).abs() < 3.0, "starts at {first}");
    assert!((last as f32 - end).abs() < 3.0, "ends at {last}");
}

#[test]
fn the_envelope_rises_decays_holds_and_releases() {
    let held = 1.0;
    assert_eq!(envelope(0.0, held), 0.0);
    assert!(
        (envelope(ATTACK_SECS, held) - 1.0).abs() < 1e-5,
        "peak after the attack"
    );
    assert!(
        (envelope(0.5, held) - SUSTAIN_LEVEL).abs() < 1e-6,
        "sustain"
    );
    let mid_release = envelope(held + RELEASE_SECS / 2.0, held);
    assert!(
        (mid_release - SUSTAIN_LEVEL / 2.0).abs() < 1e-5,
        "{mid_release}"
    );
    assert!(envelope(held + RELEASE_SECS, held) < 1e-5);
    assert_eq!(envelope(held + 1.0, held), 0.0);
}

#[test]
fn the_waves_stay_in_range_and_the_square_has_two_levels() {
    for i in 0..100 {
        let phase = i as f32 / 100.0;
        assert!(Voice::Bass.wave(phase).abs() <= 1.0);
        assert!(Voice::Lead.wave(phase).abs() == 1.0);
    }
    assert_eq!(Voice::Lead.wave(0.25), 1.0);
    assert_eq!(Voice::Lead.wave(0.75), -1.0);
}

#[test]
fn the_percussion_sits_on_the_backbeat_and_offbeats_inside_the_length() {
    let all = hits();
    let snares: Vec<u16> = all
        .iter()
        .filter(|h| h.kind == Kind::Snare)
        .map(|h| h.start)
        .collect();
    assert_eq!(snares, [4, 12, 20, 28, 36]);
    for h in &all {
        assert!(h.start > 0, "{h:?} would start on the first sample");
        let ends = unit_secs(h.start) + h.kind.length_secs();
        assert!(ends <= LENGTH.as_secs_f32(), "{h:?} ends at {ends}");
        if h.kind == Kind::Hat {
            assert_eq!(h.start % 4, 2, "{h:?} is not on an offbeat");
        }
    }
}

#[test]
fn the_snare_is_noise_and_quieter_than_the_lead() {
    let start = 12;
    let burst = rendered_hit(Kind::Snare, start);
    let from = at_sample(unit_secs(start));
    let to = from + at_sample(Kind::Snare.length_secs());
    let crossings_per_sec = sign_changes(&burst[from..to]) as f32 / Kind::Snare.length_secs();
    let lead = rendered(note(Voice::Lead, 77, 0, 8));
    let lead_rate = sign_changes(&lead[from..to]) as f32 / Kind::Snare.length_secs();
    assert!(
        crossings_per_sec > 8000.0,
        "snare {crossings_per_sec} per s"
    );
    assert!(lead_rate < 2000.0, "lead {lead_rate} per s");
    assert!(
        peak_f32(&burst) < 0.5 * peak_f32(&lead),
        "snare {} against lead {}",
        peak_f32(&burst),
        peak_f32(&lead)
    );
}

#[test]
fn the_hat_is_shorter_and_quieter_than_the_snare() {
    let snare = rendered_hit(Kind::Snare, 4);
    let hat = rendered_hit(Kind::Hat, 4);
    assert!(peak_f32(&hat) < 0.5 * peak_f32(&snare));
    let sounding = |mix: &[f32]| mix.iter().filter(|s| s.abs() > 1e-3).count();
    assert!(sounding(&hat) < sounding(&snare));
}

/// Writes the WAV to `PIN_IT_JINGLE_OUT` and prints the dominant pitch of every quarter second.
/// Run with `PIN_IT_JINGLE_OUT=<file> cargo test --lib jingle::tests::render -- --ignored --nocapture`.
#[test]
#[ignore = "writes a file; needs PIN_IT_JINGLE_OUT"]
fn render_the_wav_and_report_the_pitch_per_window() {
    let path = std::env::var("PIN_IT_JINGLE_OUT").expect("PIN_IT_JINGLE_OUT is not set");
    std::fs::write(&path, jingle_wav()).unwrap();
    let mix = pcm_f32();
    println!(
        "{} bytes, peak {} of {FULL}",
        jingle_wav().len(),
        peak_of(&pcm())
    );
    let step = at_sample(WINDOW_SECS);
    for (i, window) in mix.chunks_exact(step).enumerate() {
        let lead = dominant_hz(window, LEAD_BAND);
        let low = dominant_hz(window, LOW_BAND);
        println!(
            "{:.2}-{:.2} s: lead band {lead:.0} Hz, low band {low:.0} Hz",
            i as f32 * WINDOW_SECS,
            (i + 1) as f32 * WINDOW_SECS
        );
    }
}
