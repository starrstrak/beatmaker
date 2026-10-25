//! 12-voice two-operator FM synth (carrier + modulator with feedback).

use crate::dsp::Adsr;
use crate::midi::{MidiEvent, CC_ALL_NOTES_OFF, CC_MOD_WHEEL, CC_SUSTAIN};
use crate::params::{pi, Curve::*, ParamInfo, ParamValues};
use crate::synth::CHUNK;
use std::f32::consts::TAU;

pub const MAX_VOICES: usize = 12;
const VOICE_GAIN: f32 = 0.3;

#[repr(usize)]
enum P {
    Ratio,
    Index,
    IndexEnv,
    IndexDecay,
    Feedback,
    AmpAttack,
    AmpDecay,
    AmpSustain,
    AmpRelease,
    BendRange,
    ModDepth,
    Level,
}

pub const FM_PARAMS: [ParamInfo; 12] = [
    pi("ratio", "fm", 0.5, 14.0, 1.0, Log, "x", true),
    pi("index", "fm", 0.0, 10.0, 2.5, Linear, "", true),
    pi("index_env", "fm", 0.0, 1.0, 0.7, Linear, "", false),
    pi("index_decay", "fm", 0.01, 4.0, 0.6, Log, "s", false),
    pi("feedback", "fm", 0.0, 1.0, 0.0, Linear, "", false),
    pi("amp_attack", "amp", 0.001, 5.0, 0.004, Log, "s", true),
    pi("amp_decay", "amp", 0.005, 5.0, 0.8, Log, "s", false),
    pi("amp_sustain", "amp", 0.0, 1.0, 0.4, Linear, "", false),
    pi("amp_release", "amp", 0.005, 8.0, 0.3, Log, "s", true),
    pi("bend_range", "perf", 0.0, 12.0, 2.0, Linear, "st", false),
    pi("mod_depth", "perf", 0.0, 2.0, 0.5, Linear, "st", false),
    pi("level", "out", 0.0, 1.0, 0.8, Linear, "", true),
];

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
    cph: f32,
    mph: f32,
    fb: f32,
    menv: f32,
    amp: Adsr,
}

pub struct FmSynth {
    sr: f32,
    params: ParamValues,
    voices: [Voice; MAX_VOICES],
    bend: f32,
    mod_wheel: f32,
    pressure: f32,
    sustain_down: bool,
    lfo_phase: f32,
    counter: u64,
    ctrl_pos: usize,
    pitch_mul: f32,
    mod_pitch: f32,
    mod_bright: f32,
}

impl FmSynth {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            params: ParamValues::defaults(&FM_PARAMS),
            voices: [Voice::default(); MAX_VOICES],
            bend: 0.0,
            mod_wheel: 0.0,
            pressure: 0.0,
            sustain_down: false,
            lfo_phase: 0.0,
            counter: 0,
            ctrl_pos: 0,
            pitch_mul: 1.0,
            mod_pitch: 0.0,
            mod_bright: 0.0,
        }
    }

    pub fn set_param(&mut self, i: usize, v: f32) {
        self.params.set(&FM_PARAMS, i, v);
    }

    pub fn set_modulation(&mut self, pitch_st: f32, bright_oct: f32) {
        self.mod_pitch = pitch_st;
        self.mod_bright = bright_oct;
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
        let r = self.params.get(P::AmpRelease as usize).min(0.05);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            v.held = false;
            v.sustained = false;
            v.amp.gate_off(r, self.sr);
        }
        self.sustain_down = false;
    }

    fn set_sustain(&mut self, down: bool) {
        self.sustain_down = down;
        if !down {
            let (r, sr) = (self.params.get(P::AmpRelease as usize), self.sr);
            for v in self.voices.iter_mut().filter(|v| v.active && v.sustained && !v.held) {
                v.sustained = false;
                v.amp.gate_off(r, sr);
            }
        }
    }

    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        let idx = self
            .voices
            .iter()
            .position(|v| v.active && v.note == note && v.channel == channel)
            .or_else(|| self.voices.iter().position(|v| !v.active))
            .or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| (!v.amp.is_releasing(), v.age))
                    .map(|(i, _)| i)
            })
            .unwrap_or(0);
        self.counter += 1;
        let p = self.params;
        let v = &mut self.voices[idx];
        v.active = true;
        v.note = note;
        v.channel = channel;
        v.held = true;
        v.sustained = false;
        v.age = self.counter;
        v.base_hz = 440.0 * ((note as f32 - 69.0) / 12.0).exp2();
        v.velocity = (velocity as f32 / 127.0).powf(1.3);
        v.cph = 0.0;
        v.mph = 0.0;
        v.fb = 0.0;
        v.menv = 1.0;
        v.amp.gate_on(
            p.get(P::AmpAttack as usize),
            p.get(P::AmpDecay as usize),
            p.get(P::AmpSustain as usize),
            self.sr,
        );
    }

    fn note_off(&mut self, channel: u8, note: u8) {
        let (r, sr, sus) = (self.params.get(P::AmpRelease as usize), self.sr, self.sustain_down);
        for v in self
            .voices
            .iter_mut()
            .filter(|v| v.active && v.held && v.note == note && v.channel == channel)
        {
            v.held = false;
            if sus {
                v.sustained = true;
            } else {
                v.amp.gate_off(r, sr);
            }
        }
    }

    fn update_control(&mut self) {
        self.lfo_phase += 5.5 * CHUNK as f32 / self.sr;
        if self.lfo_phase >= 1.0 {
            self.lfo_phase -= 1.0;
        }
        let lfo = (TAU * self.lfo_phase).sin();
        let vib = lfo * self.params.get(P::ModDepth as usize) * (self.mod_wheel + 0.35 * self.pressure);
        let bend = self.bend * self.params.get(P::BendRange as usize);
        self.pitch_mul = ((bend + vib + self.mod_pitch) / 12.0).exp2();
    }

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
        let p = self.params;
        let sr = self.sr;
        let ratio = p.get(P::Ratio as usize);
        let index = p.get(P::Index as usize) * self.mod_bright.exp2();
        let ie = p.get(P::IndexEnv as usize);
        let fb = p.get(P::Feedback as usize) * 0.9;
        let menv_coef = (-6.9 / (p.get(P::IndexDecay as usize) * sr)).exp();
        let level = p.get(P::Level as usize);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let hz = v.base_hz * self.pitch_mul;
            let cinc = (hz / sr).min(0.45);
            let minc = (hz * ratio / sr).min(0.45);
            let gain = v.velocity * VOICE_GAIN * level;
            for i in 0..left.len() {
                let m = (TAU * v.mph + fb * v.fb).sin();
                v.fb = m;
                let eff = index * (1.0 - ie + ie * v.menv);
                let c = (TAU * v.cph + eff * m).sin();
                v.cph += cinc;
                if v.cph >= 1.0 {
                    v.cph -= 1.0;
                }
                v.mph += minc;
                if v.mph >= 1.0 {
                    v.mph -= 1.0;
                }
                v.menv *= menv_coef;
                let s = c * v.amp.tick() * gain;
                left[i] += s;
                right[i] += s;
            }
            if v.amp.is_idle() {
                v.active = false;
            }
        }
    }
}
