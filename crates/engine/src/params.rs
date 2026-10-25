use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    MasterGain,
    MetronomeVolume,
}

pub const PARAM_COUNT: usize = 19;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Curve {
    Linear,
    /// Exponential mapping, for frequencies and times. Requires min > 0.
    Log,
    /// Integer choice (e.g. waveform); `max` is the last index.
    Choice,
}

/// Static metadata. UI labels/tooltips live in the UI layer (i18n), keyed by `key`.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ParamInfo {
    pub id: ParamId,
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

macro_rules! p {
    ($id:ident, $key:expr, $group:expr, $min:expr, $max:expr, $def:expr, $curve:ident, $unit:expr, $basic:expr) => {
        ParamInfo {
            id: ParamId::$id,
            key: $key,
            group: $group,
            min: $min,
            max: $max,
            default: $def,
            curve: Curve::$curve,
            unit: $unit,
            basic: $basic,
        }
    };
}

pub const PARAMS: [ParamInfo; PARAM_COUNT] = [
    p!(Osc1Wave, "osc1_wave", "osc", 0.0, 3.0, 2.0, Choice, "", true),
    p!(Osc2Wave, "osc2_wave", "osc", 0.0, 3.0, 2.0, Choice, "", false),
    p!(Osc2Detune, "osc2_detune", "osc", -50.0, 50.0, 7.0, Linear, "ct", false),
    p!(Osc2Level, "osc2_level", "osc", 0.0, 1.0, 0.5, Linear, "", false),
    p!(Cutoff, "cutoff", "filter", 40.0, 18000.0, 3500.0, Log, "Hz", true),
    p!(Resonance, "resonance", "filter", 0.0, 0.95, 0.15, Linear, "", true),
    p!(
        FilterEnvAmount,
        "filter_env",
        "filter",
        -6.0,
        6.0,
        1.5,
        Linear,
        "oct",
        false
    ),
    p!(AmpAttack, "amp_attack", "amp", 0.001, 5.0, 0.005, Log, "s", true),
    p!(AmpDecay, "amp_decay", "amp", 0.005, 5.0, 0.3, Log, "s", false),
    p!(AmpSustain, "amp_sustain", "amp", 0.0, 1.0, 0.7, Linear, "", false),
    p!(AmpRelease, "amp_release", "amp", 0.005, 8.0, 0.25, Log, "s", true),
    p!(FilterAttack, "flt_attack", "fenv", 0.001, 5.0, 0.005, Log, "s", false),
    p!(FilterDecay, "flt_decay", "fenv", 0.005, 5.0, 0.35, Log, "s", false),
    p!(FilterSustain, "flt_sustain", "fenv", 0.0, 1.0, 0.3, Linear, "", false),
    p!(FilterRelease, "flt_release", "fenv", 0.005, 8.0, 0.3, Log, "s", false),
    p!(BendRange, "bend_range", "perf", 0.0, 12.0, 2.0, Linear, "st", false),
    p!(ModDepth, "mod_depth", "perf", 0.0, 2.0, 0.5, Linear, "st", false),
    p!(MasterGain, "master", "master", 0.0, 1.0, 0.7, Linear, "", true),
    p!(
        MetronomeVolume,
        "metro_volume",
        "master",
        0.0,
        1.0,
        0.5,
        Linear,
        "",
        true
    ),
];

/// Current value of every parameter, indexed by `ParamId`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamSet {
    values: [f32; PARAM_COUNT],
}

impl Default for ParamSet {
    fn default() -> Self {
        let mut values = [0.0; PARAM_COUNT];
        for info in PARAMS.iter() {
            values[info.id as usize] = info.default;
        }
        Self { values }
    }
}

impl ParamSet {
    #[inline]
    pub fn get(&self, id: ParamId) -> f32 {
        self.values[id as usize]
    }

    /// Clamps to the parameter range; NaN is ignored.
    #[inline]
    pub fn set(&mut self, id: ParamId, value: f32) {
        if value.is_nan() {
            return;
        }
        let info = &PARAMS[id as usize];
        self.values[id as usize] = value.clamp(info.min, info.max);
    }

    pub fn iter(&self) -> impl Iterator<Item = (ParamId, f32)> + '_ {
        PARAMS.iter().map(move |i| (i.id, self.values[i.id as usize]))
    }
}

impl ParamId {
    pub fn from_key(key: &str) -> Option<ParamId> {
        PARAMS.iter().find(|p| p.key == key).map(|p| p.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_ordered_like_the_enum() {
        for (i, info) in PARAMS.iter().enumerate() {
            assert_eq!(info.id as usize, i, "{}", info.key);
            assert!(info.default >= info.min && info.default <= info.max, "{}", info.key);
            if info.curve == Curve::Log {
                assert!(info.min > 0.0);
            }
        }
    }

    #[test]
    fn set_clamps_and_ignores_nan() {
        let mut p = ParamSet::default();
        p.set(ParamId::Cutoff, 1e9);
        assert_eq!(p.get(ParamId::Cutoff), 18000.0);
        p.set(ParamId::Cutoff, f32::NAN);
        assert_eq!(p.get(ParamId::Cutoff), 18000.0);
    }
}
