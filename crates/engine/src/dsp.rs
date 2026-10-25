//! Small DSP building blocks. Everything here is allocation-free.

use std::f32::consts::PI;

/// Flush tiny values to zero to avoid denormal slowdowns in feedback paths.
#[inline]
pub fn flush_denormal(x: f32) -> f32 {
    if x.abs() < 1e-20 {
        0.0
    } else {
        x
    }
}

/// PolyBLEP residual for band-limiting discontinuities. `t` is phase in [0,1), `dt` the increment.
#[inline]
pub fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Triangle,
    Saw,
    Square,
}

impl Waveform {
    pub fn from_param(v: f32) -> Self {
        match v.round() as i32 {
            0 => Waveform::Sine,
            1 => Waveform::Triangle,
            2 => Waveform::Saw,
            _ => Waveform::Square,
        }
    }

    /// One sample for phase `t` in [0,1) and increment `dt`.
    #[inline]
    pub fn sample(self, t: f32, dt: f32) -> f32 {
        match self {
            Waveform::Sine => (2.0 * PI * t).sin(),
            Waveform::Triangle => 4.0 * (t - 0.5).abs() - 1.0,
            Waveform::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
            Waveform::Square => {
                let naive = if t < 0.5 { 1.0 } else { -1.0 };
                let t2 = if t + 0.5 >= 1.0 { t + 0.5 - 1.0 } else { t + 0.5 };
                naive + poly_blep(t, dt) - poly_blep(t2, dt)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// ADSR with linear attack and exponential decay/release.
#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    stage: Stage,
    level: f32,
    attack_inc: f32,
    decay_coef: f32,
    sustain: f32,
    release_coef: f32,
}

impl Default for Adsr {
    fn default() -> Self {
        Self {
            stage: Stage::Idle,
            level: 0.0,
            attack_inc: 1.0,
            decay_coef: 0.0,
            sustain: 1.0,
            release_coef: 0.0,
        }
    }
}

#[inline]
fn time_to_coef(seconds: f32, sr: f32) -> f32 {
    // Reaches about -60 dB in `seconds`.
    1.0 - (-6.9 / (seconds.max(0.0005) * sr)).exp()
}

impl Adsr {
    pub fn gate_on(&mut self, a: f32, d: f32, s: f32, sr: f32) {
        self.attack_inc = 1.0 / (a.max(0.0005) * sr);
        self.decay_coef = time_to_coef(d, sr);
        self.sustain = s;
        self.stage = Stage::Attack;
    }

    pub fn gate_off(&mut self, r: f32, sr: f32) {
        if self.stage != Stage::Idle {
            self.release_coef = time_to_coef(r, sr);
            self.stage = Stage::Release;
        }
    }

    pub fn kill(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    #[inline]
    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    #[inline]
    pub fn is_releasing(&self) -> bool {
        self.stage == Stage::Release
    }

    #[inline]
    pub fn level(&self) -> f32 {
        self.level
    }

    #[inline]
    pub fn tick(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += self.attack_inc;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level += (self.sustain - self.level) * self.decay_coef;
                if (self.level - self.sustain).abs() < 1e-4 {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.level = self.sustain,
            Stage::Release => {
                self.level -= self.level * self.release_coef;
                if self.level < 1e-4 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

/// Topology-preserving-transform state variable filter (low-pass output).
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Svf {
    pub fn set(&mut self, cutoff_hz: f32, resonance: f32, sr: f32) {
        let fc = cutoff_hz.clamp(20.0, sr * 0.45);
        let g = (PI * fc / sr).tan();
        let k = 2.0 - 1.98 * resonance.clamp(0.0, 1.0);
        self.a1 = 1.0 / (1.0 + g * (g + k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    #[inline]
    pub fn process_lp(&mut self, x: f32) -> f32 {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = flush_denormal(2.0 * v1 - self.ic1);
        self.ic2 = flush_denormal(2.0 * v2 - self.ic2);
        v2
    }
}

/// Transparent below 0.8, smoothly saturates above. Final safety stage, never exceeds 1.0.
#[inline]
pub fn soft_limit(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        let over = (a - KNEE) / (1.0 - KNEE);
        x.signum() * (KNEE + (1.0 - KNEE) * over.tanh())
    }
}

/// Short sine click for the metronome.
#[derive(Clone, Copy, Debug, Default)]
pub struct Click {
    phase: f32,
    inc: f32,
    env: f32,
    decay: f32,
}

impl Click {
    pub fn trigger(&mut self, freq: f32, sr: f32) {
        self.phase = 0.0;
        self.inc = freq / sr;
        self.env = 1.0;
        self.decay = (-1.0 / (0.012 * sr)).exp();
    }

    #[inline]
    pub fn tick(&mut self) -> f32 {
        if self.env < 1e-4 {
            return 0.0;
        }
        let out = (2.0 * PI * self.phase).sin() * self.env;
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        self.env *= self.decay;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adsr_reaches_idle_after_release() {
        let sr = 48000.0;
        let mut e = Adsr::default();
        e.gate_on(0.001, 0.01, 0.5, sr);
        for _ in 0..4800 {
            e.tick();
        }
        assert!((e.level() - 0.5).abs() < 1e-3);
        e.gate_off(0.05, sr);
        for _ in 0..48000 {
            e.tick();
        }
        assert!(e.is_idle());
    }

    #[test]
    fn soft_limit_bounds() {
        for i in -2000..2000 {
            let x = i as f32 * 0.01;
            let y = soft_limit(x);
            assert!(y.abs() <= 1.0);
            if x.abs() <= 0.8 {
                assert_eq!(x, y);
            }
        }
    }

    #[test]
    fn svf_attenuates_highs() {
        let sr = 48000.0;
        let mut f = Svf::default();
        f.set(500.0, 0.1, sr);
        let mut peak: f32 = 0.0;
        for n in 0..4800 {
            let x = (2.0 * PI * 8000.0 * n as f32 / sr).sin();
            peak = peak.max(f.process_lp(x).abs());
        }
        assert!(peak < 0.05, "peak {peak}");
    }
}
