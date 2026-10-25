//! Control-thread facade. Owns the project and talks to the audio thread via queues.
//! Not real-time: it may allocate and lock.

use crate::clip::{Clip, Note, TimedEvent};
use crate::effects::{EffectKind, MAX_EFFECTS_PER_TRACK};
use crate::instrument::{Instrument, InstrumentKind};
use crate::messages::{Command, Event, StatusSnapshot, MAX_TRACKS};
use crate::midi::{MidiEvent, CC_SUSTAIN};
use crate::processor::AudioProcessor;
use crate::project::{build_effect, build_instrument, build_track, EffectState, Project, TrackState};
use crate::quantize::{quantize_clip, QuantizeSettings};
use crate::render::{render_project, write_wav, ExportOptions, ExportReport, Timing};
use crate::track::LfoSettings;
use crate::PPQ;
use rtrb::{Consumer, Producer};
use serde::Serialize;
use std::path::Path;
use std::sync::{Arc, Mutex};

const TRACK_COLORS: u8 = 8;

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
    project: Project,
    timing: Timing,
    master: f32,
}

impl ExportJob {
    pub fn run(&self, path: &Path, opts: &ExportOptions) -> Result<ExportReport, String> {
        let samples = render_project(
            &self.project,
            self.timing,
            self.master,
            opts.sample_rate,
            opts.tail_seconds,
        );
        write_wav(path, &samples, opts.sample_rate, opts.bit_depth, opts.dither)
    }
}

struct Snapshot {
    track: u32,
    clip: Clip,
}

/// Read-only view of a track for the UI.
#[derive(Serialize)]
pub struct TrackView {
    pub id: u32,
    pub name: String,
    pub color: u8,
    pub instrument: InstrumentKind,
    pub preset: Option<String>,
    pub params: Vec<f32>,
    pub effects: Vec<EffectState>,
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub armed: bool,
    pub lfo: LfoSettings,
    pub quantize: QuantizeSettings,
    pub clip_length: u64,
    pub notes: Vec<Note>,
}

pub struct Session {
    sender: SharedSender,
    events: Option<Consumer<Event>>,
    sample_rate: u32,
    project: Project,
    armed: Option<u32>,
    bpm: f32,
    numerator: u8,
    denominator: u8,
    metronome: bool,
    count_in_beats: u8,
    master_gain: f32,
    metronome_volume: f32,
    overdub: bool,
    loop_enabled: bool,
    loop_bars: u32,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    take: Vec<TimedEvent>,
    take_track: Option<u32>,
    status: StatusSnapshot,
    version: u64,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    /// An empty project. Use `with_starter_project` for the app's first-run tracks.
    pub fn new() -> Self {
        Self {
            sender: SharedSender::default(),
            events: None,
            sample_rate: 48_000,
            project: Project::default(),
            armed: None,
            bpm: 120.0,
            numerator: 4,
            denominator: 4,
            metronome: true,
            count_in_beats: 4,
            master_gain: 0.8,
            metronome_volume: 0.5,
            overdub: false,
            loop_enabled: false,
            loop_bars: 4,
            undo: Vec::new(),
            redo: Vec::new(),
            take: Vec::new(),
            take_track: None,
            status: StatusSnapshot::default(),
            version: 0,
        }
    }

    /// Lead synth, bass synth and drum kit, ready to play.
    pub fn with_starter_project() -> Self {
        let mut s = Self::new();
        for (kind, preset, name) in [
            (InstrumentKind::Subtractive, "Lead", "Synthé lead"),
            (InstrumentKind::Subtractive, "Basse", "Basse"),
            (InstrumentKind::Drums, "Kit Rock", "Batterie"),
        ] {
            if let Ok(id) = s.add_track(kind) {
                let _ = s.apply_preset(id, preset);
                s.rename_track(id, name);
            }
        }
        s
    }

