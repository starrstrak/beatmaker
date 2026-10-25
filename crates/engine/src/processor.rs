//! The audio-thread side of the engine.
//!
//! Real-time rules enforced here: no allocation, no locks, no I/O in `process_*`.
//! All state changes arrive through the lock-free command queue and all
//! results leave through the event queue.

use crate::clip::Clip;
use crate::dsp::{soft_limit, Click};
use crate::messages::{Command, Event, StatusSnapshot};
use crate::midi::MidiEvent;
use crate::params::{ParamId, ParamSet};
use crate::synth::{Synth, CHUNK};
use crate::PPQ;
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::time::Instant;

const COMMAND_QUEUE: usize = 4096;
const EVENT_QUEUE: usize = 8192;
const MAX_SCHED: usize = 512;
/// Largest stereo chunk handled per inner pass in `process_device`.
const SCRATCH_FRAMES: usize = 2048;
const STATUS_HZ: f32 = 50.0;
/// Notes played this close before the downbeat are recorded on the downbeat.
const PRE_ROLL_TICKS: f64 = (PPQ / 4) as f64;

#[derive(Clone, Copy)]
enum Action {
    Midi(MidiEvent),
    Click { accent: bool },
}

#[derive(Clone, Copy)]
struct Scheduled {
    offset: usize,
    action: Action,
}

pub struct AudioProcessor {
    sr: f32,
    cmd_rx: Consumer<Command>,
    ev_tx: Producer<Event>,
    synth: Synth,
    click: Click,
    params: ParamSet,

    playing: bool,
    recording: bool,
    pos_ticks: f64,
    bpm: f32,
    numerator: u8,
    denominator: u8,
    metronome: bool,
    countin_beats: u8,
    next_beat: i64,

    clip: Box<Clip>,
    clip_idx: usize,
    #[allow(clippy::vec_box)] // boxes are what the event queue carries back to the control thread
    retired: Vec<Box<Clip>>,

    sched: [Scheduled; MAX_SCHED],
    sched_len: usize,

    master: f32,
    scratch: Vec<f32>,

    frames_since_status: u32,
    peak_l: f32,
    peak_r: f32,
    max_load: f32,
    pub dropped: u32,
}

impl AudioProcessor {
    /// Creates the processor plus the control-side queue ends.
    pub fn new(sample_rate: u32) -> (Self, Producer<Command>, Consumer<Event>) {
        let (cmd_tx, cmd_rx) = RingBuffer::new(COMMAND_QUEUE);
        let (ev_tx, ev_rx) = RingBuffer::new(EVENT_QUEUE);
        let params = ParamSet::default();
        let p = Self {
            sr: sample_rate as f32,
            cmd_rx,
            ev_tx,
            synth: Synth::new(sample_rate as f32),
            click: Click::default(),
            params,
            playing: false,
            recording: false,
            pos_ticks: 0.0,
            bpm: 120.0,
            numerator: 4,
            denominator: 4,
            metronome: true,
            countin_beats: 4,
            next_beat: 0,
            clip: Box::new(Clip::default()),
            clip_idx: 0,
            retired: Vec::with_capacity(8),
            sched: [Scheduled {
                offset: 0,
                action: Action::Click { accent: false },
            }; MAX_SCHED],
            sched_len: 0,
            master: params.get(ParamId::MasterGain),
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
            frames_since_status: 0,
            peak_l: 0.0,
            peak_r: 0.0,
            max_load: 0.0,
            dropped: 0,
        };
        (p, cmd_tx, ev_rx)
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr
    }

    #[inline]
    fn ticks_per_sample(&self) -> f64 {
        self.bpm as f64 * PPQ as f64 / (60.0 * self.sr as f64)
    }

    #[inline]
    fn beat_ticks(&self) -> f64 {
        PPQ as f64 * 4.0 / self.denominator as f64
    }

    fn send(&mut self, ev: Event) {
        match self.ev_tx.push(ev) {
            Ok(()) => {}
            Err(PushError::Full(Event::ClipRetired(b))) => {
                // Never drop a Box on the audio thread: park it and retry next block.
                if self.retired.len() < self.retired.capacity() {
                    self.retired.push(b);
                } else {
                    std::mem::forget(b);
                }
            }
            Err(PushError::Full(_)) => self.dropped = self.dropped.saturating_add(1),
        }
    }

