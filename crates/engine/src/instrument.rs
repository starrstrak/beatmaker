//! Instrument devices. A track owns exactly one `Instrument` (enum dispatch, no dyn, no alloc).

use crate::drums::{DrumKit, DRUM_PARAMS};
use crate::fm::{FmSynth, FM_PARAMS};
use crate::midi::MidiEvent;
use crate::params::{index_of, ParamInfo, SUB_PARAMS};
use crate::synth::Synth;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstrumentKind {
    Subtractive,
    Fm,
    Drums,
}

pub const INSTRUMENT_KINDS: [InstrumentKind; 3] =
    [InstrumentKind::Subtractive, InstrumentKind::Fm, InstrumentKind::Drums];

pub struct Preset {
    pub name: &'static str,
    pub values: &'static [(&'static str, f32)],
}

impl InstrumentKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Subtractive => "subtractive",
            Self::Fm => "fm",
            Self::Drums => "drums",
        }
    }

    pub fn params(self) -> &'static [ParamInfo] {
        match self {
            Self::Subtractive => &SUB_PARAMS,
            Self::Fm => &FM_PARAMS,
            Self::Drums => &DRUM_PARAMS,
        }
    }

    pub fn presets(self) -> &'static [Preset] {
        match self {
            Self::Subtractive => SUB_PRESETS,
            Self::Fm => FM_PRESETS,
            Self::Drums => DRUM_PRESETS,
        }
    }

    pub fn preset(self, name: &str) -> Option<&'static Preset> {
        self.presets().iter().find(|p| p.name == name)
    }

    /// Full parameter vector for a preset (unlisted parameters take their defaults).
    pub fn preset_values(self, name: &str) -> Option<Vec<f32>> {
        let preset = self.preset(name)?;
        let infos = self.params();
        let mut v = crate::params::defaults_vec(infos);
        for (key, val) in preset.values {
            if let Some(i) = index_of(infos, key) {
                v[i] = val.clamp(infos[i].min, infos[i].max);
            }
        }
        Some(v)
    }
}

pub enum Instrument {
    Subtractive(Box<Synth>),
    Fm(Box<FmSynth>),
    Drums(Box<DrumKit>),
}

impl Instrument {
    pub fn new(kind: InstrumentKind, sample_rate: f32) -> Self {
        match kind {
            InstrumentKind::Subtractive => Self::Subtractive(Box::new(Synth::new(sample_rate))),
            InstrumentKind::Fm => Self::Fm(Box::new(FmSynth::new(sample_rate))),
            InstrumentKind::Drums => Self::Drums(Box::new(DrumKit::new(sample_rate))),
        }
    }

    pub fn kind(&self) -> InstrumentKind {
        match self {
            Self::Subtractive(_) => InstrumentKind::Subtractive,
            Self::Fm(_) => InstrumentKind::Fm,
            Self::Drums(_) => InstrumentKind::Drums,
        }
    }

    pub fn handle(&mut self, ev: MidiEvent) {
        match self {
            Self::Subtractive(s) => s.handle(ev),
            Self::Fm(s) => s.handle(ev),
            Self::Drums(s) => s.handle(ev),
        }
    }

    pub fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        match self {
            Self::Subtractive(s) => s.render(l, r),
            Self::Fm(s) => s.render(l, r),
            Self::Drums(s) => s.render(l, r),
        }
    }

    pub fn set_param(&mut self, index: usize, value: f32) {
        match self {
            Self::Subtractive(s) => s.set_param(index, value),
            Self::Fm(s) => s.set_param(index, value),
            Self::Drums(s) => s.set_param(index, value),
        }
    }

    pub fn set_params(&mut self, values: &[f32]) {
        for (i, v) in values.iter().enumerate() {
            self.set_param(i, *v);
        }
    }

    /// Track-level modulation: extra pitch in semitones and brightness in octaves.
    pub fn set_modulation(&mut self, pitch_st: f32, bright_oct: f32) {
        match self {
            Self::Subtractive(s) => s.set_modulation(pitch_st, bright_oct),
            Self::Fm(s) => s.set_modulation(pitch_st, bright_oct),
            Self::Drums(s) => s.set_modulation(pitch_st, bright_oct),
        }
    }

    pub fn panic(&mut self) {
        match self {
            Self::Subtractive(s) => s.panic(),
            Self::Fm(s) => s.panic(),
            Self::Drums(s) => s.panic(),
        }
    }

    pub fn active_voices(&self) -> usize {
        match self {
            Self::Subtractive(s) => s.active_voices(),
            Self::Fm(s) => s.active_voices(),
            Self::Drums(s) => s.active_voices(),
        }
    }
}

