use serde::{Deserialize, Serialize};

/// A parsed channel-voice MIDI message. Channels are 0-based.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum MidiEvent {
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    /// Centered at 0, range -8192..=8191.
    PitchBend {
        channel: u8,
        value: i16,
    },
    ChannelPressure {
        channel: u8,
        pressure: u8,
    },
    PolyPressure {
        channel: u8,
        note: u8,
        pressure: u8,
    },
}

pub const CC_MOD_WHEEL: u8 = 1;
pub const CC_SUSTAIN: u8 = 64;
pub const CC_ALL_NOTES_OFF: u8 = 123;

impl MidiEvent {
    /// Parses a raw MIDI message. Returns `None` for system messages or malformed data.
    /// A NoteOn with velocity 0 is normalised to NoteOff, as the MIDI spec requires.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let status = *bytes.first()?;
        if !(0x80..0xF0).contains(&status) {
            return None;
        }
        let channel = status & 0x0F;
        let d1 = *bytes.get(1)? & 0x7F;
        let d2 = bytes.get(2).copied().unwrap_or(0) & 0x7F;
        match status & 0xF0 {
            0x80 => Some(Self::NoteOff {
                channel,
                note: d1,
                velocity: d2,
            }),
            0x90 if bytes.len() >= 3 => Some(if d2 == 0 {
                Self::NoteOff {
                    channel,
                    note: d1,
                    velocity: 0,
                }
            } else {
                Self::NoteOn {
                    channel,
                    note: d1,
                    velocity: d2,
                }
            }),
            0xA0 if bytes.len() >= 3 => Some(Self::PolyPressure {
                channel,
                note: d1,
                pressure: d2,
            }),
            0xB0 if bytes.len() >= 3 => Some(Self::ControlChange {
                channel,
                controller: d1,
                value: d2,
            }),
            0xD0 => Some(Self::ChannelPressure { channel, pressure: d1 }),
            0xE0 if bytes.len() >= 3 => {
                let raw = (d2 as i16) << 7 | d1 as i16;
                Some(Self::PitchBend {
                    channel,
                    value: raw - 8192,
                })
            }
            _ => None,
        }
    }

    /// Serialises to raw bytes; the second value is the used length.
    pub fn to_bytes(self) -> ([u8; 3], usize) {
        match self {
            Self::NoteOn {
                channel,
                note,
                velocity,
            } => ([0x90 | channel, note, velocity], 3),
            Self::NoteOff {
                channel,
                note,
                velocity,
            } => ([0x80 | channel, note, velocity], 3),
            Self::ControlChange {
                channel,
                controller,
                value,
            } => ([0xB0 | channel, controller, value], 3),
            Self::PitchBend { channel, value } => {
                let raw = (value + 8192) as u16;
                ([0xE0 | channel, (raw & 0x7F) as u8, (raw >> 7) as u8], 3)
            }
            Self::ChannelPressure { channel, pressure } => ([0xD0 | channel, pressure, 0], 2),
            Self::PolyPressure {
                channel,
                note,
                pressure,
            } => ([0xA0 | channel, note, pressure], 3),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_note_on_and_zero_velocity_as_off() {
        assert_eq!(
            MidiEvent::from_bytes(&[0x90, 60, 100]),
            Some(MidiEvent::NoteOn {
                channel: 0,
                note: 60,
                velocity: 100
            })
        );
        assert_eq!(
            MidiEvent::from_bytes(&[0x91, 60, 0]),
            Some(MidiEvent::NoteOff {
                channel: 1,
                note: 60,
                velocity: 0
            })
        );
    }

    #[test]
    fn pitch_bend_is_centered() {
        assert_eq!(
            MidiEvent::from_bytes(&[0xE0, 0x00, 0x40]),
            Some(MidiEvent::PitchBend { channel: 0, value: 0 })
        );
        assert_eq!(
            MidiEvent::from_bytes(&[0xE0, 0x7F, 0x7F]),
            Some(MidiEvent::PitchBend {
                channel: 0,
                value: 8191
            })
        );
        assert_eq!(
            MidiEvent::from_bytes(&[0xE0, 0x00, 0x00]),
            Some(MidiEvent::PitchBend {
                channel: 0,
                value: -8192
            })
        );
    }

    #[test]
    fn roundtrip_bytes() {
        let evs = [
            MidiEvent::NoteOn {
                channel: 3,
                note: 72,
                velocity: 99,
            },
            MidiEvent::ControlChange {
                channel: 0,
                controller: 64,
                value: 127,
            },
            MidiEvent::PitchBend {
                channel: 2,
                value: -1234,
            },
            MidiEvent::ChannelPressure {
                channel: 0,
                pressure: 55,
            },
        ];
        for ev in evs {
            let (b, n) = ev.to_bytes();
            assert_eq!(MidiEvent::from_bytes(&b[..n]), Some(ev));
        }
    }

    #[test]
    fn rejects_system_and_truncated() {
        assert_eq!(MidiEvent::from_bytes(&[0xF8]), None);
        assert_eq!(MidiEvent::from_bytes(&[0x90, 60]), None);
        assert_eq!(MidiEvent::from_bytes(&[]), None);
        assert_eq!(MidiEvent::from_bytes(&[0x40, 1, 2]), None);
    }
}
