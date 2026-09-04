//! Deterministic note sequencer for procedural music.
//!
//! A [`Melody`] is a fixed list of [`Note`]s inside a loop length; a
//! [`Sequencer`] advances a clock and emits the notes whose beat falls inside
//! each tick — including across loop wraps — so games can drive
//! [`Audio`](crate::Audio) channels with sample-accurate-ish, replay-stable
//! music without any audio assets. Works identically native and web.

/// One scheduled tone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    /// Beat offset inside the loop, seconds.
    pub at: f32,
    /// Frequency in Hz.
    pub frequency: f32,
    /// Duration in seconds.
    pub duration: f32,
    /// Volume in `0..=1` before the mixer channel scales it.
    pub volume: f32,
}

impl Note {
    pub const fn new(at: f32, frequency: f32, duration: f32, volume: f32) -> Self {
        Self {
            at,
            frequency,
            duration,
            volume,
        }
    }
}

/// A looping list of notes. `length` must be positive.
#[derive(Debug, Clone, PartialEq)]
pub struct Melody {
    pub length: f32,
    pub notes: Vec<Note>,
}

impl Melody {
    pub fn new(length: f32, notes: Vec<Note>) -> Self {
        Self { length, notes }
    }
}

/// Emits melody notes as the clock advances. Deterministic: the same tick
/// schedule always produces the same note stream.
#[derive(Debug, Clone)]
pub struct Sequencer {
    melody: Melody,
    clock: f32,
}

impl Sequencer {
    pub fn new(melody: Melody) -> Self {
        let clock = 0.0;
        Self { melody, clock }
    }

    pub fn clock(&self) -> f32 {
        self.clock
    }

    pub fn melody(&self) -> &Melody {
        &self.melody
    }

    /// Advances by `dt` and returns every note whose beat falls inside the
    /// elapsed window, in melody order. Loop wraps emit the tail then the
    /// head. A zero or negative `dt` emits nothing.
    pub fn tick(&mut self, dt: f32) -> Vec<Note> {
        if dt <= 0.0 || self.melody.length <= f32::EPSILON || self.melody.notes.is_empty() {
            return Vec::new();
        }
        let length = self.melody.length;
        let mut start = self.clock;
        let mut end = start + dt;
        let mut due = Vec::new();
        while end >= length {
            // Notes in (start, length].
            for note in &self.melody.notes {
                if note.at > start && note.at <= length {
                    due.push(*note);
                }
            }
            end -= length;
            start = 0.0;
        }
        for note in &self.melody.notes {
            if note.at > start && note.at <= end {
                due.push(*note);
            }
        }
        self.clock = end % length;
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn melody() -> Melody {
        Melody::new(
            4.0,
            vec![
                Note::new(0.5, 220.0, 0.2, 0.1),
                Note::new(2.0, 330.0, 0.2, 0.1),
                Note::new(3.9, 440.0, 0.2, 0.1),
            ],
        )
    }

    #[test]
    fn emits_notes_inside_the_elapsed_window_only() {
        let mut seq = Sequencer::new(melody());
        assert!(seq.tick(0.4).is_empty(), "beat 0.5 not reached yet");
        let due = seq.tick(0.2);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].frequency, 220.0);
        assert!(seq.tick(1.0).is_empty());
        let due = seq.tick(1.5);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].frequency, 330.0);
    }

    #[test]
    fn loop_wraps_emit_tail_then_head_in_order() {
        let mut seq = Sequencer::new(melody());
        seq.tick(3.0); // clock 3.0, beat 3.9 still ahead
        let due = seq.tick(1.5); // crosses the wrap: 3.9 then 0.5
        let frequencies: Vec<f32> = due.iter().map(|note| note.frequency).collect();
        assert_eq!(frequencies, vec![440.0, 220.0]);
        assert!(
            (seq.clock() - 0.5).abs() < 1e-4,
            "clock lands past the wrap"
        );
    }

    #[test]
    fn identical_tick_schedules_produce_identical_streams() {
        let mut a = Sequencer::new(melody());
        let mut b = Sequencer::new(melody());
        for dt in [0.1_f32, 0.25, 0.4, 0.05, 1.0, 2.0, 0.75] {
            let notes_a = a.tick(dt);
            let notes_b = b.tick(dt);
            assert_eq!(notes_a, notes_b);
            assert_eq!(a.clock(), b.clock());
        }
    }

    #[test]
    fn zero_and_negative_ticks_emit_nothing() {
        let mut seq = Sequencer::new(melody());
        assert!(seq.tick(0.0).is_empty());
        assert!(seq.tick(-1.0).is_empty());
        assert_eq!(seq.clock(), 0.0);
    }

    #[test]
    fn long_ticks_span_multiple_wraps() {
        let mut seq = Sequencer::new(melody());
        let due = seq.tick(9.5); // 2.375 loops
        let count = due.len();
        assert_eq!(count, 7, "two full loops (3+3) plus the partial (1)");
        assert_eq!(seq.clock(), 1.5);
    }
}
