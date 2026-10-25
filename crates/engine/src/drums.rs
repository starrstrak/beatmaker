//! Synthesised drum kit (General-MIDI style note map). Everything is generated from scratch.
//! Notes: 36 kick, 37 rim, 38/40 snare, 39 clap, 41/43/45/47/48/50 toms, 42/44 closed hat,
//! 46 open hat, 49/57 crash, 51/59 ride, 56 cowbell.

use crate::midi::MidiEvent;
use crate::params::{pi, Curve::*, ParamInfo, ParamValues};
use std::f32::consts::TAU;

#[repr(usize)]
enum P {
    KickTune,
    KickDecay,
    SnareTone,
    SnareSnap,
    HatDecay,
    TomTune,
    Level,
}

pub const DRUM_PARAMS: [ParamInfo; 7] = [
    pi("kick_tune", "kick", 35.0, 90.0, 52.0, Log, "Hz", true),
    pi("kick_decay", "kick", 0.08, 1.5, 0.35, Log, "s", true),
    pi("snare_tone", "snare", 120.0, 320.0, 190.0, Log, "Hz", false),
    pi("snare_snap", "snare", 0.0, 1.0, 0.6, Linear, "", true),
    pi("hat_decay", "hats", 0.02, 0.4, 0.06, Log, "s", true),
    pi("tom_tune", "toms", 0.5, 2.0, 1.0, Log, "x", false),
    pi("level", "out", 0.0, 1.0, 0.8, Linear, "", true),
];

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Kind {
    #[default]
    Kick,
    Snare,
    Rim,
    Clap,
    Tom,
    HatClosed,
    HatOpen,
    Crash,
    Ride,
    Cowbell,
}

const SLOTS: usize = 12;

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    kind: Kind,
    note: u8,
    n: u32,
    vel: f32,
    phase: f32,
    phase2: f32,
    noise: u32,
    hp: f32,
    base: f32,
}

pub struct DrumKit {
    sr: f32,
    params: ParamValues,
    voices: [Voice; SLOTS],
    seed: u32,
}

fn kind_of(note: u8) -> Option<(Kind, f32)> {
    Some(match note {
        36 | 35 => (Kind::Kick, 0.0),
        37 => (Kind::Rim, 0.0),
        38 | 40 => (Kind::Snare, 0.0),
        39 => (Kind::Clap, 0.0),
        41 => (Kind::Tom, 0.6),
        43 => (Kind::Tom, 0.75),
        45 => (Kind::Tom, 0.9),
        47 => (Kind::Tom, 1.1),
        48 => (Kind::Tom, 1.3),
        50 => (Kind::Tom, 1.5),
        42 | 44 => (Kind::HatClosed, 0.0),
        46 => (Kind::HatOpen, 0.0),
        49 | 57 => (Kind::Crash, 0.0),
        51 | 59 => (Kind::Ride, 0.0),
        56 => (Kind::Cowbell, 0.0),
        _ => return None,
    })
}

