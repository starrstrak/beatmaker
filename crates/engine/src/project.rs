//! Project model (control-thread truth) and its conversion into audio-thread objects.

use crate::clip::Clip;
use crate::effects::{Effect, EffectKind};
use crate::instrument::{Instrument, InstrumentKind};
use crate::params::defaults_vec;
use crate::quantize::QuantizeSettings;
use crate::track::{LfoSettings, Slot, Track};
use serde::{Deserialize, Serialize};

pub const MAX_TRACKS_USER: usize = crate::messages::MAX_TRACKS;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectState {
    pub kind: EffectKind,
    pub params: Vec<f32>,
    pub bypass: bool,
}

impl EffectState {
    pub fn new(kind: EffectKind) -> Self {
        Self {
            kind,
            params: defaults_vec(kind.params()),
            bypass: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackState {
    pub id: u32,
    pub name: String,
    pub color: u8,
    pub instrument: InstrumentKind,
    pub preset: Option<String>,
    pub params: Vec<f32>,
    pub effects: Vec<EffectState>,
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub lfo: LfoSettings,
    pub quantize: QuantizeSettings,
    pub clip: Clip,
}

impl TrackState {
    pub fn new(id: u32, name: String, color: u8, instrument: InstrumentKind) -> Self {
        Self {
            id,
            name,
            color,
            instrument,
            preset: None,
            params: defaults_vec(instrument.params()),
            effects: Vec::new(),
            volume: 0.7,
            pan: 0.0,
            mute: false,
            solo: false,
            lfo: LfoSettings::default(),
            quantize: QuantizeSettings::default(),
            clip: Clip::default(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub tracks: Vec<TrackState>,
    pub next_id: u32,
}

impl Project {
    pub fn track(&self, id: u32) -> Option<&TrackState> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn track_mut(&mut self, id: u32) -> Option<&mut TrackState> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    /// End of the longest clip, in ticks.
    pub fn song_end(&self) -> u64 {
        self.tracks.iter().map(|t| t.clip.length_ticks).max().unwrap_or(0)
    }
}

pub fn build_instrument(kind: InstrumentKind, params: &[f32], sr: f32) -> Instrument {
    let mut i = Instrument::new(kind, sr);
    i.set_params(params);
    i
}

pub fn build_effect(state: &EffectState, sr: f32) -> Effect {
    let mut e = Effect::new(state.kind, sr);
    e.set_params(&state.params);
    e
}

/// Builds the audio-thread track (allocating everything it needs). Control thread only.
pub fn build_track(state: &TrackState, sr: f32) -> Box<Track> {
    let mut t = Track::new(state.id, build_instrument(state.instrument, &state.params, sr), sr);
    for e in &state.effects {
        t.effects.push(Slot {
            effect: Box::new(build_effect(e, sr)),
            bypass: e.bypass,
        });
    }
    t.volume = state.volume;
    t.pan = state.pan;
    t.mute = state.mute;
    t.solo = state.solo;
    t.lfo = state.lfo;
    t.clip = Box::new(state.clip.clone());
    Box::new(t)
}