    fn retry_retired(&mut self) {
        while let Some(b) = self.retired.pop() {
            match self.ev_tx.push(Event::ClipRetired(b)) {
                Ok(()) => {}
                Err(PushError::Full(Event::ClipRetired(b))) => {
                    self.retired.push(b);
                    break;
                }
                Err(PushError::Full(_)) => break,
            }
        }
    }

    fn start_transport(&mut self, record: bool) {
        self.synth.panic();
        self.recording = record;
        self.playing = true;
        let countin = if record {
            self.countin_beats as f64 * self.beat_ticks()
        } else {
            0.0
        };
        self.pos_ticks = -countin;
        self.next_beat = (self.pos_ticks / self.beat_ticks()).ceil() as i64;
        self.clip_idx = 0;
    }

    /// `cut_notes`: fast-release all voices (user stop). Natural end of clip lets them ring out.
    fn stop_transport(&mut self, cut_notes: bool) {
        if !self.playing {
            return;
        }
        let was_recording = self.recording;
        self.playing = false;
        self.recording = false;
        if cut_notes {
            self.synth.panic();
        }
        let pos = self.pos_ticks;
        self.send(Event::TransportStopped {
            position_ticks: pos,
            was_recording,
        });
    }

    fn apply_command(&mut self, cmd: Command) {
        match cmd {
            Command::Midi(ev) => {
                self.synth.handle(ev);
                if self.playing && self.recording && self.pos_ticks >= -PRE_ROLL_TICKS {
                    let tick = self.pos_ticks.max(0.0) as u64;
                    self.send(Event::Recorded { tick, event: ev });
                }
            }
            Command::SetParam(id, v) => {
                self.params.set(id, v);
                self.synth.set_param(id, v);
            }
            Command::Play { record } => self.start_transport(record),
            Command::Stop => self.stop_transport(true),
            Command::SetTempo(bpm) => {
                if bpm.is_finite() {
                    self.bpm = bpm.clamp(20.0, 300.0);
                }
            }
            Command::SetTimeSignature { numerator, denominator } => {
                if (1..=16).contains(&numerator) && matches!(denominator, 2 | 4 | 8 | 16) {
                    self.numerator = numerator;
                    self.denominator = denominator;
                }
            }
            Command::SetMetronome(on) => self.metronome = on,
            Command::SetCountIn(beats) => self.countin_beats = beats.min(16),
            Command::LoadClip(new) => {
                let old = std::mem::replace(&mut self.clip, new);
                let pos = self.pos_ticks;
                self.clip_idx = self.clip.events.partition_point(|e| (e.tick as f64) < pos);
                self.send(Event::ClipRetired(old));
            }
            Command::Panic => self.synth.panic(),
        }
    }