impl DrumKit {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            params: ParamValues::defaults(&DRUM_PARAMS),
            voices: [Voice::default(); SLOTS],
            seed: 0x9E37_79B9,
        }
    }

    pub fn set_param(&mut self, i: usize, v: f32) {
        self.params.set(&DRUM_PARAMS, i, v);
    }

    pub fn set_modulation(&mut self, _pitch_st: f32, _bright_oct: f32) {}

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    pub fn panic(&mut self) {
        for v in self.voices.iter_mut() {
            v.active = false;
        }
    }

    pub fn handle(&mut self, ev: MidiEvent) {
        let MidiEvent::NoteOn { note, velocity, .. } = ev else {
            if let MidiEvent::ControlChange { controller: 123, .. } = ev {
                self.panic();
            }
            return;
        };
        let Some((kind, base)) = kind_of(note) else { return };
        if kind == Kind::HatClosed {
            for v in self.voices.iter_mut().filter(|v| v.kind == Kind::HatOpen) {
                v.active = false; // closed hat chokes open hat
            }
        }
        let idx = self
            .voices
            .iter()
            .position(|v| v.active && v.note == note)
            .or_else(|| self.voices.iter().position(|v| !v.active))
            .or_else(|| self.voices.iter().enumerate().max_by_key(|(_, v)| v.n).map(|(i, _)| i))
            .unwrap_or(0);
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        self.voices[idx] = Voice {
            active: true,
            kind,
            note,
            n: 0,
            vel: (velocity as f32 / 127.0).powf(1.2),
            phase: 0.0,
            phase2: 0.0,
            noise: self.seed | 1,
            hp: 0.0,
            base,
        };
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let total = left.len().min(right.len());
        left[..total].fill(0.0);
        right[..total].fill(0.0);
        let p = self.params;
        let sr = self.sr;
        let level = p.get(P::Level as usize);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            for i in 0..total {
                let t = v.n as f32 / sr;
                v.noise ^= v.noise << 13;
                v.noise ^= v.noise >> 17;
                v.noise ^= v.noise << 5;
                let white = (v.noise >> 8) as f32 / (1u32 << 23) as f32 - 1.0;
                let hp_in = white - v.hp;
                v.hp += hp_in * 0.35; // one-pole high-pass: white - lowpassed(white)
                let hiss = hp_in;
                let (s, alive) = match v.kind {
                    Kind::Kick => {
                        let tune = p.get(P::KickTune as usize);
                        let decay = p.get(P::KickDecay as usize);
                        v.phase += tune * (1.0 + 5.0 * (-t / 0.035).exp()) / sr;
                        let body = (TAU * v.phase).sin() * (-t / (decay * 0.35)).exp();
                        (body + white * 0.25 * (-t / 0.002).exp(), t < decay * 1.6)
                    }
                    Kind::Snare => {
                        let tone = p.get(P::SnareTone as usize);
                        let snap = p.get(P::SnareSnap as usize);
                        v.phase += tone * (1.0 + 0.4 * (-t / 0.02).exp()) / sr;
                        let body = (TAU * v.phase).sin() * (-t / 0.09).exp() * (1.0 - 0.5 * snap);
                        (body + hiss * snap * 1.2 * (-t / 0.11).exp(), t < 0.5)
                    }
                    Kind::Rim => {
                        v.phase += 1700.0 / sr;
                        (
                            (TAU * v.phase).sin() * (-t / 0.012).exp() + hiss * 0.3 * (-t / 0.005).exp(),
                            t < 0.1,
                        )
                    }
                    Kind::Clap => {
                        let bursts = (-((t * 1000.0) % 11.0) / 2.5).exp() * if t < 0.033 { 1.0 } else { 0.0 };
                        (hiss * (bursts * 0.9 + (-t / 0.12).exp() * 0.6), t < 0.5)
                    }
                    Kind::Tom => {
                        let tune = 110.0 * v.base * p.get(P::TomTune as usize);
                        v.phase += tune * (1.0 + 0.8 * (-t / 0.05).exp()) / sr;
                        ((TAU * v.phase).sin() * (-t / 0.18).exp(), t < 0.9)
                    }
                    Kind::HatClosed => (hiss * (-t / p.get(P::HatDecay as usize)).exp() * 0.7, t < 0.5),
                    Kind::HatOpen => (hiss * (-t / (p.get(P::HatDecay as usize) * 6.0)).exp() * 0.7, t < 2.0),
                    Kind::Crash => (hiss * (-t / 0.7).exp() * 0.8, t < 3.5),
                    Kind::Ride => {
                        v.phase += 5200.0 / sr;
                        v.phase2 += 3100.0 / sr;
                        let ping = ((TAU * v.phase).sin() + (TAU * v.phase2).sin()) * 0.15;
                        ((ping + hiss * 0.3) * (-t / 0.45).exp(), t < 2.5)
                    }
                    Kind::Cowbell => {
                        v.phase += 540.0 / sr;
                        v.phase2 += 810.0 / sr;
                        let sq = |x: f32| if x.fract() < 0.5 { 1.0 } else { -1.0 };
                        ((sq(v.phase) + sq(v.phase2)) * 0.25 * (-t / 0.09).exp(), t < 0.6)
                    }
                };
                let out = s * v.vel * level * 0.8;
                left[i] += out;
                right[i] += out;
                v.n += 1;
                if !alive {
                    v.active = false;
                    break;
                }
            }
        }
    }
}
