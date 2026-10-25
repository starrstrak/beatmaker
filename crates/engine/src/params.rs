//! Generic parameter descriptors shared by every device (instrument or effect).
//! UI labels and tooltips live in the UI layer, keyed by `key`.

use serde::{Deserialize, Serialize};

pub const MAX_PARAMS: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Curve {
    Linear,
    /// Exponential mapping, for frequencies and times. Requires min > 0.
    Log,
    /// Integer choice (waveform, filter type...); `max` is the last index.
    Choice,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ParamInfo {
    pub key: &'static str,
    pub group: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub curve: Curve,
    pub unit: &'static str,
    /// Shown in beginner mode.
    pub basic: bool,
}

#[allow(clippy::too_many_arguments)]
pub const fn pi(
    key: &'static str,
    group: &'static str,
    min: f32,
    max: f32,
    default: f32,
    curve: Curve,
    unit: &'static str,
    basic: bool,
) -> ParamInfo {
    ParamInfo {
        key,
        group,
        min,
        max,
        default,
        curve,
        unit,
        basic,
    }
}

/// Current values of one device, indexed like its `ParamInfo` table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamValues {
    v: [f32; MAX_PARAMS],
}

impl ParamValues {
    pub fn defaults(infos: &[ParamInfo]) -> Self {
        let mut v = [0.0; MAX_PARAMS];
        for (i, info) in infos.iter().enumerate().take(MAX_PARAMS) {
            v[i] = info.default;
        }
        Self { v }
    }

    #[inline]
    pub fn get(&self, i: impl Into<usize>) -> f32 {
        self.v[i.into()]
    }

    /// Clamps to the parameter range; NaN and out-of-table indices are ignored.
    pub fn set(&mut self, infos: &[ParamInfo], i: usize, value: f32) {
        if let (Some(info), false) = (infos.get(i), value.is_nan()) {
            self.v[i] = value.clamp(info.min, info.max);
        }
    }
}

pub fn index_of(infos: &[ParamInfo], key: &str) -> Option<usize> {
    infos.iter().position(|p| p.key == key)
}

pub fn defaults_vec(infos: &[ParamInfo]) -> Vec<f32> {
    infos.iter().map(|p| p.default).collect()
}

use Curve::{Choice, Linear, Log};

/// Subtractive synth parameter indices (order must match `SUB_PARAMS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum ParamId {
    Osc1Wave,
    Osc2Wave,
    Osc2Detune,
    Osc2Level,
    Cutoff,
    Resonance,
    FilterEnvAmount,
    AmpAttack,
    AmpDecay,
    AmpSustain,
    AmpRelease,
    FilterAttack,
    FilterDecay,
    FilterSustain,
    FilterRelease,
    BendRange,
    ModDepth,
    Level,
}

impl From<ParamId> for usize {
    fn from(p: ParamId) -> usize {
        p as usize
    }
}

pub const SUB_PARAMS: [ParamInfo; 18] = [
    pi("osc1_wave", "osc", 0.0, 3.0, 2.0, Choice, "", true),
    pi("osc2_wave", "osc", 0.0, 3.0, 2.0, Choice, "", false),
    pi("osc2_detune", "osc", -50.0, 50.0, 7.0, Linear, "ct", false),
    pi("osc2_level", "osc", 0.0, 1.0, 0.5, Linear, "", false),
    pi("cutoff", "filter", 40.0, 18000.0, 3500.0, Log, "Hz", true),
    pi("resonance", "filter", 0.0, 0.95, 0.15, Linear, "", true),
    pi("filter_env", "filter", -6.0, 6.0, 1.5, Linear, "oct", false),
    pi("amp_attack", "amp", 0.001, 5.0, 0.005, Log, "s", true),
    pi("amp_decay", "amp", 0.005, 5.0, 0.3, Log, "s", false),
    pi("amp_sustain", "amp", 0.0, 1.0, 0.7, Linear, "", false),
    pi("amp_release", "amp", 0.005, 8.0, 0.25, Log, "s", true),
    pi("flt_attack", "fenv", 0.001, 5.0, 0.005, Log, "s", false),
    pi("flt_decay", "fenv", 0.005, 5.0, 0.35, Log, "s", false),
    pi("flt_sustain", "fenv", 0.0, 1.0, 0.3, Linear, "", false),
    pi("flt_release", "fenv", 0.005, 8.0, 0.3, Log, "s", false),
    pi("bend_range", "perf", 0.0, 12.0, 2.0, Linear, "st", false),
    pi("mod_depth", "perf", 0.0, 2.0, 0.5, Linear, "st", false),
    pi("level", "out", 0.0, 1.0, 0.8, Linear, "", true),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_consistent() {
        assert_eq!(SUB_PARAMS[ParamId::Level as usize].key, "level");
        assert_eq!(SUB_PARAMS[ParamId::Cutoff as usize].key, "cutoff");
        for info in SUB_PARAMS.iter() {
            assert!(info.default >= info.min && info.default <= info.max, "{}", info.key);
            if info.curve == Curve::Log {
                assert!(info.min > 0.0);
            }
        }
    }

    #[test]
    fn set_clamps_and_ignores_nan_and_bad_index() {
        let mut p = ParamValues::defaults(&SUB_PARAMS);
        p.set(&SUB_PARAMS, ParamId::Cutoff as usize, 1e9);
        assert_eq!(p.get(ParamId::Cutoff), 18000.0);
        p.set(&SUB_PARAMS, ParamId::Cutoff as usize, f32::NAN);
        assert_eq!(p.get(ParamId::Cutoff), 18000.0);
        p.set(&SUB_PARAMS, 999, 1.0);
    }
}
