use crate::{lock, SharedSession};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, SampleRate, SupportedBufferSize};
use serde::Serialize;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

const COMMON_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

#[derive(Clone, Debug, Serialize)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub is_default: bool,
    pub default_sample_rate: u32,
    /// Standard rates the device accepts as f32 output.
    pub sample_rates: Vec<u32>,
    pub min_buffer: Option<u32>,
    pub max_buffer: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct AudioOptions {
    pub device: Option<String>,
    pub sample_rate: Option<u32>,
    /// Requested buffer size in frames; clamped to what the device supports.
    pub buffer_frames: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ActiveAudio {
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// `None` means the driver chose the size.
    pub buffer_frames: Option<u32>,
    pub latency_ms: Option<f32>,
}

fn buffer_range(r: &SupportedBufferSize) -> (Option<u32>, Option<u32>) {
    match r {
        SupportedBufferSize::Range { min, max } => (Some(*min), Some(*max)),
        SupportedBufferSize::Unknown => (None, None),
    }
}

pub fn list_output_devices() -> Vec<AudioDeviceInfo> {
    let host = cpal::default_host();
    let default_name = host.default_output_device().and_then(|d| d.name().ok());
    let Ok(devices) = host.output_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|d| {
            let name = d.name().ok()?;
            let default_sample_rate = d.default_output_config().ok()?.sample_rate().0;
            let ranges: Vec<_> = d
                .supported_output_configs()
                .ok()?
                .filter(|c| c.sample_format() == SampleFormat::F32)
                .collect();
            let mut sample_rates: Vec<u32> = COMMON_RATES
                .iter()
                .copied()
                .filter(|r| {
                    ranges
                        .iter()
                        .any(|c| (c.min_sample_rate().0..=c.max_sample_rate().0).contains(r))
                })
                .collect();
            if sample_rates.is_empty() {
                sample_rates.push(default_sample_rate);
            }
            let (min_buffer, max_buffer) = ranges
                .first()
                .map(|c| buffer_range(c.buffer_size()))
                .unwrap_or((None, None));
            Some(AudioDeviceInfo {
                is_default: Some(&name) == default_name.as_ref(),
                name,
                default_sample_rate,
                sample_rates,
                min_buffer,
                max_buffer,
            })
        })
        .collect()
}

enum Msg {
    Start(AudioOptions, SharedSession, Sender<Result<ActiveAudio, String>>),
    Stop(Sender<()>),
    Shutdown,
}

/// Owns the output stream on a dedicated thread (cpal streams are not `Send` everywhere),
/// so this handle can live in shared application state.
pub struct AudioService {
    tx: Sender<Msg>,
    thread: Option<JoinHandle<()>>,
    stream_errors: Arc<AtomicU32>,
}

impl AudioService {
    pub fn spawn() -> Self {
        let (tx, rx) = channel::<Msg>();
        let stream_errors = Arc::new(AtomicU32::new(0));
        let errs = stream_errors.clone();
        let thread = std::thread::Builder::new()
            .name("audio-owner".into())
            .spawn(move || {
                let mut stream: Option<cpal::Stream> = None;
                while let Ok(msg) = rx.recv() {
                    match msg {
                        Msg::Start(opts, session, reply) => {
                            stream = None; // close the old device first
                            errs.store(0, Ordering::Relaxed);
                            match open_stream(&opts, &session, errs.clone()) {
                                Ok((s, info)) => {
                                    stream = Some(s);
                                    let _ = reply.send(Ok(info));
                                }
                                Err(e) => {
                                    let _ = reply.send(Err(e));
                                }
                            }
                        }
                        Msg::Stop(reply) => {
                            stream = None;
                            let _ = reply.send(());
                        }
                        Msg::Shutdown => break,
                    }
                }
                drop(stream);
            })
            .expect("spawn audio thread");
        Self {
            tx,
            thread: Some(thread),
            stream_errors,
        }
    }

    /// Opens the device and starts audio. Replaces any running stream.
    pub fn start(&self, opts: AudioOptions, session: SharedSession) -> Result<ActiveAudio, String> {
        let (reply, rx) = channel();
        self.tx
            .send(Msg::Start(opts, session, reply))
            .map_err(|_| "audio thread is gone".to_string())?;
        rx.recv().map_err(|_| "audio thread is gone".to_string())?
    }

    pub fn stop(&self) {
        let (reply, rx) = channel();
        if self.tx.send(Msg::Stop(reply)).is_ok() {
            let _ = rx.recv();
        }
    }

    /// Number of stream errors reported by the driver since the last start.
    pub fn stream_errors(&self) -> u32 {
        self.stream_errors.load(Ordering::Relaxed)
    }
}

impl Drop for AudioService {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn open_stream(
    opts: &AudioOptions,
    session: &SharedSession,
    errors: Arc<AtomicU32>,
) -> Result<(cpal::Stream, ActiveAudio), String> {
    let host = cpal::default_host();
    let device = match &opts.device {
        Some(name) => host
            .output_devices()
            .map_err(|e| e.to_string())?
            .find(|d| d.name().ok().as_deref() == Some(name.as_str()))
            .ok_or_else(|| format!("audio device not found: {name}"))?,
        None => host.default_output_device().ok_or("no audio output device available")?,
    };
    let device_name = device.name().unwrap_or_else(|_| "unknown".into());
    let wanted = match opts.sample_rate {
        Some(r) => r,
        None => {
            device
                .default_output_config()
                .map_err(|e| e.to_string())?
                .sample_rate()
                .0
        }
    };

    // Prefer stereo, then the fewest channels >= 1.
    let range = device
        .supported_output_configs()
        .map_err(|e| e.to_string())?
        .filter(|c| c.sample_format() == SampleFormat::F32)
        .filter(|c| (c.min_sample_rate().0..=c.max_sample_rate().0).contains(&wanted))
        .min_by_key(|c| if c.channels() == 2 { 0 } else { c.channels() as u32 + 1 })
        .ok_or_else(|| format!("{device_name} does not support {wanted} Hz with 32-bit float output"))?;

    let channels = range.channels();
    let mut config = range.with_sample_rate(SampleRate(wanted)).config();
    let mut buffer_frames = None;
    if let (Some(req), SupportedBufferSize::Range { min, max }) = (opts.buffer_frames, range.buffer_size()) {
        let n = req.clamp(*min, *max);
        config.buffer_size = BufferSize::Fixed(n);
        buffer_frames = Some(n);
    }

    let mut processor = lock(session).attach(wanted);
    let ch = channels as usize;
    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _| processor.process_device(data, ch),
            move |err| {
                errors.fetch_add(1, Ordering::Relaxed);
                eprintln!("audio stream error: {err}");
            },
            None,
        )
        .map_err(|e| format!("cannot open {device_name}: {e}"))?;
    stream.play().map_err(|e| e.to_string())?;

    let info = ActiveAudio {
        device: device_name,
        sample_rate: wanted,
        channels,
        buffer_frames,
        latency_ms: buffer_frames.map(|b| b as f32 * 1000.0 / wanted as f32),
    };
    Ok((stream, info))
}
