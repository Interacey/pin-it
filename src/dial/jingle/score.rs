//! The notes: MIDI numbers (69 is A4 = 440 Hz), start and length in sixteenth notes. Bar 1 is the
//! instrumental groove, bar 2 the first vocal phrase as lead, bar 3 starts its resolution.

use super::{TOTAL_UNITS, Voice};

/// One note: MIDI pitch, start and length in sixteenth notes.
#[derive(Debug, Clone, Copy)]
pub(super) struct Note {
    pub(super) midi: u8,
    pub(super) start: u16,
    pub(super) len: u16,
    pub(super) voice: Voice,
}

impl Note {
    pub(super) fn hz(&self) -> f32 {
        midi_hz(self.midi)
    }
}

pub(super) fn midi_hz(midi: u8) -> f32 {
    440.0 * 2.0f32.powf((f32::from(midi) - 69.0) / 12.0)
}

pub(super) const fn note(voice: Voice, midi: u8, start: u16, len: u16) -> Note {
    Note {
        midi,
        start,
        len,
        voice,
    }
}

const AB4: u8 = 68;
const BB4: u8 = 70;
const DB5: u8 = 73;
const EB5: u8 = 75;
const F5: u8 = 77;

/// Scale degrees 1 2 4 2 6 6 5 of A-flat major: four eighths, two dotted eighths, a held note.
pub(super) const LEAD: [Note; 7] = [
    note(Voice::Lead, AB4, 16, 2),
    note(Voice::Lead, BB4, 18, 2),
    note(Voice::Lead, DB5, 20, 2),
    note(Voice::Lead, BB4, 22, 2),
    note(Voice::Lead, F5, 24, 3),
    note(Voice::Lead, F5, 27, 3),
    note(Voice::Lead, EB5, 30, TOTAL_UNITS - 30),
];

/// A chord for `len` sixteenths: bass root and the three tones the stabs play.
pub(super) struct Chord {
    pub(super) start: u16,
    pub(super) len: u16,
    pub(super) root: u8,
    pub(super) tones: [u8; 3],
}

const HALF_BAR: u16 = 8;

/// Fm7, Bbm7, Ab, Bbm7 and Eb, one chord per half bar.
pub(super) const CHORDS: [Chord; 5] = [
    chord(0, HALF_BAR, 41, [56, 60, 63]),
    chord(8, HALF_BAR, 46, [61, 65, 68]),
    chord(16, HALF_BAR, 44, [56, 60, 63]),
    chord(24, HALF_BAR, 46, [61, 65, 68]),
    chord(32, TOTAL_UNITS - 32, 51, [55, 58, 63]),
];

const fn chord(start: u16, len: u16, root: u8, tones: [u8; 3]) -> Chord {
    Chord {
        start,
        len,
        root,
        tones,
    }
}

/// Bass bounce inside a chord: (offset, semitones above the root, length). Low, high, low, high,
/// with the high notes pushed off the beat.
const BASS_BOUNCE: [(u16, u8, u16); 4] = [(0, 0, 2), (3, 12, 1), (4, 0, 2), (7, 12, 1)];
/// Chord stabs inside a chord: (offset, length). One on the beat, one pushed ahead of the next.
const STAB_HITS: [(u16, u16); 2] = [(0, 3), (6, 2)];

/// Every pitched note of the jingle; what would run past its chord is left out.
pub(super) fn score() -> Vec<Note> {
    let mut notes = LEAD.to_vec();
    for chord in &CHORDS {
        add_pattern(chord, &mut notes);
    }
    notes
}

fn add_pattern(chord: &Chord, notes: &mut Vec<Note>) {
    let end = chord.start + chord.len;
    for (offset, up, len) in BASS_BOUNCE {
        if chord.start + offset + len <= end {
            notes.push(note(
                Voice::Bass,
                chord.root + up,
                chord.start + offset,
                len,
            ));
        }
    }
    for (offset, len) in STAB_HITS {
        if chord.start + offset + len <= end {
            for tone in chord.tones {
                notes.push(note(Voice::Stab, tone, chord.start + offset, len));
            }
        }
    }
}
