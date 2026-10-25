//! Control-thread facade. Owns project state and talks to the audio thread via queues.
//! Not real-time: it may allocate and lock.

use crate::clip::{Clip, Note, TimedEvent};
use crate::messages::{Command, Event, StatusSnapshot};
use crate::midi::{MidiEvent, CC_SUSTAIN};
use crate::params::{ParamId, ParamSet};
use crate::processor::AudioProcessor;
use crate::render::{render_clip, write_wav, ExportOptions, ExportReport, Timing};
use crate::PPQ;
use rtrb::{Consumer, Producer};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Cloneable handle that any non-real-time thread (MIDI callbacks, UI) can use to
/// send commands. The audio thread never takes this lock.
#[derive(Clone, Default)]
pub struct SharedSender {
    inner: Arc<Mutex<Option<Producer<Command>>>>,
}

impl SharedSender {
    /// Returns false if no audio engine is attached or the queue is full.
    pub fn send(&self, cmd: Command) -> bool {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(p) => p.push(cmd).is_ok(),
            None => false,
        }
    }

    pub fn midi(&self, ev: MidiEvent) -> bool {
        self.send(Command::Midi(ev))
    }

    fn replace(&self, producer: Producer<Command>) {
        *self.inner.lock().unwrap_or_else(|e| e.into_inner()) = Some(producer);
    }
}

/// A self-contained offline render request. See `Session::export_job`.
pub struct ExportJob {
    clip: Clip,
    params: ParamSet,
    timing: Timing,
}

impl ExportJob {
    pub fn run(&self, path: &Path, opts: &ExportOptions) -> Result<ExportReport, String> {
        let samples = render_clip(
            &self.clip,
            &self.params,
            self.timing,
            opts.sample_rate,
            opts.tail_seconds,
        );
        write_wav(path, &samples, opts.sample_rate, opts.bit_depth, opts.dither)
    }
}

