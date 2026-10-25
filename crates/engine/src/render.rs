//! Offline rendering and WAV export. Runs the exact same processor as live playback,
//! so what you hear is what you export, at any sample rate (44.1 kHz .. 192 kHz).

use crate::clip::Clip;
use crate::messages::Command;
use crate::midi::MidiEvent;
use crate::params::ParamSet;
use crate::processor::AudioProcessor;
use crate::PPQ;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitDepth {
    Pcm16,
    Pcm24,
    Float32,
}

#[derive(Clone, Copy, Debug)]
pub struct ExportOptions {
    pub sample_rate: u32,
    pub bit_depth: BitDepth,
    /// Extra time after the clip end so releases and tails are not cut.
    pub tail_seconds: f32,
    /// TPDF dither, applied for 16/24-bit output only.
    pub dither: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            bit_depth: BitDepth::Pcm24,
            tail_seconds: 2.0,
            dither: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ExportReport {
    pub frames: u64,
    pub seconds: f32,
    /// Peak of the float mix before quantisation (1.0 = full scale).
    pub peak: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub bpm: f32,
    pub numerator: u8,
    pub denominator: u8,
}

/// Renders the clip through the synth to interleaved stereo f32.
pub fn render_clip(clip: &Clip, params: &ParamSet, timing: Timing, sample_rate: u32, tail_seconds: f32) -> Vec<f32> {
    let (mut proc, mut tx, mut rx) = AudioProcessor::new(sample_rate);
    for (id, v) in params.iter() {
        let _ = tx.push(Command::SetParam(id, v));
    }
    let _ = tx.push(Command::SetTempo(timing.bpm));
    let _ = tx.push(Command::SetTimeSignature {
        numerator: timing.numerator,
        denominator: timing.denominator,
    });
    let _ = tx.push(Command::SetMetronome(false));
    let _ = tx.push(Command::SetCountIn(0));
    let _ = tx.push(Command::LoadClip(Box::new(clip.clone())));
    let _ = tx.push(Command::Play { record: false });

    let tps = timing.bpm as f64 * PPQ as f64 / (60.0 * sample_rate as f64);
    let total = (clip.length_ticks as f64 / tps + tail_seconds as f64 * sample_rate as f64).ceil() as usize;
    let mut out = Vec::with_capacity(total * 2);
    let mut block = vec![0.0f32; 512 * 2];
    while out.len() < total * 2 {
        let n = ((total * 2 - out.len()) / 2).min(512);
        proc.process_stereo(&mut block[..n * 2]);
        out.extend_from_slice(&block[..n * 2]);
        while rx.pop().is_ok() {}
    }
    out
}

struct Xorshift(u32);

impl Xorshift {
    fn next_unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}

pub fn write_wav(
    path: &Path,
    samples: &[f32],
    sample_rate: u32,
    depth: BitDepth,
    dither: bool,
) -> Result<ExportReport, String> {
    let (bits, format) = match depth {
        BitDepth::Pcm16 => (16, hound::SampleFormat::Int),
        BitDepth::Pcm24 => (24, hound::SampleFormat::Int),
        BitDepth::Float32 => (32, hound::SampleFormat::Float),
    };
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: bits,
        sample_format: format,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    let mut rng = Xorshift(0x1234_5678);
    let mut peak = 0.0f32;
    for &s in samples {
        peak = peak.max(s.abs());
        match depth {
            BitDepth::Float32 => w.write_sample(s).map_err(|e| e.to_string())?,
            _ => {
                let max = if bits == 16 { 32767.0 } else { 8_388_607.0 };
                let d = if dither { rng.next_unit() - rng.next_unit() } else { 0.0 };
                let v = (s.clamp(-1.0, 1.0) * max + d).round().clamp(-max - 1.0, max) as i32;
                w.write_sample(v).map_err(|e| e.to_string())?;
            }
        }
    }
    w.finalize().map_err(|e| e.to_string())?;
    let frames = (samples.len() / 2) as u64;
    Ok(ExportReport {
        frames,
        seconds: frames as f32 / sample_rate as f32,
        peak,
    })
}

/// Convenience for the CLI/tests: a clip with a repeated chord pattern.
pub fn demo_clip() -> Clip {
    use crate::clip::TimedEvent;
    let mut events = Vec::new();
    let chords: [[u8; 3]; 4] = [[57, 60, 64], [53, 57, 60], [48, 52, 55], [55, 59, 62]];
    for (bar, chord) in chords.iter().enumerate() {
        let start = bar as u64 * PPQ as u64 * 4;
        for &n in chord {
            events.push(TimedEvent {
                tick: start,
                event: MidiEvent::NoteOn {
                    channel: 0,
                    note: n,
                    velocity: 96,
                },
            });
            events.push(TimedEvent {
                tick: start + PPQ as u64 * 4 - 40,
                event: MidiEvent::NoteOff {
                    channel: 0,
                    note: n,
                    velocity: 0,
                },
            });
        }
    }
    events.sort_by_key(|e| e.tick);
    Clip {
        events,
        length_ticks: PPQ as u64 * 16,
    }
}
