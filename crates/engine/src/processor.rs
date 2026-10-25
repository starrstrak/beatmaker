//! The audio-thread side of the engine.
//!
//! Real-time rules enforced here: no allocation, no locks, no I/O in `process_*`.
//! All state changes arrive through the lock-free command queue and all
//! results leave through the event queue. Anything that must be freed is sent back
//! to the control thread as `Retired`.

use crate::dsp::{soft_limit, Click};
use crate::messages::{Command, Event, Retired, StatusSnapshot, TrackMeter, MAX_TRACKS};
use crate::midi::MidiEvent;
use crate::synth::CHUNK;
use crate::track::{Slot, Track};
use crate::PPQ;
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::time::Instant;

const COMMAND_QUEUE: usize = 4096;
const EVENT_QUEUE: usize = 8192;
const MAX_SCHED: usize = 1024;
const MAX_PENDING_RETIRED: usize = 64;
/// Largest stereo chunk handled per inner pass in `process_device`.
const SCRATCH_FRAMES: usize = 2048;
const STATUS_HZ: f32 = 50.0;
/// Notes played this close before the downbeat are recorded on the downbeat.
const PRE_ROLL_TICKS: f64 = (PPQ / 4) as f64;
const CLICK_TRACK: u8 = u8::MAX;

#[derive(Clone, Copy)]
enum Action {
    Midi(MidiEvent),
    Click { accent: bool },
}

#[derive(Clone, Copy)]
struct Scheduled {
    offset: usize,
    track: u8,
    action: Action,
}

pub struct AudioProcessor {
    sr: f32,
    cmd_rx: Consumer<Command>,
    ev_tx: Producer<Event>,
    #[allow(clippy::vec_box)] // boxed so adding/removing a track moves a pointer, not a whole track
    tracks: Vec<Box<Track>>,
    click: Click,

    armed: Option<u32>,
    overdub: bool,
    song_end: u64,
    looping: bool,
    loop_len: f64,

    playing: bool,
    recording: bool,
    pos_ticks: f64,
    bpm: f32,
    numerator: u8,
    denominator: u8,
    metronome: bool,
    countin_beats: u8,
    next_beat: i64,

    pending: Vec<Retired>,
    sched: [Scheduled; MAX_SCHED],
    sched_len: usize,