pub struct Session {
    sender: SharedSender,
    events: Option<Consumer<Event>>,
    sample_rate: u32,
    params: ParamSet,
    bpm: f32,
    numerator: u8,
    denominator: u8,
    metronome: bool,
    count_in_beats: u8,
    clip: Clip,
    clip_version: u64,
    take: Vec<TimedEvent>,
    status: StatusSnapshot,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            sender: SharedSender::default(),
            events: None,
            sample_rate: 48_000,
            params: ParamSet::default(),
            bpm: 120.0,
            numerator: 4,
            denominator: 4,
            metronome: true,
            count_in_beats: 4,
            clip: Clip::default(),
            clip_version: 0,
            take: Vec::new(),
            status: StatusSnapshot::default(),
        }
    }

    pub fn sender(&self) -> SharedSender {
        self.sender.clone()
    }

    /// Creates a fresh audio processor (e.g. after the device or sample rate changed)
    /// and replays the full session state into it. Hand the result to the audio callback.
    pub fn attach(&mut self, sample_rate: u32) -> AudioProcessor {
        let (proc, tx, rx) = AudioProcessor::new(sample_rate);
        self.sender.replace(tx);
        self.events = Some(rx);
        self.sample_rate = sample_rate;
        self.status = StatusSnapshot::default();
        self.take.clear();
        for (id, v) in self.params.iter() {
            self.sender.send(Command::SetParam(id, v));
        }
        self.sender.send(Command::SetTempo(self.bpm));
        self.sender.send(Command::SetTimeSignature {
            numerator: self.numerator,
            denominator: self.denominator,
        });
        self.sender.send(Command::SetMetronome(self.metronome));
        self.sender.send(Command::SetCountIn(self.count_in_beats));
        self.sender.send(Command::LoadClip(Box::new(self.clip.clone())));
        proc
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn play(&mut self) -> bool {
        self.sender.send(Command::Play { record: false })
    }

    pub fn record(&mut self) -> bool {
        self.take.clear();
        self.sender.send(Command::Play { record: true })
    }

    pub fn stop(&mut self) -> bool {
        self.sender.send(Command::Stop)
    }

    pub fn panic(&mut self) -> bool {
        self.sender.send(Command::Panic)
    }

    pub fn midi(&self, ev: MidiEvent) -> bool {
        self.sender.midi(ev)
    }

    pub fn set_param(&mut self, id: ParamId, value: f32) {
        self.params.set(id, value);
        self.sender.send(Command::SetParam(id, self.params.get(id)));
    }

    pub fn params(&self) -> &ParamSet {
        &self.params
    }

    pub fn set_tempo(&mut self, bpm: f32) {
        if bpm.is_finite() {
            self.bpm = bpm.clamp(20.0, 300.0);
            self.sender.send(Command::SetTempo(self.bpm));
        }
    }

    pub fn tempo(&self) -> f32 {
        self.bpm
    }

    pub fn set_time_signature(&mut self, numerator: u8, denominator: u8) {
        if (1..=16).contains(&numerator) && matches!(denominator, 2 | 4 | 8 | 16) {
            self.numerator = numerator;
            self.denominator = denominator;
            self.sender.send(Command::SetTimeSignature { numerator, denominator });
        }
    }

    pub fn time_signature(&self) -> (u8, u8) {
        (self.numerator, self.denominator)
    }

    pub fn set_metronome(&mut self, on: bool) {
        self.metronome = on;
        self.sender.send(Command::SetMetronome(on));
    }

    pub fn metronome(&self) -> bool {
        self.metronome
    }

    pub fn set_count_in(&mut self, beats: u8) {
        self.count_in_beats = beats.min(16);
        self.sender.send(Command::SetCountIn(self.count_in_beats));
    }

    pub fn count_in(&self) -> u8 {
        self.count_in_beats
    }

    pub fn status(&self) -> StatusSnapshot {
        self.status
    }

    pub fn clip(&self) -> &Clip {
        &self.clip
    }

    /// Increments whenever the committed clip changes (UI redraw trigger).
    pub fn clip_version(&self) -> u64 {
        self.clip_version
    }

    pub fn notes(&self) -> Vec<Note> {
        self.clip.notes()
    }

    /// Drains audio-thread events. Call regularly (about 30 Hz is plenty).
    pub fn poll(&mut self) {
        let mut stopped: Option<(f64, bool)> = None;
        if let Some(rx) = self.events.as_mut() {
            while let Ok(ev) = rx.pop() {
                match ev {
                    Event::Status(s) => self.status = s,
                    Event::Recorded { tick, event } => self.take.push(TimedEvent { tick, event }),
                    Event::TransportStopped {
                        position_ticks,
                        was_recording,
                    } => {
                        stopped = Some((position_ticks, was_recording));
                    }
                    Event::ClipRetired(_old) => {}
                }
            }
        }
        if let Some((pos, was_recording)) = stopped {
            self.status.playing = false;
            self.status.recording = false;
            self.status.counting_in = false;
            if was_recording {
                self.finalize_take(pos);
            }
        }
    }

    fn finalize_take(&mut self, end_pos: f64) {
        let take = std::mem::take(&mut self.take);
        if !take.iter().any(|e| matches!(e.event, MidiEvent::NoteOn { .. })) {
            return; // nothing played: keep the previous clip
        }
        let bar_ticks = PPQ as u64 * 4 * self.numerator as u64 / self.denominator as u64;
        let last = take.iter().map(|e| e.tick).max().unwrap_or(0);
        let end_tick = (end_pos.max(0.0) as u64).max(last);
        let bars = end_tick.div_ceil(bar_ticks).max(1);
        let length_ticks = bars * bar_ticks;

        let mut events = take;
        // Close notes still held and release the pedal so the clip never leaves hanging notes.
        let mut open = [false; 128 * 16];
        let mut pedal = [false; 16];
        for e in &events {
            match e.event {
                MidiEvent::NoteOn { channel, note, .. } => open[channel as usize * 128 + note as usize] = true,
                MidiEvent::NoteOff { channel, note, .. } => open[channel as usize * 128 + note as usize] = false,
                MidiEvent::ControlChange {
                    channel,
                    controller: CC_SUSTAIN,
                    value,
                } => pedal[channel as usize] = value >= 64,
                _ => {}
            }
        }
        let close_at = end_tick.max(1);
        for (slot, &is_open) in open.iter().enumerate() {
            if is_open {
                events.push(TimedEvent {
                    tick: close_at,
                    event: MidiEvent::NoteOff {
                        channel: (slot / 128) as u8,
                        note: (slot % 128) as u8,
                        velocity: 0,
                    },
                });
            }
        }
        for (ch, &down) in pedal.iter().enumerate() {
            if down {
                events.push(TimedEvent {
                    tick: close_at,
                    event: MidiEvent::ControlChange {
                        channel: ch as u8,
                        controller: CC_SUSTAIN,
                        value: 0,
                    },
                });
            }
        }
        events.sort_by_key(|e| e.tick); // stable: arrival order kept for equal ticks

        self.clip = Clip { events, length_ticks };
        self.clip_version += 1;
        self.sender.send(Command::LoadClip(Box::new(self.clip.clone())));
    }

    /// Snapshot of everything needed to render, so the (slow) render can run without
    /// holding the session lock.
    pub fn export_job(&self) -> Result<ExportJob, String> {
        if self.clip.is_empty() {
            return Err("nothing to export: record something first".into());
        }
        Ok(ExportJob {
            clip: self.clip.clone(),
            params: self.params,
            timing: Timing {
                bpm: self.bpm,
                numerator: self.numerator,
                denominator: self.denominator,
            },
        })
    }

    pub fn export_wav(&self, path: &Path, opts: &ExportOptions) -> Result<ExportReport, String> {
        self.export_job()?.run(path, opts)
    }

    /// Replaces the clip (used by tests, the CLI demo and future project loading).
    pub fn set_clip(&mut self, clip: Clip) {
        self.clip = clip;
        self.clip_version += 1;
        self.sender.send(Command::LoadClip(Box::new(self.clip.clone())));
    }
}
