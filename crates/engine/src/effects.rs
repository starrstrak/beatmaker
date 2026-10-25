//! Built-in insert effects ("plug-ins"). Enum dispatch, stereo in/out, in place.
//! Buffers are allocated in `new`, which always runs on the control thread.

use crate::dsp::Svf;
use crate::params::{pi, Curve::*, ParamInfo, ParamValues};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffectKind {
    Filter,
    Drive,
    Chorus,
    Delay,
    Reverb,
    Compressor,
}

pub const EFFECT_KINDS: [EffectKind; 6] = [
    EffectKind::Filter,
    EffectKind::Drive,
    EffectKind::Chorus,
    EffectKind::Delay,
    EffectKind::Reverb,
    EffectKind::Compressor,
];

const FILTER_PARAMS: [ParamInfo; 3] = [
    pi("type", "filter", 0.0, 2.0, 0.0, Choice, "", true),
    pi("cutoff", "filter", 40.0, 18000.0, 2000.0, Log, "Hz", true),
    pi("resonance", "filter", 0.0, 0.95, 0.2, Linear, "", true),
];
const DRIVE_PARAMS: [ParamInfo; 4] = [
    pi("drive", "drive", 0.0, 1.0, 0.4, Linear, "", true),
    pi("tone", "drive", 500.0, 16000.0, 8000.0, Log, "Hz", true),
    pi("mix", "out", 0.0, 1.0, 1.0, Linear, "", true),
    pi("level", "out", 0.0, 1.0, 0.7, Linear, "", false),
];
const CHORUS_PARAMS: [ParamInfo; 3] = [
    pi("rate", "chorus", 0.1, 5.0, 0.6, Log, "Hz", true),
    pi("depth", "chorus", 0.0, 1.0, 0.5, Linear, "", true),
    pi("mix", "out", 0.0, 1.0, 0.5, Linear, "", true),
];
const DELAY_PARAMS: [ParamInfo; 5] = [
    pi("time", "delay", 10.0, 1500.0, 350.0, Log, "ms", true),
    pi("feedback", "delay", 0.0, 0.95, 0.4, Linear, "", true),
    pi("tone", "delay", 800.0, 16000.0, 6000.0, Log, "Hz", false),
    pi("pingpong", "delay", 0.0, 1.0, 1.0, Choice, "", false),
    pi("mix", "out", 0.0, 1.0, 0.3, Linear, "", true),
];
const REVERB_PARAMS: [ParamInfo; 4] = [
    pi("size", "reverb", 0.0, 1.0, 0.55, Linear, "", true),
    pi("damp", "reverb", 0.0, 1.0, 0.5, Linear, "", true),
    pi("width", "reverb", 0.0, 1.0, 1.0, Linear, "", false),
    pi("mix", "out", 0.0, 1.0, 0.25, Linear, "", true),
];
const COMP_PARAMS: [ParamInfo; 6] = [
    pi("threshold", "comp", -60.0, 0.0, -18.0, Linear, "dB", true),
    pi("ratio", "comp", 1.0, 20.0, 4.0, Log, ":1", true),
    pi("attack", "comp", 0.1, 100.0, 10.0, Log, "ms", false),
    pi("release", "comp", 10.0, 1000.0, 120.0, Log, "ms", false),
    pi("makeup", "comp", 0.0, 24.0, 3.0, Linear, "dB", true),
    pi("mix", "out", 0.0, 1.0, 1.0, Linear, "", false),
];

impl EffectKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Filter => "filter",
            Self::Drive => "drive",
            Self::Chorus => "chorus",
            Self::Delay => "delay",
            Self::Reverb => "reverb",
            Self::Compressor => "compressor",
        }
    }

    pub fn params(self) -> &'static [ParamInfo] {
        match self {
            Self::Filter => &FILTER_PARAMS,
            Self::Drive => &DRIVE_PARAMS,
            Self::Chorus => &CHORUS_PARAMS,
            Self::Delay => &DELAY_PARAMS,
            Self::Reverb => &REVERB_PARAMS,
            Self::Compressor => &COMP_PARAMS,
        }
    }
}

pub const MAX_EFFECTS_PER_TRACK: usize = 8;

fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

