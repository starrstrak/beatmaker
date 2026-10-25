//! Track quantiser: snaps note starts to a grid with adjustable strength and swing.
//! Note lengths are preserved (the note-off moves with its note-on).

use crate::clip::{Clip, TimedEvent};
use crate::midi::MidiEvent;
use crate::PPQ;
use serde::{Deserialize, Serialize};

/// Grid steps offered by the UI, from coarse to fine. `1/128` is the finest step.
pub const DIVISIONS: [u16; 8] = [1, 2, 4, 8, 16, 32, 64, 128];

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuantizeSettings {
    /// Note value denominator: 4 = quarter notes, 16 = sixteenth notes, 128 = 1/128 notes.
    pub division: u16,
    /// Triplet grid (3 steps in the time of 2).
    pub triplet: bool,
    /// 0 = untouched, 1 = fully on the grid.
    pub strength: f32,
    /// Position of the off-beat inside each pair of steps: 0.5 = straight, 0.667 = triplet swing.
    pub swing: f32,
}

impl Default for QuantizeSettings {
    fn default() -> Self {
        Self {
            division: 16,
            triplet: false,
            strength: 1.0,
            swing: 0.5,
        }
    }
}

impl QuantizeSettings {
    /// Grid step in ticks (never below 1).
    pub fn step_ticks(&self) -> f64 {
        let d = self.division.clamp(1, 128) as f64;
        let base = PPQ as f64 * 4.0 / d;
        if self.triplet {
            base * 2.0 / 3.0
        } else {
            base
        }
    }

    /// The grid point nearest to `tick`, honouring swing.
    pub fn snap(&self, tick: f64) -> f64 {
        let step = self.step_ticks();
        let pair = step * 2.0;
        let swing = self.swing.clamp(0.5, 0.75) as f64;
        let base = (tick / pair).floor() * pair;
        let candidates = [base, base + swing * pair, base + pair];
        candidates
            .into_iter()
            .min_by(|a, b| (a - tick).abs().total_cmp(&(b - tick).abs()))
            .unwrap_or(tick)
    }
}

