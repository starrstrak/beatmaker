//! BeatMaker engine: real-time safe audio core.
//!
//! Layout:
//! - `processor`: the audio-thread object (`AudioProcessor`). No alloc, no locks, no I/O.
//! - `session`: the control-thread object (`Session`) that talks to the processor
//!   through lock-free queues and owns the project state (clip, params, tempo).
//! - `synth`, `dsp`, `params`: sound generation.
//! - `render`: offline rendering and WAV export (any sample rate, up to 192 kHz).

pub mod clip;
pub mod dsp;
pub mod messages;
pub mod midi;
pub mod params;
pub mod processor;
pub mod render;
pub mod session;
pub mod synth;

/// Musical resolution: ticks per quarter note.
pub const PPQ: u32 = 960;
