#![allow(dead_code)]
use beatmaker_engine::clip::{Clip, TimedEvent};
use beatmaker_engine::instrument::InstrumentKind;
use beatmaker_engine::midi::MidiEvent;
use beatmaker_engine::params::index_of;
use beatmaker_engine::processor::AudioProcessor;
use beatmaker_engine::session::Session;

pub const SR: u32 = 48_000;

pub fn on(note: u8, velocity: u8) -> MidiEvent {
    MidiEvent::NoteOn {
        channel: 0,
        note,
        velocity,
    }
}
pub fn off(note: u8) -> MidiEvent {
    MidiEvent::NoteOff {
        channel: 0,
        note,
        velocity: 0,
    }
}

/// Renders `frames` stereo frames in blocks of `block`, returning interleaved samples.
pub fn run_stereo(p: &mut AudioProcessor, frames: usize, block: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames * 2);
    let mut buf = vec![0.0f32; block * 2];
    let mut done = 0;
    while done < frames {
        let n = block.min(frames - done);
        p.process_stereo(&mut buf[..n * 2]);
        out.extend_from_slice(&buf[..n * 2]);
        done += n;
    }
    out
}

pub fn left(x: &[f32]) -> Vec<f32> {
    x.chunks(2).map(|c| c[0]).collect()
}
pub fn right(x: &[f32]) -> Vec<f32> {
    x.chunks(2).map(|c| c[1]).collect()
}

/// Left channel only.
pub fn run(p: &mut AudioProcessor, frames: usize, block: usize) -> Vec<f32> {
    left(&run_stereo(p, frames, block))
}

pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

pub fn rising_crossings(x: &[f32]) -> usize {
    x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
}

pub fn secs(s: f32) -> usize {
    (s * SR as f32) as usize
}

pub fn set(s: &mut Session, id: u32, key: &str, v: f32) {
    let kind = s.project().track(id).unwrap().instrument;
    s.set_instrument_param(id, index_of(kind.params(), key).unwrap(), v);
}

pub fn set_fx(s: &mut Session, id: u32, slot: usize, key: &str, v: f32) {
    let kind = s.project().track(id).unwrap().effects[slot].kind;
    s.set_effect_param(id, slot, index_of(kind.params(), key).unwrap(), v);
}

/// A clean sine voice with a short release.
pub fn pure_sine(s: &mut Session, id: u32) {
    set(s, id, "osc1_wave", 0.0);
    set(s, id, "osc2_level", 0.0);
    set(s, id, "cutoff", 18000.0);
    set(s, id, "resonance", 0.0);
    set(s, id, "filter_env", 0.0);
    set(s, id, "amp_release", 0.05);
    set(s, id, "bend_range", 2.0);
}

/// Session + attached processor + one subtractive track (armed).
pub fn setup() -> (Session, AudioProcessor, u32) {
    let mut s = Session::new();
    let p = s.attach(SR);
    let id = s.add_track(InstrumentKind::Subtractive).unwrap();
    (s, p, id)
}

pub fn clip_of(notes: &[(u64, u64, u8)], length: u64) -> Clip {
    let mut events = Vec::new();
    for &(a, b, n) in notes {
        events.push(TimedEvent {
            tick: a,
            event: on(n, 100),
        });
        events.push(TimedEvent { tick: b, event: off(n) });
    }
    events.sort_by_key(|e| e.tick);
    Clip {
        events,
        length_ticks: length,
    }
}