// ---------------------------------------------------------------- filter
pub struct FilterFx {
    sr: f32,
    p: ParamValues,
    l: Svf,
    r: Svf,
    dirty: bool,
}

impl FilterFx {
    fn new(sr: f32) -> Self {
        Self {
            sr,
            p: ParamValues::defaults(&FILTER_PARAMS),
            l: Svf::default(),
            r: Svf::default(),
            dirty: true,
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        if self.dirty {
            let (c, q) = (self.p.get(1usize), self.p.get(2usize));
            self.l.set(c, q, self.sr);
            self.r.set(c, q, self.sr);
            self.dirty = false;
        }
        let ty = self.p.get(0usize).round() as i32;
        let pick = |t: (f32, f32, f32)| match ty {
            0 => t.0,
            2 => t.1 * 0.5,
            _ => t.2,
        };
        for i in 0..l.len() {
            l[i] = pick(self.l.process_all(l[i]));
            r[i] = pick(self.r.process_all(r[i]));
        }
    }
}

// ---------------------------------------------------------------- drive
pub struct DriveFx {
    sr: f32,
    p: ParamValues,
    lp_l: f32,
    lp_r: f32,
}

impl DriveFx {
    fn new(sr: f32) -> Self {
        Self {
            sr,
            p: ParamValues::defaults(&DRIVE_PARAMS),
            lp_l: 0.0,
            lp_r: 0.0,
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let d = self.p.get(0usize);
        let gain = 1.0 + 40.0 * d * d;
        let norm = 1.0 / gain.tanh().max(0.2);
        let a = 1.0 - (-TAU * self.p.get(1usize) / self.sr).exp();
        let (mix, level) = (self.p.get(2usize), self.p.get(3usize));
        for i in 0..l.len() {
            let (dl, dr) = (l[i], r[i]);
            self.lp_l += a * ((dl * gain).tanh() * norm - self.lp_l);
            self.lp_r += a * ((dr * gain).tanh() * norm - self.lp_r);
            l[i] = (dl * (1.0 - mix) + self.lp_l * mix * level.max(0.0) * 1.2).clamp(-4.0, 4.0);
            r[i] = (dr * (1.0 - mix) + self.lp_r * mix * level.max(0.0) * 1.2).clamp(-4.0, 4.0);
        }
    }
}

// ---------------------------------------------------------------- chorus
pub struct ChorusFx {
    sr: f32,
    p: ParamValues,
    buf: [Vec<f32>; 2],
    pos: usize,
    phase: f32,
}

impl ChorusFx {
    fn new(sr: f32) -> Self {
        let n = (0.06 * sr) as usize + 8;
        Self {
            sr,
            p: ParamValues::defaults(&CHORUS_PARAMS),
            buf: [vec![0.0; n], vec![0.0; n]],
            pos: 0,
            phase: 0.0,
        }
    }
    fn read(buf: &[f32], pos: usize, delay: f32) -> f32 {
        let n = buf.len();
        let d = delay.clamp(1.0, (n - 2) as f32);
        let i = d.floor() as usize;
        let f = d - i as f32;
        let a = buf[(pos + n - i) % n];
        let b = buf[(pos + n - i - 1) % n];
        a + (b - a) * f
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let (rate, depth, mix) = (self.p.get(0usize), self.p.get(1usize), self.p.get(2usize));
        let base = 0.012 * self.sr;
        let swing = depth * 0.006 * self.sr;
        let n = self.buf[0].len();
        for i in 0..l.len() {
            self.buf[0][self.pos] = l[i];
            self.buf[1][self.pos] = r[i];
            let ml = (TAU * self.phase).sin();
            let mr = (TAU * (self.phase + 0.25)).sin();
            let wl = Self::read(&self.buf[0], self.pos, base + swing * ml);
            let wr = Self::read(&self.buf[1], self.pos, base + swing * mr);
            l[i] = l[i] * (1.0 - mix * 0.5) + wl * mix * 0.7;
            r[i] = r[i] * (1.0 - mix * 0.5) + wr * mix * 0.7;
            self.pos = (self.pos + 1) % n;
            self.phase += rate / self.sr;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
    }
}

// ---------------------------------------------------------------- delay
pub struct DelayFx {
    sr: f32,
    p: ParamValues,
    buf: [Vec<f32>; 2],
    pos: usize,
    cur: f32,
    lp: [f32; 2],
}

impl DelayFx {
    fn new(sr: f32) -> Self {
        let n = (1.6 * sr) as usize + 4;
        let p = ParamValues::defaults(&DELAY_PARAMS);
        let cur = p.get(0usize) * 0.001 * sr;
        Self {
            sr,
            p,
            buf: [vec![0.0; n], vec![0.0; n]],
            pos: 0,
            cur,
            lp: [0.0; 2],
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = self.buf[0].len();
        let target = (self.p.get(0usize) * 0.001 * self.sr).clamp(2.0, (n - 3) as f32);
        let (fb, mix) = (self.p.get(1usize), self.p.get(4usize));
        let a = 1.0 - (-TAU * self.p.get(2usize) / self.sr).exp();
        let ping = self.p.get(3usize) >= 0.5;
        for i in 0..l.len() {
            self.cur += (target - self.cur) * 0.0015;
            let rp = self.cur;
            let k = rp.floor() as usize;
            let f = rp - k as f32;
            let rd = |b: &Vec<f32>| {
                let x = b[(self.pos + n - k) % n];
                let y = b[(self.pos + n - k - 1) % n];
                x + (y - x) * f
            };
            let (dl, dr) = (rd(&self.buf[0]), rd(&self.buf[1]));
            let (fl, fr) = if ping { (dr, dl) } else { (dl, dr) };
            self.lp[0] += a * (fl - self.lp[0]);
            self.lp[1] += a * (fr - self.lp[1]);
            self.buf[0][self.pos] = crate::dsp::flush_denormal(l[i] + fb * self.lp[0]);
            self.buf[1][self.pos] = crate::dsp::flush_denormal(r[i] + fb * self.lp[1]);
            l[i] += dl * mix;
            r[i] += dr * mix;
            self.pos = (self.pos + 1) % n;
        }
    }
}

// ---------------------------------------------------------------- reverb (Freeverb layout)
struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}
struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS: [usize; 4] = [556, 441, 341, 225];
const SPREAD: usize = 23;

pub struct ReverbFx {
    p: ParamValues,
    combs: [Vec<Comb>; 2],
    allpass: [Vec<Allpass>; 2],
}

impl ReverbFx {
    fn new(sr: f32) -> Self {
        let scale = sr / 44100.0;
        let size = |n: usize, ch: usize| (((n + ch * SPREAD) as f32) * scale) as usize + 1;
        let mk = |ch: usize| {
            (
                COMBS
                    .iter()
                    .map(|&n| Comb {
                        buf: vec![0.0; size(n, ch)],
                        idx: 0,
                        store: 0.0,
                    })
                    .collect::<Vec<_>>(),
                ALLPASS
                    .iter()
                    .map(|&n| Allpass {
                        buf: vec![0.0; size(n, ch)],
                        idx: 0,
                    })
                    .collect::<Vec<_>>(),
            )
        };
        let (c0, a0) = mk(0);
        let (c1, a1) = mk(1);
        Self {
            p: ParamValues::defaults(&REVERB_PARAMS),
            combs: [c0, c1],
            allpass: [a0, a1],
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let feedback = 0.7 + 0.28 * self.p.get(0usize);
        let damp = 0.4 * self.p.get(1usize);
        let width = self.p.get(2usize);
        let mix = self.p.get(3usize);
        let (w1, w2) = (3.0 * (0.5 + 0.5 * width), 3.0 * 0.5 * (1.0 - width));
        for i in 0..l.len() {
            let input = (l[i] + r[i]) * 0.015;
            let mut out = [0.0f32; 2];
            #[allow(clippy::needless_range_loop)]
            for ch in 0..2 {
                let mut acc = 0.0;
                for c in self.combs[ch].iter_mut() {
                    let y = c.buf[c.idx];
                    c.store = crate::dsp::flush_denormal(y * (1.0 - damp) + c.store * damp);
                    c.buf[c.idx] = input + c.store * feedback;
                    c.idx = (c.idx + 1) % c.buf.len();
                    acc += y;
                }
                for a in self.allpass[ch].iter_mut() {
                    let b = a.buf[a.idx];
                    let y = -acc + b;
                    a.buf[a.idx] = crate::dsp::flush_denormal(acc + b * 0.5);
                    a.idx = (a.idx + 1) % a.buf.len();
                    acc = y;
                }
                out[ch] = acc;
            }
            let (wl, wr) = (out[0] * w1 + out[1] * w2, out[1] * w1 + out[0] * w2);
            l[i] = l[i] * (1.0 - mix * 0.5) + wl * mix;
            r[i] = r[i] * (1.0 - mix * 0.5) + wr * mix;
        }
    }
}

// ---------------------------------------------------------------- compressor
pub struct CompressorFx {
    sr: f32,
    p: ParamValues,
    env: f32,
}

impl CompressorFx {
    fn new(sr: f32) -> Self {
        Self {
            sr,
            p: ParamValues::defaults(&COMP_PARAMS),
            env: 0.0,
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let (thr, ratio) = (self.p.get(0usize), self.p.get(1usize));
        let att = (-1.0 / (self.p.get(2usize) * 0.001 * self.sr)).exp();
        let rel = (-1.0 / (self.p.get(3usize) * 0.001 * self.sr)).exp();
        let makeup = db_to_lin(self.p.get(4usize));
        let mix = self.p.get(5usize);
        for i in 0..l.len() {
            let level = l[i].abs().max(r[i].abs());
            let c = if level > self.env { att } else { rel };
            self.env = crate::dsp::flush_denormal(level + c * (self.env - level));
            let env_db = 20.0 * self.env.max(1e-6).log10();
            let reduction = if env_db > thr {
                (thr - env_db) * (1.0 - 1.0 / ratio)
            } else {
                0.0
            };
            let g = db_to_lin(reduction) * makeup;
            l[i] = l[i] * (1.0 - mix) + l[i] * g * mix;
            r[i] = r[i] * (1.0 - mix) + r[i] * g * mix;
        }
    }
}

// ---------------------------------------------------------------- enum
pub enum Effect {
    Filter(Box<FilterFx>),
    Drive(Box<DriveFx>),
    Chorus(Box<ChorusFx>),
    Delay(Box<DelayFx>),
    Reverb(Box<ReverbFx>),
    Compressor(Box<CompressorFx>),
}

impl Effect {
    pub fn new(kind: EffectKind, sr: f32) -> Self {
        match kind {
            EffectKind::Filter => Self::Filter(Box::new(FilterFx::new(sr))),
            EffectKind::Drive => Self::Drive(Box::new(DriveFx::new(sr))),
            EffectKind::Chorus => Self::Chorus(Box::new(ChorusFx::new(sr))),
            EffectKind::Delay => Self::Delay(Box::new(DelayFx::new(sr))),
            EffectKind::Reverb => Self::Reverb(Box::new(ReverbFx::new(sr))),
            EffectKind::Compressor => Self::Compressor(Box::new(CompressorFx::new(sr))),
        }
    }

