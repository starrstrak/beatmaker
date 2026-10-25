//! 16-voice polyphonic subtractive synth: 2 oscillators -> low-pass SVF -> amp envelope.

use crate::dsp::{Adsr, Svf, Waveform};
use crate::midi::{MidiEvent, CC_ALL_NOTES_OFF, CC_MOD_WHEEL, CC_SUSTAIN};
use crate::params::{ParamId, ParamSet};

pub const MAX_VOICES: usize = 16;
/// Render granularity: filter coefficients and pitch modulation update once per chunk.
pub const CHUNK: usize = 32;
const VOICE_GAIN: f32 = 0.3;
const VIBRATO_HZ: f32 = 5.5;

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    note: u8,
    channel: u8,
    held: bool,
    sustained: bool,
    age: u64,
    base_hz: f32,
    velocity: f32,
    phase1: f32,
    phase2: f32,
    amp: Adsr,
    flt: Adsr,
    svf: Svf,
}

pub struct Synth {
    sample_rate: f32,
    params: ParamSet,
    voices: [Voice; MAX_VOICES],
    bend: f32,
    mod_wheel: f32,
    pressure: f32,
    sustain_down: bool,
    lfo_phase: f32,
    counter: u64,
    ctrl_pos: usize,
    pitch_mul: f32,
}

#[inline]
fn midi_to_hz(note: u8) -> f32 {
    440.0 * ((note as f32 - 69.0) / 12.0).exp2()
}

