use crate::clip::Clip;
use crate::effects::Effect;
use crate::instrument::Instrument;
use crate::midi::MidiEvent;
use crate::track::{LfoSettings, Track};

pub const MAX_TRACKS: usize = 32;

/// Objects handed back to the control thread so they are freed there, never on the audio thread.
pub enum Retired {
    Clip(Box<Clip>),
    Track(Box<Track>),
    Instrument(Box<Instrument>),
    Effect(Box<Effect>),
}

/// Control thread -> audio thread. Tracks are addressed by their stable `u32` id.
pub enum Command {
    /// Live input: goes to the armed track and is recorded when recording.
    Midi(MidiEvent),
    AddTrack(Box<Track>),
    RemoveTrack(u32),
    SetInstrument {
        track: u32,
        instrument: Box<Instrument>,
    },
    SetInstrumentParam {
        track: u32,
        index: usize,
        value: f32,
    },
    InsertEffect {
        track: u32,
        index: usize,
        effect: Box<Effect>,
        bypass: bool,
    },
    RemoveEffect {
        track: u32,
        index: usize,
    },
    MoveEffect {
        track: u32,
        from: usize,
        to: usize,
    },
    SetEffectBypass {
        track: u32,
        index: usize,
        bypass: bool,
    },
    SetEffectParam {
        track: u32,
        slot: usize,
        index: usize,
        value: f32,
    },
    SetMix {
        track: u32,
        volume: f32,
        pan: f32,
        mute: bool,
        solo: bool,
    },
    SetLfo {
        track: u32,
        settings: LfoSettings,
    },
    LoadClip {
        track: u32,
        clip: Box<Clip>,
    },
    SetArmed(Option<u32>),
    /// When true, the armed track's existing clip keeps playing while recording.
    SetOverdub(bool),
    /// Playback stops here when not looping (end of the longest clip).
    SetSongEnd(u64),
    SetLoop {
        enabled: bool,
        length_ticks: u64,
    },
    SetMasterGain(f32),
    SetMetronomeVolume(f32),
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
    /// Release every sounding note immediately.
    Panic,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TrackMeter {
    pub id: u32,
    pub peak_l: f32,
    pub peak_r: f32,
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
    pub track_count: u8,
    pub tracks: [TrackMeter; MAX_TRACKS],
}

/// Audio thread -> control thread. Fixed-size on purpose: pushing never allocates.
#[allow(clippy::large_enum_variant)]
pub enum Event {
    Status(StatusSnapshot),
    Recorded { track: u32, tick: u64, event: MidiEvent },
    TransportStopped { position_ticks: f64, was_recording: bool },
    Retired(Retired),
}