    fn push_sched(&mut self, offset: usize, action: Action) {
        if self.sched_len < MAX_SCHED {
            self.sched[self.sched_len] = Scheduled { offset, action };
            self.sched_len += 1;
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    /// Fills the schedule with metronome clicks and clip events falling in this block.
    fn schedule_block(&mut self, frames: usize) {
        self.sched_len = 0;
        if !self.playing || frames == 0 {
            return;
        }
        let tps = self.ticks_per_sample();
        let start = self.pos_ticks;
        let end = start + frames as f64 * tps;
        let beat_ticks = self.beat_ticks();
        let offset_of = |tick: f64| (((tick - start) / tps).max(0.0) as usize).min(frames - 1);

        while (self.next_beat as f64) * beat_ticks < end {
            let t = self.next_beat as f64 * beat_ticks;
            if self.metronome {
                let accent = self.next_beat.rem_euclid(self.numerator as i64) == 0;
                self.push_sched(offset_of(t), Action::Click { accent });
            }
            self.next_beat += 1;
        }

        if !self.recording {
            while self.clip_idx < self.clip.events.len() && (self.clip.events[self.clip_idx].tick as f64) < end {
                let te = self.clip.events[self.clip_idx];
                self.push_sched(offset_of(te.tick as f64), Action::Midi(te.event));
                self.clip_idx += 1;
            }
        }

        // Stable insertion sort; the schedule is tiny and mostly ordered already.
        for i in 1..self.sched_len {
            let mut j = i;
            while j > 0 && self.sched[j - 1].offset > self.sched[j].offset {
                self.sched.swap(j - 1, j);
                j -= 1;
            }
        }
    }

    fn apply_action(&mut self, action: Action) {
        match action {
            Action::Midi(ev) => self.synth.handle(ev),
            Action::Click { accent } => self.click.trigger(if accent { 1500.0 } else { 1000.0 }, self.sr),
        }
    }

    fn render_range(&mut self, out: &mut [f32], from: usize, to: usize) {
        let metro_vol = self.params.get(ParamId::MetronomeVolume);
        let master_target = self.params.get(ParamId::MasterGain);
        let mut l = [0.0f32; CHUNK];
        let mut r = [0.0f32; CHUNK];
        let mut i = from;
        while i < to {
            let m = (to - i).min(CHUNK);
            self.synth.render(&mut l[..m], &mut r[..m]);
            for k in 0..m {
                self.master += (master_target - self.master) * 0.003;
                let c = self.click.tick() * metro_vol * 0.5;
                let sl = soft_limit((l[k] + c) * self.master);
                let sr = soft_limit((r[k] + c) * self.master);
                out[(i + k) * 2] = sl;
                out[(i + k) * 2 + 1] = sr;
                self.peak_l = self.peak_l.max(sl.abs());
                self.peak_r = self.peak_r.max(sr.abs());
            }
            i += m;
        }
    }

    /// Renders interleaved stereo (`out.len()` must be even). Any length is accepted.
    pub fn process_stereo(&mut self, out: &mut [f32]) {
        let started = Instant::now();
        let frames = out.len() / 2;

        while let Ok(cmd) = self.cmd_rx.pop() {
            self.apply_command(cmd);
        }
        self.retry_retired();
        self.schedule_block(frames);

        let mut cursor = 0;
        let mut si = 0;
        while cursor < frames {
            while si < self.sched_len && self.sched[si].offset <= cursor {
                let a = self.sched[si].action;
                self.apply_action(a);
                si += 1;
            }
            let end = if si < self.sched_len {
                self.sched[si].offset.min(frames)
            } else {
                frames
            };
            self.render_range(out, cursor, end);
            cursor = end;
        }

        if self.playing {
            self.pos_ticks += frames as f64 * self.ticks_per_sample();
            if !self.recording
                && self.clip.length_ticks > 0
                && self.pos_ticks >= self.clip.length_ticks as f64
                && self.clip_idx >= self.clip.events.len()
            {
                self.stop_transport(false);
            }
        }

        let budget = frames as f32 / self.sr;
        if budget > 0.0 {
            self.max_load = self.max_load.max(started.elapsed().as_secs_f32() / budget);
        }
        self.frames_since_status += frames as u32;
        if self.frames_since_status as f32 >= self.sr / STATUS_HZ {
            let snap = StatusSnapshot {
                position_ticks: self.pos_ticks,
                peak_l: self.peak_l,
                peak_r: self.peak_r,
                load: self.max_load,
                voices: self.synth.active_voices() as u8,
                playing: self.playing,
                recording: self.recording,
                counting_in: self.playing && self.pos_ticks < 0.0,
            };
            self.send(Event::Status(snap));
            self.frames_since_status = 0;
            self.peak_l = 0.0;
            self.peak_r = 0.0;
            self.max_load = 0.0;
        }
    }

    /// Device callback entry: writes `channels` interleaved channels.
    /// Stereo is copied, mono is averaged, extra channels are silent.
    pub fn process_device(&mut self, data: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        if channels == 2 {
            return self.process_stereo(data);
        }
        let total = data.len() / channels;
        let mut done = 0;
        while done < total {
            let n = (total - done).min(SCRATCH_FRAMES);
            let mut scratch = std::mem::take(&mut self.scratch);
            self.process_stereo(&mut scratch[..n * 2]);
            for f in 0..n {
                let (l, r) = (scratch[f * 2], scratch[f * 2 + 1]);
                let frame = &mut data[(done + f) * channels..(done + f + 1) * channels];
                frame.fill(0.0);
                if channels == 1 {
                    frame[0] = 0.5 * (l + r);
                } else {
                    frame[0] = l;
                    frame[1] = r;
                }
            }
            self.scratch = scratch;
            done += n;
        }
    }
}