    pub fn kind(&self) -> EffectKind {
        match self {
            Self::Filter(_) => EffectKind::Filter,
            Self::Drive(_) => EffectKind::Drive,
            Self::Chorus(_) => EffectKind::Chorus,
            Self::Delay(_) => EffectKind::Delay,
            Self::Reverb(_) => EffectKind::Reverb,
            Self::Compressor(_) => EffectKind::Compressor,
        }
    }

    pub fn set_param(&mut self, index: usize, value: f32) {
        let infos = self.kind().params();
        match self {
            Self::Filter(e) => {
                e.p.set(infos, index, value);
                e.dirty = true;
            }
            Self::Drive(e) => e.p.set(infos, index, value),
            Self::Chorus(e) => e.p.set(infos, index, value),
            Self::Delay(e) => e.p.set(infos, index, value),
            Self::Reverb(e) => e.p.set(infos, index, value),
            Self::Compressor(e) => e.p.set(infos, index, value),
        }
    }

    pub fn set_params(&mut self, values: &[f32]) {
        for (i, v) in values.iter().enumerate() {
            self.set_param(i, *v);
        }
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        match self {
            Self::Filter(e) => e.process(l, r),
            Self::Drive(e) => e.process(l, r),
            Self::Chorus(e) => e.process(l, r),
            Self::Delay(e) => e.process(l, r),
            Self::Reverb(e) => e.process(l, r),
            Self::Compressor(e) => e.process(l, r),
        }
    }
}