/// Returns a quantised copy of `clip`. Non-note events keep their position.
pub fn quantize_clip(clip: &Clip, q: &QuantizeSettings) -> Clip {
    let strength = q.strength.clamp(0.0, 1.0) as f64;
    // Pair each NoteOn with the NoteOff that ends it (same channel and key, FIFO).
    let n = clip.events.len();
    let mut shift = vec![0.0f64; n];
    let mut open: Vec<Vec<usize>> = vec![Vec::new(); 128 * 16];
    for (i, te) in clip.events.iter().enumerate() {
        match te.event {
            MidiEvent::NoteOn { channel, note, .. } => {
                let t = te.tick as f64;
                shift[i] = (q.snap(t) - t) * strength;
                open[channel as usize * 128 + note as usize].push(i);
            }
            MidiEvent::NoteOff { channel, note, .. } => {
                let slot = &mut open[channel as usize * 128 + note as usize];
                if !slot.is_empty() {
                    shift[i] = shift[slot.remove(0)];
                }
            }
            _ => {}
        }
    }
    let mut events: Vec<TimedEvent> = clip
        .events
        .iter()
        .zip(&shift)
        .map(|(te, s)| TimedEvent {
            tick: (te.tick as f64 + s).round().max(0.0) as u64,
            event: te.event,
        })
        .collect();
    // Note-offs sort before note-ons at the same tick so repeated notes retrigger cleanly.
    events.sort_by_key(|e| (e.tick, matches!(e.event, MidiEvent::NoteOn { .. })));
    let last = events.last().map(|e| e.tick).unwrap_or(0);
    Clip {
        events,
        length_ticks: clip.length_ticks.max(last),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip_of(notes: &[(u64, u64, u8)]) -> Clip {
        let mut events = Vec::new();
        for &(on, off, n) in notes {
            events.push(TimedEvent {
                tick: on,
                event: MidiEvent::NoteOn {
                    channel: 0,
                    note: n,
                    velocity: 100,
                },
            });
            events.push(TimedEvent {
                tick: off,
                event: MidiEvent::NoteOff {
                    channel: 0,
                    note: n,
                    velocity: 0,
                },
            });
        }
        events.sort_by_key(|e| e.tick);
        Clip {
            events,
            length_ticks: 3840,
        }
    }

    #[test]
    fn steps_cover_down_to_one_128th() {
        let q = QuantizeSettings {
            division: 128,
            ..Default::default()
        };
        assert_eq!(q.step_ticks(), 30.0);
        assert_eq!(
            QuantizeSettings {
                division: 4,
                ..Default::default()
            }
            .step_ticks(),
            960.0
        );
        assert_eq!(
            QuantizeSettings {
                division: 8,
                triplet: true,
                ..Default::default()
            }
            .step_ticks(),
            320.0
        );
    }

    #[test]
    fn snaps_to_sixteenths_and_keeps_length() {
        let c = clip_of(&[(250, 450, 60), (1000, 1100, 62)]);
        let out = quantize_clip(&c, &QuantizeSettings::default());
        let notes = out.notes();
        assert_eq!((notes[0].start_tick, notes[0].end_tick), (240, 440));
        assert_eq!((notes[1].start_tick, notes[1].end_tick), (960, 1060));
    }

    #[test]
    fn strength_moves_partway() {
        let c = clip_of(&[(250, 300, 60)]);
        let q = QuantizeSettings {
            strength: 0.5,
            ..Default::default()
        };
        assert_eq!(quantize_clip(&c, &q).notes()[0].start_tick, 245);
        let q0 = QuantizeSettings {
            strength: 0.0,
            ..Default::default()
        };
        assert_eq!(quantize_clip(&c, &q0).notes()[0].start_tick, 250);
    }

    #[test]
    fn swing_delays_offbeats() {
        // 1/16 grid: step 240, pair 480. Straight off-beat at 240, swung at 0.667*480 = 320.
        let c = clip_of(&[(235, 300, 60)]);
        let q = QuantizeSettings {
            swing: 2.0 / 3.0,
            ..Default::default()
        };
        assert_eq!(quantize_clip(&c, &q).notes()[0].start_tick, 320);
        assert_eq!(
            quantize_clip(&c, &QuantizeSettings::default()).notes()[0].start_tick,
            240
        );
    }

    #[test]
    fn fine_grid_1_128_is_nearly_transparent() {
        let c = clip_of(&[(247, 400, 60)]);
        let q = QuantizeSettings {
            division: 128,
            ..Default::default()
        };
        assert_eq!(quantize_clip(&c, &q).notes()[0].start_tick, 240);
    }

    #[test]
    fn keeps_other_events_and_is_idempotent() {
        let mut c = clip_of(&[(250, 450, 60)]);
        c.events.push(TimedEvent {
            tick: 333,
            event: MidiEvent::PitchBend { channel: 0, value: 100 },
        });
        c.events.sort_by_key(|e| e.tick);
        let once = quantize_clip(&c, &QuantizeSettings::default());
        assert!(once.events.iter().any(|e| e.tick == 333));
        let twice = quantize_clip(&once, &QuantizeSettings::default());
        assert_eq!(once, twice);
    }

    #[test]
    fn same_note_repeated_orders_off_before_on() {
        // Both starts snap to tick 0 and 960 respectively; the first note's off lands before the second on.
        let c = clip_of(&[(10, 500, 60), (950, 1100, 60)]);
        let out = quantize_clip(
            &c,
            &QuantizeSettings {
                division: 4,
                ..Default::default()
            },
        );
        let notes = out.notes();
        assert_eq!(notes.len(), 2);
        assert!(notes[0].end_tick <= notes[1].start_tick);
    }
}
