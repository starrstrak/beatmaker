//! One mixer/sequencer track on the audio thread: instrument -> insert effects -> fader/pan.

use crate::clip::Clip;
use crate::effects::{Effect, MAX_EFFECTS_PER_TRACK};
use crate::instrument::Instrument;
use crate::synth::CHUNK;
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LfoShape {
    Sine,
    Triangle,
    Square,
    SampleHold,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LfoTarget {
    None,
    Pitch,
    Brightness,
    Volume,
    Pan,
}

/// Per-track modulation source. `depth` is 0..1; its meaning depends on the target:
/// pitch +-2 semitones, brightness +-3 octaves, volume 0..-100 % tremolo, pan +-100 %.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LfoSettings {
    pub shape: LfoShape,
    pub target: LfoTarget,
    pub rate_hz: f32,
    pub depth: f32,
}

impl Default for LfoSettings {
    fn default() -> Self {
        Self {
            shape: LfoShape::Sine,
            target: LfoTarget::None,
            rate_hz: 4.0,
            depth: 0.5,
        }
    }
}

pub struct Slot {
    pub effect: Box<Effect>,
    pub bypass: bool,
}

pub struct Track {
    pub id: u32,
    pub instrument: Instrument,
    pub effects: Vec<Slot>,
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// Recomputed by the processor from mute/solo of all tracks.
    pub audible: bool,
    pub lfo: LfoSettings,
    pub clip: Box<Clip>,
    pub clip_idx: usize,
    sr: f32,
    lfo_phase: f32,
    sh_value: f32,
    rng: u32,
    ctrl_pos: usize,
    tgt_l: f32,
    tgt_r: f32,
    gain_l: f32,
    gain_r: f32,
    peak_l: f32,
    peak_r: f32,
}

impl Track {
    /// Allocates everything the track will ever need. Call on the control thread only.
    pub fn new(id: u32, instrument: Instrument, sr: f32) -> Self {
        Self {
            id,
            instrument,
            effects: Vec::with_capacity(MAX_EFFECTS_PER_TRACK),
            volume: 0.7,
            pan: 0.0,
            mute: false,
            solo: false,
            audible: true,
            lfo: LfoSettings::default(),
            clip: Box::default(),
            clip_idx: 0,
            sr,
            lfo_phase: 0.0,
            sh_value: 0.0,
            rng: 0x1357_9BDF ^ id.wrapping_mul(2654435761),
            ctrl_pos: 0,
            tgt_l: 0.7,
            tgt_r: 0.7,
            gain_l: 0.7,
            gain_r: 0.7,
            peak_l: 0.0,
            peak_r: 0.0,
        }
    }

    pub fn take_peaks(&mut self) -> (f32, f32) {
        let p = (self.peak_l, self.peak_r);
        self.peak_l = 0.0;
        self.peak_r = 0.0;
        p
    }

    fn next_random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }

    fn update_control(&mut self) {
        let l = self.lfo;
        let prev = self.lfo_phase;
        self.lfo_phase += l.rate_hz * CHUNK as f32 / self.sr;
        let wrapped = self.lfo_phase >= 1.0;
        if wrapped {
            self.lfo_phase -= self.lfo_phase.floor();
        }
        if wrapped || (prev == 0.0 && self.sh_value == 0.0) {
            self.sh_value = self.next_random();
        }
        let ph = self.lfo_phase;
        let v = match l.shape {
            LfoShape::Sine => (TAU * ph).sin(),
            LfoShape::Triangle => 4.0 * (ph - 0.5).abs() - 1.0,
            LfoShape::Square => {
                if ph < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoShape::SampleHold => self.sh_value,
        };
        let (mut pitch, mut bright, mut trem, mut pan) = (0.0, 0.0, 1.0, 0.0);
        match l.target {
            LfoTarget::None => {}
            LfoTarget::Pitch => pitch = v * l.depth * 2.0,
            LfoTarget::Brightness => bright = v * l.depth * 3.0,
            LfoTarget::Volume => trem = 1.0 - l.depth * 0.5 * (1.0 + v),
            LfoTarget::Pan => pan = v * l.depth,
        }
        self.instrument.set_modulation(pitch, bright);
        let p = (self.pan + pan).clamp(-1.0, 1.0);
        let base = if self.audible { self.volume * trem } else { 0.0 };
        // Unity at centre, the far side fades linearly to silence.
        self.tgt_l = base * (1.0 - p).min(1.0);
        self.tgt_r = base * (1.0 + p).min(1.0);
    }

    /// Renders `l.len()` (<= CHUNK) frames of the finished track signal, overwriting the buffers.
    pub fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let total = l.len().min(r.len()).min(CHUNK);
        let mut done = 0;
        while done < total {
            if self.ctrl_pos == 0 {
                self.update_control();
            }
            let m = (total - done).min(CHUNK - self.ctrl_pos);
            let (cl, cr) = (&mut l[done..done + m], &mut r[done..done + m]);
            self.instrument.render(cl, cr);
            for slot in self.effects.iter_mut().filter(|s| !s.bypass) {
                slot.effect.process(cl, cr);
            }
            for i in 0..m {
                self.gain_l += (self.tgt_l - self.gain_l) * 0.01;
                self.gain_r += (self.tgt_r - self.gain_r) * 0.01;
                cl[i] *= self.gain_l;
                cr[i] *= self.gain_r;
                self.peak_l = self.peak_l.max(cl[i].abs());
                self.peak_r = self.peak_r.max(cr[i].abs());
            }
            self.ctrl_pos = (self.ctrl_pos + m) % CHUNK;
            done += m;
        }
    }
}
