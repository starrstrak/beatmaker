use crate::midi::MidiEvent;
use serde::{Deserialize, Serialize};

/// A MIDI event positioned in musical time (ticks at `PPQ`), independent of sample rate.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimedEvent {
    pub tick: u64,
    pub event: MidiEvent,
}

/// A recorded MIDI take. `events` must be sorted by tick.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub events: Vec<TimedEvent>,
    pub length_ticks: u64,
}

/// A note with resolved start/end, for display and export.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub note: u8,
    pub start_tick: u64,
    pub end_tick: u64,
    pub velocity: u8,
}

impl Clip {
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Pairs NoteOn/NoteOff into notes. Sustain pedal is ignored (display shows key timing).
    pub fn notes(&self) -> Vec<Note> {
        let mut open: [Option<(u64, u8)>; 128 * 16] = [None; 128 * 16];
        let mut out = Vec::new();
        for te in &self.events {
            match te.event {
                MidiEvent::NoteOn {
                    channel,
                    note,
                    velocity,
                } => {
                    let slot = channel as usize * 128 + note as usize;
                    if let Some((start, vel)) = open[slot].take() {
                        out.push(Note {
                            note,
                            start_tick: start,
                            end_tick: te.tick,
                            velocity: vel,
                        });
                    }
                    open[slot] = Some((te.tick, velocity));
                }
                MidiEvent::NoteOff { channel, note, .. } => {
                    let slot = channel as usize * 128 + note as usize;
                    if let Some((start, vel)) = open[slot].take() {
                        out.push(Note {
                            note,
                            start_tick: start,
                            end_tick: te.tick,
                            velocity: vel,
                        });
                    }
                }
                _ => {}
            }
        }
        out.sort_by_key(|n| (n.start_tick, n.note));
        out
    }
}
