//! BeatMaker engine: real-time safe audio core.
//!
//! Layout:
//! - `processor`: the audio-thread object (`AudioProcessor`). No alloc, no locks, no I/O.
//! - `session`: the control-thread object (`Session`) that owns the project and talks to the
//!   processor through lock-free queues.
//! - `project`, `track`, `instrument`, `effects`: the device model (tracks, instruments, inserts).
//! - `synth`, `fm`, `drums`: the instruments. `dsp`, `params`: shared building blocks.
//! - `quantize`: grid/swing/strength quantiser.
//! - `render`: offline rendering and WAV export (any sample rate, up to 192 kHz).

pub mod clip;
pub mod drums;
pub mod dsp;
pub mod effects;
pub mod fm;
pub mod instrument;
pub mod messages;
pub mod midi;
pub mod params;
pub mod processor;
pub mod project;
pub mod quantize;
pub mod render;
pub mod session;
pub mod synth;
pub mod track;

/// Musical resolution: ticks per quarter note.
pub const PPQ: u32 = 960;