const SUB_PRESETS: &[Preset] = &[
    Preset {
        name: "Lead",
        values: &[
            ("osc1_wave", 2.0),
            ("osc2_wave", 2.0),
            ("osc2_detune", 9.0),
            ("cutoff", 4500.0),
            ("resonance", 0.25),
            ("filter_env", 1.5),
            ("amp_release", 0.2),
        ],
    },
    Preset {
        name: "Basse",
        values: &[
            ("osc1_wave", 3.0),
            ("osc2_wave", 2.0),
            ("osc2_detune", -5.0),
            ("osc2_level", 0.4),
            ("cutoff", 600.0),
            ("resonance", 0.3),
            ("filter_env", 2.6),
            ("flt_decay", 0.2),
            ("flt_sustain", 0.1),
            ("amp_sustain", 0.85),
            ("amp_release", 0.1),
        ],
    },
    Preset {
        name: "Nappe",
        values: &[
            ("osc1_wave", 2.0),
            ("osc2_wave", 1.0),
            ("osc2_detune", 12.0),
            ("osc2_level", 0.8),
            ("cutoff", 2200.0),
            ("resonance", 0.1),
            ("filter_env", 0.8),
            ("amp_attack", 0.6),
            ("amp_sustain", 0.9),
            ("amp_release", 1.4),
            ("flt_attack", 0.8),
            ("flt_sustain", 0.7),
        ],
    },
    Preset {
        name: "Pluck",
        values: &[
            ("osc1_wave", 2.0),
            ("osc2_level", 0.3),
            ("cutoff", 2500.0),
            ("resonance", 0.2),
            ("filter_env", 3.2),
            ("flt_decay", 0.15),
            ("flt_sustain", 0.0),
            ("amp_decay", 0.28),
            ("amp_sustain", 0.0),
            ("amp_release", 0.15),
        ],
    },
    Preset {
        name: "Orgue",
        values: &[
            ("osc1_wave", 0.0),
            ("osc2_wave", 3.0),
            ("osc2_level", 0.35),
            ("osc2_detune", 0.0),
            ("cutoff", 8000.0),
            ("resonance", 0.0),
            ("filter_env", 0.0),
            ("amp_sustain", 1.0),
            ("amp_release", 0.06),
        ],
    },
    Preset {
        name: "Cuivres",
        values: &[
            ("osc1_wave", 2.0),
            ("osc2_wave", 2.0),
            ("osc2_detune", 6.0),
            ("cutoff", 1800.0),
            ("resonance", 0.12),
            ("filter_env", 2.2),
            ("flt_attack", 0.07),
            ("flt_decay", 0.4),
            ("flt_sustain", 0.6),
            ("amp_attack", 0.06),
            ("amp_sustain", 0.85),
            ("amp_release", 0.18),
        ],
    },
];

const FM_PRESETS: &[Preset] = &[
    Preset {
        name: "Piano électrique",
        values: &[
            ("ratio", 1.0),
            ("index", 2.2),
            ("index_env", 0.8),
            ("index_decay", 0.5),
            ("amp_decay", 1.2),
            ("amp_sustain", 0.25),
            ("amp_release", 0.3),
        ],
    },
    Preset {
        name: "Cloche",
        values: &[
            ("ratio", 3.5),
            ("index", 5.0),
            ("index_env", 0.9),
            ("index_decay", 1.5),
            ("amp_decay", 2.5),
            ("amp_sustain", 0.0),
            ("amp_release", 1.5),
        ],
    },
    Preset {
        name: "Basse FM",
        values: &[
            ("ratio", 1.0),
            ("index", 4.0),
            ("index_env", 0.9),
            ("index_decay", 0.18),
            ("feedback", 0.25),
            ("amp_decay", 0.4),
            ("amp_sustain", 0.7),
            ("amp_release", 0.1),
        ],
    },
    Preset {
        name: "Cuivres FM",
        values: &[
            ("ratio", 1.0),
            ("index", 3.2),
            ("index_env", 0.5),
            ("index_decay", 0.5),
            ("amp_attack", 0.07),
            ("amp_sustain", 0.85),
            ("amp_release", 0.2),
        ],
    },
];

const DRUM_PRESETS: &[Preset] = &[
    Preset {
        name: "Kit 808",
        values: &[
            ("kick_tune", 45.0),
            ("kick_decay", 0.9),
            ("snare_snap", 0.5),
            ("hat_decay", 0.05),
        ],
    },
    Preset {
        name: "Kit Rock",
        values: &[
            ("kick_tune", 62.0),
            ("kick_decay", 0.28),
            ("snare_tone", 220.0),
            ("snare_snap", 0.8),
            ("hat_decay", 0.09),
            ("tom_tune", 0.9),
        ],
    },
    Preset {
        name: "Kit Électro",
        values: &[
            ("kick_tune", 55.0),
            ("kick_decay", 0.45),
            ("snare_snap", 0.35),
            ("hat_decay", 0.03),
            ("tom_tune", 1.4),
        ],
    },
];
