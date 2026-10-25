use crate::clip::Clip;
use crate::midi::MidiEvent;
use crate::params::ParamId;

/// Control thread -> audio thread.
pub enum Command {
    Midi(MidiEvent),
    SetParam(ParamId, f32),
    /// Start the transport from the beginning. With `record`, a count-in is played first.
    Play {
        record: bool,
    },
    Stop,
    SetTempo(f32),
    SetTimeSignature {
        numerator: u8,
        denominator: u8,
    },
    SetMetronome(bool),
    /// Count-in length in beats (0 = none).
    SetCountIn(u8),
    /// The audio thread hands the previous clip back via `Event::ClipRetired`
    /// so deallocation never happens on the audio thread.
    LoadClip(Box<Clip>),
    /// Release every sounding note immediately.
    Panic,
}

/// Periodic meter/transport snapshot (about 50 per second).
#[derive(Clone, Copy, Debug, Default)]
pub struct StatusSnapshot {
    pub position_ticks: f64,
    pub peak_l: f32,
    pub peak_r: f32,
    /// Worst callback time / available time over the interval. >1.0 means a glitch.
    pub load: f32,
    pub voices: u8,
    pub playing: bool,
    pub recording: bool,
    pub counting_in: bool,
}

/// Audio thread -> control thread.
pub enum Event {
    Status(StatusSnapshot),
    Recorded { tick: u64, event: MidiEvent },
    TransportStopped { position_ticks: f64, was_recording: bool },
    ClipRetired(Box<Clip>),
}