    master_target: f32,
    master: f32,
    metro_vol: f32,
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
        let p = Self {
            sr: sample_rate as f32,
            cmd_rx,
            ev_tx,
            tracks: Vec::with_capacity(MAX_TRACKS),
            click: Click::default(),
            armed: None,
            overdub: false,
            song_end: 0,
            looping: false,
            loop_len: 0.0,
            playing: false,
            recording: false,
            pos_ticks: 0.0,
            bpm: 120.0,
            numerator: 4,
            denominator: 4,
            metronome: true,
            countin_beats: 4,
            next_beat: 0,
            pending: Vec::with_capacity(MAX_PENDING_RETIRED),
            sched: [Scheduled {
                offset: 0,
                track: CLICK_TRACK,
                action: Action::Click { accent: false },
            }; MAX_SCHED],
            sched_len: 0,
            master_target: 0.8,
            master: 0.8,
            metro_vol: 0.5,
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
            Err(PushError::Full(Event::Retired(r))) => self.park(r),
            Err(PushError::Full(_)) => self.dropped = self.dropped.saturating_add(1),
        }
    }

    /// Never drop a boxed object on the audio thread: park it and retry next block.
    fn park(&mut self, r: Retired) {
        if self.pending.len() < self.pending.capacity() {
            self.pending.push(r);
        } else {
            std::mem::forget(r);
        }
    }

    fn retire(&mut self, r: Retired) {
        self.send(Event::Retired(r));
    }

    fn retry_pending(&mut self) {
        while let Some(r) = self.pending.pop() {
            match self.ev_tx.push(Event::Retired(r)) {
                Ok(()) => {}
                Err(PushError::Full(Event::Retired(r))) => {
                    self.pending.push(r);
                    break;
                }
                Err(PushError::Full(_)) => break,
            }
        }
    }

    fn track_mut(&mut self, id: u32) -> Option<&mut Box<Track>> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    fn refresh_audibility(&mut self) {
        let any_solo = self.tracks.iter().any(|t| t.solo);
        for t in self.tracks.iter_mut() {
            t.audible = !t.mute && (!any_solo || t.solo);
        }
    }

    fn start_transport(&mut self, record: bool) {
        for t in self.tracks.iter_mut() {
            t.instrument.panic();
            t.clip_idx = 0;
        }
        self.recording = record;
        self.playing = true;
        let countin = if record {
            self.countin_beats as f64 * self.beat_ticks()
        } else {
            0.0
        };
        self.pos_ticks = -countin;
        self.next_beat = (self.pos_ticks / self.beat_ticks()).ceil() as i64;
    }

    /// `cut_notes`: fast-release all voices (user stop). Natural end of song flushes the
    /// remaining clip events (note-offs) instead, so releases ring out.
    fn stop_transport(&mut self, cut_notes: bool) {
        if !self.playing {
            return;
        }
        let was_recording = self.recording;
        self.playing = false;
        self.recording = false;
        for t in self.tracks.iter_mut() {
            if cut_notes {
                t.instrument.panic();
            } else {
                while t.clip_idx < t.clip.events.len() {
                    let ev = t.clip.events[t.clip_idx].event;
                    t.instrument.handle(ev);
                    t.clip_idx += 1;
                }
            }
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
                let (playing, recording, pos, armed) = (self.playing, self.recording, self.pos_ticks, self.armed);
                if let Some(id) = armed {
                    if let Some(t) = self.track_mut(id) {
                        t.instrument.handle(ev);
                    }
                    if playing && recording && pos >= -PRE_ROLL_TICKS {
                        self.send(Event::Recorded {
                            track: id,
                            tick: pos.max(0.0) as u64,
                            event: ev,
                        });
                    }
                }
            }
            Command::AddTrack(t) => {
                if self.tracks.len() < self.tracks.capacity() {
                    self.tracks.push(t);
                    self.refresh_audibility();
                } else {
                    self.retire(Retired::Track(t));
                }
            }
            Command::RemoveTrack(id) => {
                if let Some(i) = self.tracks.iter().position(|t| t.id == id) {
                    let t = self.tracks.remove(i);
                    if self.armed == Some(id) {
                        self.armed = None;
                    }
                    self.retire(Retired::Track(t));
                    self.refresh_audibility();
                }
            }
            Command::SetInstrument { track, instrument } => {
                let mut new = instrument;
                if let Some(t) = self.track_mut(track) {
                    std::mem::swap(&mut t.instrument, &mut *new);
                    self.retire(Retired::Instrument(new));
                } else {
                    self.retire(Retired::Instrument(new));
                }
            }
            Command::SetInstrumentParam { track, index, value } => {
                if let Some(t) = self.track_mut(track) {
                    t.instrument.set_param(index, value);
                }
            }
            Command::InsertEffect {
                track,
                index,
                effect,
                bypass,
            } => {
                let mut rejected = None;
                match self.track_mut(track) {
                    Some(t) if t.effects.len() < t.effects.capacity() => {
                        let i = index.min(t.effects.len());
                        t.effects.insert(i, Slot { effect, bypass });
                    }
                    _ => rejected = Some(effect),
                }
                if let Some(e) = rejected {
                    self.retire(Retired::Effect(e));
                }
            }
            Command::RemoveEffect { track, index } => {
                let removed = self
                    .track_mut(track)
                    .and_then(|t| (index < t.effects.len()).then(|| t.effects.remove(index)));
                if let Some(s) = removed {
                    self.retire(Retired::Effect(s.effect));
                }
            }
            Command::MoveEffect { track, from, to } => {
                if let Some(t) = self.track_mut(track) {
                    if from < t.effects.len() && to < t.effects.len() && from != to {
                        if from < to {
                            t.effects[from..=to].rotate_left(1);
                        } else {
                            t.effects[to..=from].rotate_right(1);
                        }
                    }
                }
            }
            Command::SetEffectBypass { track, index, bypass } => {
                if let Some(s) = self.track_mut(track).and_then(|t| t.effects.get_mut(index)) {
                    s.bypass = bypass;
                }
            }
            Command::SetEffectParam {
                track,
                slot,
                index,
                value,
            } => {
                if let Some(s) = self.track_mut(track).and_then(|t| t.effects.get_mut(slot)) {
                    s.effect.set_param(index, value);
                }
            }
            Command::SetMix {
                track,
                volume,
                pan,
                mute,
                solo,
            } => {
                if let Some(t) = self.track_mut(track) {
                    t.volume = volume.clamp(0.0, 2.0);
                    t.pan = pan.clamp(-1.0, 1.0);
                    t.mute = mute;
                    t.solo = solo;
                }
                self.refresh_audibility();
            }
            Command::SetLfo { track, settings } => {
                if let Some(t) = self.track_mut(track) {
                    t.lfo = settings;
                }
            }
            Command::LoadClip { track, clip } => {
                let pos = self.pos_ticks;
                let mut new = clip;
                if let Some(t) = self.track_mut(track) {
                    std::mem::swap(&mut t.clip, &mut new);
                    t.clip_idx = t.clip.events.partition_point(|e| (e.tick as f64) < pos);
                }
                self.retire(Retired::Clip(new));
            }
            Command::SetArmed(id) => self.armed = id,
            Command::SetOverdub(on) => self.overdub = on,
            Command::SetSongEnd(t) => self.song_end = t,
            Command::SetLoop { enabled, length_ticks } => {
                self.looping = enabled;
                self.loop_len = length_ticks as f64;
            }
            Command::SetMasterGain(g) => self.master_target = g.clamp(0.0, 1.5),
            Command::SetMetronomeVolume(v) => self.metro_vol = v.clamp(0.0, 1.0),
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
            Command::Panic => {
                for t in self.tracks.iter_mut() {
                    t.instrument.panic();
                }
            }
        }
    }

    fn push_sched(&mut self, offset: usize, track: u8, action: Action) {
        if self.sched_len < MAX_SCHED {
            self.sched[self.sched_len] = Scheduled { offset, track, action };
            self.sched_len += 1;
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    /// Fills the schedule with metronome clicks and clip events falling in this segment.
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
                self.push_sched(offset_of(t), CLICK_TRACK, Action::Click { accent });
            }
            self.next_beat += 1;
        }

        for ti in 0..self.tracks.len() {
            let silent = self.recording && !self.overdub && self.armed == Some(self.tracks[ti].id);
            if silent {
                continue;
            }
            loop {
                let t = &self.tracks[ti];
                let Some(te) = t.clip.events.get(t.clip_idx).copied() else {
                    break;
                };
                if (te.tick as f64) >= end {
                    break;
                }
                self.tracks[ti].clip_idx += 1;
                self.push_sched(offset_of(te.tick as f64), ti as u8, Action::Midi(te.event));
            }
        }

        // Stable insertion sort; the schedule is small and mostly ordered already.
        for i in 1..self.sched_len {
            let mut j = i;
            while j > 0 && self.sched[j - 1].offset > self.sched[j].offset {
                self.sched.swap(j - 1, j);
                j -= 1;
            }
        }
    }

    fn apply_action(&mut self, s: Scheduled) {
        match s.action {
            Action::Midi(ev) => {
                if let Some(t) = self.tracks.get_mut(s.track as usize) {
                    t.instrument.handle(ev);
                }
            }
            Action::Click { accent } => self.click.trigger(if accent { 1500.0 } else { 1000.0 }, self.sr),
        }
    }

    fn render_range(&mut self, out: &mut [f32], from: usize, to: usize) {
        let mut mix_l = [0.0f32; CHUNK];
        let mut mix_r = [0.0f32; CHUNK];
        let mut tl = [0.0f32; CHUNK];
        let mut tr = [0.0f32; CHUNK];
        let mut i = from;
        while i < to {
            let m = (to - i).min(CHUNK);
            mix_l[..m].fill(0.0);
            mix_r[..m].fill(0.0);
            for t in self.tracks.iter_mut() {
                t.render(&mut tl[..m], &mut tr[..m]);
                for k in 0..m {
                    mix_l[k] += tl[k];
                    mix_r[k] += tr[k];
                }
            }
            for k in 0..m {
                self.master += (self.master_target - self.master) * 0.003;
                let c = self.click.tick() * self.metro_vol * 0.5;
                let sl = soft_limit((mix_l[k] + c) * self.master);
                let sr = soft_limit((mix_r[k] + c) * self.master);
                out[(i + k) * 2] = sl;
                out[(i + k) * 2 + 1] = sr;
                self.peak_l = self.peak_l.max(sl.abs());
                self.peak_r = self.peak_r.max(sr.abs());
            }
            i += m;
        }
    }

    /// Renders one stretch of audio that does not cross the loop end.
    fn process_segment(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        self.schedule_block(frames);
        let mut cursor = 0;
        let mut si = 0;
        while cursor < frames {
            while si < self.sched_len && self.sched[si].offset <= cursor {
                let s = self.sched[si];
                self.apply_action(s);
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
        }
    }

    fn wrap_loop(&mut self) {
        self.pos_ticks = (self.pos_ticks - self.loop_len).max(0.0);
        self.next_beat = ((self.pos_ticks - 1e-6) / self.beat_ticks()).ceil() as i64;
        let pos = self.pos_ticks;
        for t in self.tracks.iter_mut() {
            t.instrument.panic();
            t.clip_idx = t.clip.events.partition_point(|e| (e.tick as f64) < pos);
        }
    }

    /// Renders interleaved stereo (`out.len()` must be even). Any length is accepted.
    pub fn process_stereo(&mut self, out: &mut [f32]) {
        let started = Instant::now();
        let frames = out.len() / 2;

        while let Ok(cmd) = self.cmd_rx.pop() {
            self.apply_command(cmd);
        }
        self.retry_pending();

        let mut done = 0;
        while done < frames {
            let mut seg = frames - done;
            let loop_active = self.playing && self.looping && self.loop_len > 0.0 && self.pos_ticks >= 0.0;
            if loop_active {
                let remaining = ((self.loop_len - self.pos_ticks) / self.ticks_per_sample())
                    .ceil()
                    .max(1.0) as usize;
                seg = seg.min(remaining);
            }
            self.process_segment(&mut out[done * 2..(done + seg) * 2]);
            done += seg;
            if loop_active && self.pos_ticks >= self.loop_len - 1e-6 {
                self.wrap_loop();
            }
        }

        if self.playing
            && !self.looping
            && !self.recording
            && self.song_end > 0
            && self.pos_ticks >= self.song_end as f64
        {
            self.stop_transport(false);
        }

        let budget = frames as f32 / self.sr;
        if budget > 0.0 {
            self.max_load = self.max_load.max(started.elapsed().as_secs_f32() / budget);
        }
        self.frames_since_status += frames as u32;
        if self.frames_since_status as f32 >= self.sr / STATUS_HZ {
            let mut snap = StatusSnapshot {
                position_ticks: self.pos_ticks,
                peak_l: self.peak_l,
                peak_r: self.peak_r,
                load: self.max_load,
                voices: 0,
                playing: self.playing,
                recording: self.recording,
                counting_in: self.playing && self.pos_ticks < 0.0,
                track_count: self.tracks.len() as u8,
                tracks: [TrackMeter::default(); MAX_TRACKS],
            };
            let mut voices = 0usize;
            for (i, t) in self.tracks.iter_mut().enumerate() {
                let (l, r) = t.take_peaks();
                snap.tracks[i] = TrackMeter {
                    id: t.id,
                    peak_l: l,
                    peak_r: r,
                };
                voices += t.instrument.active_voices();
            }
            snap.voices = voices.min(255) as u8;
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
