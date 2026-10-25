//! Hardware I/O for BeatMaker: audio output (cpal) and MIDI input (midir).
//!
//! Neither service touches the audio-thread rules of the engine: the audio callback only
//! calls `AudioProcessor::process_device`, and MIDI callbacks only push to a lock-free queue.

pub mod audio;
pub mod midi_in;

pub use audio::{list_output_devices, ActiveAudio, AudioDeviceInfo, AudioOptions, AudioService};
pub use midi_in::{MidiInfo, MidiService};

use beatmaker_engine::session::Session;
use std::sync::{Arc, Mutex};

/// The shared session handle used by the UI and by the audio service.
pub type SharedSession = Arc<Mutex<Session>>;

/// Locks a mutex, recovering from poisoning (a panicked UI handler must not kill audio control).
pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