impl Synth {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            params: ParamSet::default(),
            voices: [Voice::default(); MAX_VOICES],
            bend: 0.0,
            mod_wheel: 0.0,
            pressure: 0.0,
            sustain_down: false,
            lfo_phase: 0.0,
            counter: 0,
            ctrl_pos: 0,
            pitch_mul: 1.0,
        }
    }

    pub fn set_param(&mut self, id: ParamId, value: f32) {
        self.params.set(id, value);
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    pub fn handle(&mut self, ev: MidiEvent) {
        match ev {
            MidiEvent::NoteOn {
                channel,
                note,
                velocity,
            } => self.note_on(channel, note, velocity),
            MidiEvent::NoteOff { channel, note, .. } => self.note_off(channel, note),
            MidiEvent::PitchBend { value, .. } => self.bend = value as f32 / 8192.0,
            MidiEvent::ChannelPressure { pressure, .. } => self.pressure = pressure as f32 / 127.0,
            MidiEvent::ControlChange { controller, value, .. } => match controller {
                CC_MOD_WHEEL => self.mod_wheel = value as f32 / 127.0,
                CC_SUSTAIN => self.set_sustain(value >= 64),
                CC_ALL_NOTES_OFF => self.panic(),
                _ => {}
            },
            MidiEvent::PolyPressure { .. } => {}
        }
    }

    pub fn panic(&mut self) {
        let r = self.params.get(ParamId::AmpRelease).min(0.05);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            v.held = false;
            v.sustained = false;
            v.amp.gate_off(r, self.sample_rate);
            v.flt.gate_off(r, self.sample_rate);
        }
        self.sustain_down = false;
    }

    fn set_sustain(&mut self, down: bool) {
        self.sustain_down = down;
        if !down {
            let (r, fr, sr) = (
                self.params.get(ParamId::AmpRelease),
                self.params.get(ParamId::FilterRelease),
                self.sample_rate,
            );
            for v in self.voices.iter_mut().filter(|v| v.active && v.sustained && !v.held) {
                v.sustained = false;
                v.amp.gate_off(r, sr);
                v.flt.gate_off(fr, sr);
            }
        }
    }

    fn pick_voice(&self, channel: u8, note: u8) -> usize {
        // Same key retriggers its own voice.
        if let Some(i) = self
            .voices
            .iter()
            .position(|v| v.active && v.note == note && v.channel == channel)
        {
            return i;
        }
        if let Some(i) = self.voices.iter().position(|v| !v.active) {
            return i;
        }
        // Steal: oldest releasing voice, else oldest voice.
        let pick = |only_releasing: bool| {
            self.voices
                .iter()
                .enumerate()
                .filter(|(_, v)| !only_releasing || v.amp.is_releasing())
                .min_by_key(|(_, v)| v.age)
                .map(|(i, _)| i)
        };
        pick(true).or_else(|| pick(false)).unwrap_or(0)
    }

    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        let idx = self.pick_voice(channel, note);
        self.counter += 1;
        let p = &self.params;
        let sr = self.sample_rate;
        let v = &mut self.voices[idx];
        let retrigger = v.active && v.note == note && v.channel == channel;
        v.active = true;
        v.note = note;
        v.channel = channel;
        v.held = true;
        v.sustained = false;
        v.age = self.counter;
        v.base_hz = midi_to_hz(note);
        v.velocity = (velocity as f32 / 127.0).powf(1.3);
        if !retrigger {
            v.phase1 = 0.0;
            v.phase2 = 0.25;
            v.svf.reset();
        }
        v.svf.set(p.get(ParamId::Cutoff), p.get(ParamId::Resonance), sr);
        v.amp.gate_on(
            p.get(ParamId::AmpAttack),
            p.get(ParamId::AmpDecay),
            p.get(ParamId::AmpSustain),
            sr,
        );
        v.flt.gate_on(
            p.get(ParamId::FilterAttack),
            p.get(ParamId::FilterDecay),
            p.get(ParamId::FilterSustain),
            sr,
        );
    }

    fn note_off(&mut self, channel: u8, note: u8) {
        let (r, fr, sr, sustain) = (
            self.params.get(ParamId::AmpRelease),
            self.params.get(ParamId::FilterRelease),
            self.sample_rate,
            self.sustain_down,
        );
        for v in self
            .voices
            .iter_mut()
            .filter(|v| v.active && v.held && v.note == note && v.channel == channel)
        {
            v.held = false;
            if sustain {
                v.sustained = true;
            } else {
                v.amp.gate_off(r, sr);
                v.flt.gate_off(fr, sr);
            }
        }
    }

    /// Control-rate update (once per `CHUNK` samples, aligned to the absolute stream so the
    /// output does not depend on how the host splits the buffer).
    fn update_control(&mut self) {
        let p = self.params;
        let sr = self.sample_rate;
        self.lfo_phase += VIBRATO_HZ * CHUNK as f32 / sr;
        if self.lfo_phase >= 1.0 {
            self.lfo_phase -= 1.0;
        }
        let lfo = (std::f32::consts::TAU * self.lfo_phase).sin();
        let vibrato_st = lfo * p.get(ParamId::ModDepth) * (self.mod_wheel + 0.35 * self.pressure);
        self.pitch_mul = ((self.bend * p.get(ParamId::BendRange) + vibrato_st) / 12.0).exp2();
        let (cutoff, res, env_amt) = (
            p.get(ParamId::Cutoff),
            p.get(ParamId::Resonance),
            p.get(ParamId::FilterEnvAmount),
        );
        for v in self.voices.iter_mut().filter(|v| v.active) {
            v.svf.set(cutoff * (env_amt * v.flt.level()).exp2(), res, sr);
        }
    }

    /// Renders `left.len()` frames (any length <= CHUNK), overwriting the buffers.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let total = left.len().min(right.len()).min(CHUNK);
        left[..total].fill(0.0);
        right[..total].fill(0.0);
        let mut done = 0;
        while done < total {
            if self.ctrl_pos == 0 {
                self.update_control();
            }
            let m = (total - done).min(CHUNK - self.ctrl_pos);
            if self.active_voices() > 0 {
                self.render_voices(&mut left[done..done + m], &mut right[done..done + m]);
            }
            self.ctrl_pos = (self.ctrl_pos + m) % CHUNK;
            done += m;
        }
    }

    fn render_voices(&mut self, left: &mut [f32], right: &mut [f32]) {
        let sr = self.sample_rate;
        let p = self.params;
        let wave1 = Waveform::from_param(p.get(ParamId::Osc1Wave));
        let wave2 = Waveform::from_param(p.get(ParamId::Osc2Wave));
        let detune_mul = (p.get(ParamId::Osc2Detune) / 1200.0).exp2();
        let level2 = p.get(ParamId::Osc2Level);
        let norm = 1.0 / (1.0 + level2);
        let pitch_mul = self.pitch_mul;

        for v in self.voices.iter_mut().filter(|v| v.active) {
            let hz = v.base_hz * pitch_mul;
            let dt1 = (hz / sr).min(0.45);
            let dt2 = (hz * detune_mul / sr).min(0.45);
            let vgain = v.velocity * VOICE_GAIN;
            for i in 0..left.len() {
                v.flt.tick();
                let amp = v.amp.tick();
                let o1 = wave1.sample(v.phase1, dt1);
                let o2 = wave2.sample(v.phase2, dt2);
                v.phase1 += dt1;
                if v.phase1 >= 1.0 {
                    v.phase1 -= 1.0;
                }
                v.phase2 += dt2;
                if v.phase2 >= 1.0 {
                    v.phase2 -= 1.0;
                }
                let s = v.svf.process_lp((o1 + level2 * o2) * norm) * amp * vgain;
                left[i] += s;
                right[i] += s;
            }
            if v.amp.is_idle() {
                v.active = false;
            }
        }
    }
}