    pub fn sender(&self) -> SharedSender {
        self.sender.clone()
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn sr(&self) -> f32 {
        self.sample_rate as f32
    }

    fn bump(&mut self) {
        self.version += 1;
    }

    /// Increments on every structural or clip change (UI refresh trigger).
    pub fn version(&self) -> u64 {
        self.version
    }

    fn bar_ticks(&self) -> u64 {
        PPQ as u64 * 4 * self.numerator as u64 / self.denominator as u64
    }

    fn loop_ticks(&self) -> u64 {
        self.bar_ticks() * self.loop_bars as u64
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
        self.take_track = None;
        self.sender.send(Command::SetMasterGain(self.master_gain));
        self.sender.send(Command::SetMetronomeVolume(self.metronome_volume));
        self.sender.send(Command::SetTempo(self.bpm));
        self.sender.send(Command::SetTimeSignature {
            numerator: self.numerator,
            denominator: self.denominator,
        });
        self.sender.send(Command::SetMetronome(self.metronome));
        self.sender.send(Command::SetCountIn(self.count_in_beats));
        self.sender.send(Command::SetOverdub(self.overdub));
        for t in &self.project.tracks {
            self.sender.send(Command::AddTrack(build_track(t, self.sr())));
        }
        self.sender.send(Command::SetArmed(self.armed));
        self.send_loop();
        self.sender.send(Command::SetSongEnd(self.project.song_end()));
        proc
    }

    // ------------------------------------------------------------ tracks
    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn add_track(&mut self, kind: InstrumentKind) -> Result<u32, String> {
        if self.project.tracks.len() >= MAX_TRACKS {
            return Err(format!("limit of {MAX_TRACKS} tracks reached"));
        }
        let id = self.project.next_id;
        self.project.next_id += 1;
        let n = self.project.tracks.len();
        let name = format!("Piste {}", n + 1);
        let state = TrackState::new(id, name, (n as u8) % TRACK_COLORS, kind);
        self.sender.send(Command::AddTrack(build_track(&state, self.sr())));
        self.project.tracks.push(state);
        if self.armed.is_none() {
            self.arm(id);
        }
        self.bump();
        Ok(id)
    }

    pub fn remove_track(&mut self, id: u32) {
        let Some(i) = self.project.tracks.iter().position(|t| t.id == id) else {
            return;
        };
        self.project.tracks.remove(i);
        self.sender.send(Command::RemoveTrack(id));
        self.undo.retain(|s| s.track != id);
        self.redo.retain(|s| s.track != id);
        if self.armed == Some(id) {
            let next = self.project.tracks.first().map(|t| t.id);
            self.armed = None;
            if let Some(n) = next {
                self.arm(n);
            } else {
                self.sender.send(Command::SetArmed(None));
            }
        }
        self.send_song_end();
        self.bump();
    }

    pub fn rename_track(&mut self, id: u32, name: &str) {
        if let Some(t) = self.project.track_mut(id) {
            t.name = name.trim().chars().take(40).collect();
            self.bump();
        }
    }

    pub fn arm(&mut self, id: u32) {
        if self.project.track(id).is_some() {
            self.armed = Some(id);
            self.sender.send(Command::SetArmed(Some(id)));
            self.bump();
        }
    }

    pub fn armed(&self) -> Option<u32> {
        self.armed
    }

    pub fn set_instrument(&mut self, id: u32, kind: InstrumentKind) {
        let sr = self.sr();
        let Some(t) = self.project.track_mut(id) else { return };
        if t.instrument == kind {
            return;
        }
        t.instrument = kind;
        t.preset = None;
        t.params = crate::params::defaults_vec(kind.params());
        let inst = build_instrument(kind, &t.params, sr);
        self.sender.send(Command::SetInstrument {
            track: id,
            instrument: Box::new(inst),
        });
        self.bump();
    }

    pub fn apply_preset(&mut self, id: u32, name: &str) -> Result<(), String> {
        let t = self.project.track_mut(id).ok_or("unknown track")?;
        let values = t
            .instrument
            .preset_values(name)
            .ok_or_else(|| format!("unknown preset {name}"))?;
        t.params = values.clone();
        t.preset = Some(name.to_string());
        for (i, v) in values.into_iter().enumerate() {
            self.sender.send(Command::SetInstrumentParam {
                track: id,
                index: i,
                value: v,
            });
        }
        self.bump();
        Ok(())
    }

    pub fn set_instrument_param(&mut self, id: u32, index: usize, value: f32) {
        let Some(t) = self.project.track_mut(id) else { return };
        let infos = t.instrument.params();
        if let (Some(info), false) = (infos.get(index), value.is_nan()) {
            let v = value.clamp(info.min, info.max);
            t.params[index] = v;
            t.preset = None;
            self.sender.send(Command::SetInstrumentParam {
                track: id,
                index,
                value: v,
            });
        }
    }

    pub fn add_effect(&mut self, id: u32, kind: EffectKind) -> Result<usize, String> {
        let sr = self.sr();
        let t = self.project.track_mut(id).ok_or("unknown track")?;
        if t.effects.len() >= MAX_EFFECTS_PER_TRACK {
            return Err(format!("limit of {MAX_EFFECTS_PER_TRACK} effects per track reached"));
        }
        let state = EffectState::new(kind);
        let index = t.effects.len();
        self.sender.send(Command::InsertEffect {
            track: id,
            index,
            effect: Box::new(build_effect(&state, sr)),
            bypass: false,
        });
        t.effects.push(state);
        self.bump();
        Ok(index)
    }

    pub fn remove_effect(&mut self, id: u32, index: usize) {
        if let Some(t) = self.project.track_mut(id) {
            if index < t.effects.len() {
                t.effects.remove(index);
                self.sender.send(Command::RemoveEffect { track: id, index });
                self.bump();
            }
        }
    }

    pub fn move_effect(&mut self, id: u32, from: usize, to: usize) {
        if let Some(t) = self.project.track_mut(id) {
            if from < t.effects.len() && to < t.effects.len() && from != to {
                let e = t.effects.remove(from);
                t.effects.insert(to, e);
                self.sender.send(Command::MoveEffect { track: id, from, to });
                self.bump();
            }
        }
    }

    pub fn set_effect_bypass(&mut self, id: u32, index: usize, bypass: bool) {
        if let Some(e) = self.project.track_mut(id).and_then(|t| t.effects.get_mut(index)) {
            e.bypass = bypass;
            self.sender.send(Command::SetEffectBypass {
                track: id,
                index,
                bypass,
            });
        }
    }

    pub fn set_effect_param(&mut self, id: u32, slot: usize, index: usize, value: f32) {
        if let Some(e) = self.project.track_mut(id).and_then(|t| t.effects.get_mut(slot)) {
            let infos = e.kind.params();
            if let (Some(info), false) = (infos.get(index), value.is_nan()) {
                let v = value.clamp(info.min, info.max);
                e.params[index] = v;
                self.sender.send(Command::SetEffectParam {
                    track: id,
                    slot,
                    index,
                    value: v,
                });
            }
        }
    }

    fn send_mix(&mut self, id: u32) {
        if let Some(t) = self.project.track(id) {
            self.sender.send(Command::SetMix {
                track: id,
                volume: t.volume,
                pan: t.pan,
                mute: t.mute,
                solo: t.solo,
            });
        }
    }

    pub fn set_volume(&mut self, id: u32, v: f32) {
        if let Some(t) = self.project.track_mut(id) {
            t.volume = v.clamp(0.0, 1.5);
        }
        self.send_mix(id);
    }

    pub fn set_pan(&mut self, id: u32, v: f32) {
        if let Some(t) = self.project.track_mut(id) {
            t.pan = v.clamp(-1.0, 1.0);
        }
        self.send_mix(id);
    }

    pub fn set_mute(&mut self, id: u32, on: bool) {
        if let Some(t) = self.project.track_mut(id) {
            t.mute = on;
        }
        self.send_mix(id);
        self.bump();
    }

    pub fn set_solo(&mut self, id: u32, on: bool) {
        if let Some(t) = self.project.track_mut(id) {
            t.solo = on;
        }
        self.send_mix(id);
        self.bump();
    }

    pub fn set_lfo(&mut self, id: u32, settings: LfoSettings) {
        if let Some(t) = self.project.track_mut(id) {
            let s = LfoSettings {
                rate_hz: settings.rate_hz.clamp(0.05, 20.0),
                depth: settings.depth.clamp(0.0, 1.0),
                ..settings
            };
            t.lfo = s;
            self.sender.send(Command::SetLfo { track: id, settings: s });
        }
    }

    pub fn set_master_gain(&mut self, g: f32) {
        self.master_gain = g.clamp(0.0, 1.5);
        self.sender.send(Command::SetMasterGain(self.master_gain));
    }

    pub fn master_gain(&self) -> f32 {
        self.master_gain
    }

    pub fn set_metronome_volume(&mut self, v: f32) {
        self.metronome_volume = v.clamp(0.0, 1.0);
        self.sender.send(Command::SetMetronomeVolume(self.metronome_volume));
    }

    pub fn metronome_volume(&self) -> f32 {
        self.metronome_volume
    }

    // ------------------------------------------------------------ clips, quantise, undo
    fn send_clip(&mut self, id: u32) {
        if let Some(t) = self.project.track(id) {
            self.sender.send(Command::LoadClip {
                track: id,
                clip: Box::new(t.clip.clone()),
            });
        }
        self.send_song_end();
    }

    fn send_song_end(&mut self) {
        self.sender.send(Command::SetSongEnd(self.project.song_end()));
    }

    /// Replaces a track's clip, recording the old one for undo.
    pub fn set_clip(&mut self, id: u32, clip: Clip) {
        let Some(t) = self.project.track_mut(id) else { return };
        let old = std::mem::replace(&mut t.clip, clip);
        self.undo.push(Snapshot { track: id, clip: old });
        self.redo.clear();
        self.send_clip(id);
        self.bump();
    }

    pub fn clip(&self, id: u32) -> Option<&Clip> {
        self.project.track(id).map(|t| &t.clip)
    }

    pub fn notes(&self, id: u32) -> Vec<Note> {
        self.clip(id).map(|c| c.notes()).unwrap_or_default()
    }

    pub fn clear_clip(&mut self, id: u32) {
        if self.clip(id).is_some_and(|c| !c.is_empty()) {
            self.set_clip(id, Clip::default());
        }
    }

    pub fn set_quantize(&mut self, id: u32, q: QuantizeSettings) {
        if let Some(t) = self.project.track_mut(id) {
            t.quantize = QuantizeSettings {
                division: q.division.clamp(1, 128),
                strength: q.strength.clamp(0.0, 1.0),
                swing: q.swing.clamp(0.5, 0.75),
                ..q
            };
            self.bump();
        }
    }

    /// Applies the track's quantise settings to its clip (undoable).
    pub fn quantize_track(&mut self, id: u32) -> Result<(), String> {
        let t = self.project.track(id).ok_or("unknown track")?;
        if t.clip.is_empty() {
            return Err("nothing to quantise: record some notes first".into());
        }
        let out = quantize_clip(&t.clip, &t.quantize);
        self.set_clip(id, out);
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undoes the latest clip change (recording, quantise, clear). History is unlimited.
    pub fn undo(&mut self) -> bool {
        self.swap_history(true)
    }

    pub fn redo(&mut self) -> bool {
        self.swap_history(false)
    }

    fn swap_history(&mut self, undo: bool) -> bool {
        let entry = if undo { self.undo.pop() } else { self.redo.pop() };
        let Some(Snapshot { track, clip }) = entry else {
            return false;
        };
        let Some(t) = self.project.track_mut(track) else {
            return self.swap_history(undo);
        };
        let current = std::mem::replace(&mut t.clip, clip);
        let counter = Snapshot { track, clip: current };
        if undo {
            self.redo.push(counter);
        } else {
            self.undo.push(counter);
        }
        self.send_clip(track);
        self.bump();
        true
    }

    // ------------------------------------------------------------ transport
    pub fn play(&mut self) -> bool {
        self.sender.send(Command::Play { record: false })
    }

    /// Records into the armed track.
    pub fn record(&mut self) -> bool {
        if self.armed.is_none() {
            return false;
        }
        self.take.clear();
        self.take_track = self.armed;
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
            self.send_loop();
            self.bump();
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

    pub fn set_overdub(&mut self, on: bool) {
        self.overdub = on;
        self.sender.send(Command::SetOverdub(on));
    }

    pub fn overdub(&self) -> bool {
        self.overdub
    }

    fn send_loop(&mut self) {
        let len = self.loop_ticks();
        self.sender.send(Command::SetLoop {
            enabled: self.loop_enabled,
            length_ticks: len,
        });
    }

    pub fn set_loop(&mut self, enabled: bool, bars: u32) {
        self.loop_enabled = enabled;
        self.loop_bars = bars.clamp(1, 64);
        self.send_loop();
        self.bump();
    }

    pub fn loop_settings(&self) -> (bool, u32) {
        (self.loop_enabled, self.loop_bars)
    }

    pub fn status(&self) -> StatusSnapshot {
        self.status
    }

    pub fn project_view(&self) -> Vec<TrackView> {
        self.project
            .tracks
            .iter()
            .map(|t| TrackView {
                id: t.id,
                name: t.name.clone(),
                color: t.color,
                instrument: t.instrument,
                preset: t.preset.clone(),
                params: t.params.clone(),
                effects: t.effects.clone(),
                volume: t.volume,
                pan: t.pan,
                mute: t.mute,
                solo: t.solo,
                armed: self.armed == Some(t.id),
                lfo: t.lfo,
                quantize: t.quantize,
                clip_length: t.clip.length_ticks,
                notes: t.clip.notes(),
            })
            .collect()
    }

    /// Drains audio-thread events. Call regularly (about 30 Hz is plenty).
    pub fn poll(&mut self) {
        let mut stopped: Option<(f64, bool)> = None;
        if let Some(rx) = self.events.as_mut() {
            while let Ok(ev) = rx.pop() {
                match ev {
                    Event::Status(s) => self.status = s,
                    Event::Recorded { track, tick, event } => {
                        if self.take_track == Some(track) {
                            self.take.push(TimedEvent { tick, event });
                        }
                    }
                    Event::TransportStopped {
                        position_ticks,
                        was_recording,
                    } => {
                        stopped = Some((position_ticks, was_recording));
                    }
                    Event::Retired(_) => {}
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
        let Some(track) = self.take_track.take() else { return };
        if !take.iter().any(|e| matches!(e.event, MidiEvent::NoteOn { .. })) {
            return; // nothing played: keep the previous clip
        }
        let bar = self.bar_ticks();
        let last = take.iter().map(|e| e.tick).max().unwrap_or(0);
        let end_tick = (end_pos.max(0.0) as u64).max(last);
        let length_ticks = if self.loop_enabled {
            self.loop_ticks()
        } else {
            end_tick.div_ceil(bar).max(1) * bar
        };

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
        let close_at = end_tick.min(length_ticks).max(1);
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
        let mut length = length_ticks;
        if self.overdub {
            if let Some(old) = self.project.track(track) {
                events.extend(old.clip.events.iter().copied());
                length = length.max(old.clip.length_ticks);
            }
        }
        events.sort_by_key(|e| e.tick); // stable: arrival order kept for equal ticks
        self.set_clip(
            track,
            Clip {
                events,
                length_ticks: length,
            },
        );
    }

    // ------------------------------------------------------------ export
    pub fn export_job(&self) -> Result<ExportJob, String> {
        if self.project.tracks.iter().all(|t| t.clip.is_empty()) {
            return Err("nothing to export: record something first".into());
        }
        Ok(ExportJob {
            project: self.project.clone(),
            timing: Timing {
                bpm: self.bpm,
                numerator: self.numerator,
                denominator: self.denominator,
            },
            master: self.master_gain,
        })
    }

    pub fn export_wav(&self, path: &Path, opts: &ExportOptions) -> Result<ExportReport, String> {
        self.export_job()?.run(path, opts)
    }
}

/// Re-exported for tests and callers that build instruments directly.
pub fn new_instrument(kind: InstrumentKind, sr: f32) -> Instrument {
    Instrument::new(kind, sr)
}
