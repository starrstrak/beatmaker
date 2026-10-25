// Prevents an extra console window on Windows in release; harmless on macOS.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use beatmaker_engine::effects::{EffectKind, EFFECT_KINDS};
use beatmaker_engine::instrument::{InstrumentKind, INSTRUMENT_KINDS};
use beatmaker_engine::midi::MidiEvent;
use beatmaker_engine::quantize::{QuantizeSettings, DIVISIONS};
use beatmaker_engine::render::{BitDepth, ExportOptions};
use beatmaker_engine::session::Session;
use beatmaker_engine::track::{LfoSettings, LfoShape, LfoTarget};
use beatmaker_io::{
    list_output_devices, lock, ActiveAudio, AudioDeviceInfo, AudioOptions, AudioService, MidiService, SharedSession,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::State;

struct AppState {
    session: SharedSession,
    audio: AudioService,
    midi: MidiService,
}

type Res<T> = Result<T, String>;

fn parse<T: serde::de::DeserializeOwned>(what: &str, key: &str) -> Res<T> {
    serde_json::from_value(json!(key)).map_err(|_| format!("unknown {what}: {key}"))
}

// ------------------------------------------------------------------ audio
#[tauri::command]
fn list_audio_devices() -> Vec<AudioDeviceInfo> {
    list_output_devices()
}

#[tauri::command]
fn start_audio(
    state: State<AppState>,
    device: Option<String>,
    sample_rate: Option<u32>,
    buffer_frames: Option<u32>,
) -> Res<ActiveAudio> {
    state.audio.start(
        AudioOptions {
            device,
            sample_rate,
            buffer_frames,
        },
        state.session.clone(),
    )
}

#[tauri::command]
fn stop_audio(state: State<AppState>) {
    state.audio.stop();
}

// ------------------------------------------------------------------ catalogue
/// Static description of every instrument, preset and effect, for building the rack.
#[tauri::command]
fn get_catalog() -> Value {
    let instruments: Vec<Value> = INSTRUMENT_KINDS
        .iter()
        .map(|k| {
            json!({
                "key": k.key(),
                "params": k.params(),
                "presets": k.presets().iter().map(|p| p.name).collect::<Vec<_>>(),
            })
        })
        .collect();
    let effects: Vec<Value> = EFFECT_KINDS
        .iter()
        .map(|k| json!({ "key": k.key(), "params": k.params() }))
        .collect();
    json!({ "instruments": instruments, "effects": effects, "divisions": DIVISIONS })
}

#[tauri::command]
fn get_project(state: State<AppState>) -> Value {
    let s = lock(&state.session);
    json!({ "version": s.version(), "tracks": s.project_view() })
}

// ------------------------------------------------------------------ tracks
#[tauri::command]
fn add_track(state: State<AppState>, kind: String) -> Res<u32> {
    let kind: InstrumentKind = parse("instrument", &kind)?;
    lock(&state.session).add_track(kind)
}

#[tauri::command]
fn remove_track(state: State<AppState>, id: u32) {
    lock(&state.session).remove_track(id);
}

#[tauri::command]
fn rename_track(state: State<AppState>, id: u32, name: String) {
    lock(&state.session).rename_track(id, &name);
}

#[tauri::command]
fn arm_track(state: State<AppState>, id: u32) {
    lock(&state.session).arm(id);
}

#[tauri::command]
fn set_instrument(state: State<AppState>, id: u32, kind: String) -> Res<()> {
    let kind: InstrumentKind = parse("instrument", &kind)?;
    lock(&state.session).set_instrument(id, kind);
    Ok(())
}

#[tauri::command]
fn apply_preset(state: State<AppState>, id: u32, name: String) -> Res<()> {
    lock(&state.session).apply_preset(id, &name)
}

#[tauri::command]
fn set_instrument_param(state: State<AppState>, id: u32, index: usize, value: f32) {
    lock(&state.session).set_instrument_param(id, index, value);
}

#[tauri::command]
fn add_effect(state: State<AppState>, id: u32, kind: String) -> Res<usize> {
    let kind: EffectKind = parse("effect", &kind)?;
    lock(&state.session).add_effect(id, kind)
}

#[tauri::command]
fn remove_effect(state: State<AppState>, id: u32, index: usize) {
    lock(&state.session).remove_effect(id, index);
}

#[tauri::command]
fn move_effect(state: State<AppState>, id: u32, from: usize, to: usize) {
    lock(&state.session).move_effect(id, from, to);
}

#[tauri::command]
fn set_effect_bypass(state: State<AppState>, id: u32, index: usize, bypass: bool) {
    lock(&state.session).set_effect_bypass(id, index, bypass);
}

#[tauri::command]
fn set_effect_param(state: State<AppState>, id: u32, slot: usize, index: usize, value: f32) {
    lock(&state.session).set_effect_param(id, slot, index, value);
}

#[tauri::command]
fn set_mix(
    state: State<AppState>,
    id: u32,
    volume: Option<f32>,
    pan: Option<f32>,
    mute: Option<bool>,
    solo: Option<bool>,
) {
    let mut s = lock(&state.session);
    if let Some(v) = volume {
        s.set_volume(id, v);
    }
    if let Some(p) = pan {
        s.set_pan(id, p);
    }
    if let Some(m) = mute {
        s.set_mute(id, m);
    }
    if let Some(x) = solo {
        s.set_solo(id, x);
    }
}

#[tauri::command]
fn set_lfo(state: State<AppState>, id: u32, shape: String, target: String, rate_hz: f32, depth: f32) -> Res<()> {
    let shape: LfoShape = parse("LFO shape", &shape)?;
    let target: LfoTarget = parse("LFO target", &target)?;
    lock(&state.session).set_lfo(
        id,
        LfoSettings {
            shape,
            target,
            rate_hz,
            depth,
        },
    );
    Ok(())
}

#[tauri::command]
fn set_quantize(state: State<AppState>, id: u32, division: u16, triplet: bool, strength: f32, swing: f32) {
    lock(&state.session).set_quantize(
        id,
        QuantizeSettings {
            division,
            triplet,
            strength,
            swing,
        },
    );
}

#[tauri::command]
fn quantize_track(state: State<AppState>, id: u32) -> Res<()> {
    lock(&state.session).quantize_track(id)
}

#[tauri::command]
fn clear_clip(state: State<AppState>, id: u32) {
    lock(&state.session).clear_clip(id);
}

#[tauri::command]
fn undo(state: State<AppState>) -> bool {
    lock(&state.session).undo()
}

#[tauri::command]
fn redo(state: State<AppState>) -> bool {
    lock(&state.session).redo()
}

// ------------------------------------------------------------------ transport and global settings
#[tauri::command]
fn transport(state: State<AppState>, action: String) -> Res<()> {
    let mut s = lock(&state.session);
    let ok = match action.as_str() {
        "play" => s.play(),
        "record" => s.record(),
        "stop" => s.stop(),
        "panic" => s.panic(),
        other => return Err(format!("unknown transport action {other}")),
    };
    if ok {
        Ok(())
    } else {
        Err("audio is not running or no track is armed: start the audio engine and arm a track".into())
    }
}

#[tauri::command]
fn set_tempo(state: State<AppState>, bpm: f32) {
    lock(&state.session).set_tempo(bpm);
}

#[tauri::command]
fn set_time_signature(state: State<AppState>, numerator: u8, denominator: u8) {
    lock(&state.session).set_time_signature(numerator, denominator);
}

#[tauri::command]
fn set_metronome(state: State<AppState>, on: bool) {
    lock(&state.session).set_metronome(on);
}

#[tauri::command]
fn set_count_in(state: State<AppState>, beats: u8) {
    lock(&state.session).set_count_in(beats);
}

#[tauri::command]
fn set_overdub(state: State<AppState>, on: bool) {
    lock(&state.session).set_overdub(on);
}

#[tauri::command]
fn set_loop(state: State<AppState>, enabled: bool, bars: u32) {
    lock(&state.session).set_loop(enabled, bars);
}

#[tauri::command]
fn set_master(state: State<AppState>, gain: Option<f32>, metronome: Option<f32>) {
    let mut s = lock(&state.session);
    if let Some(g) = gain {
        s.set_master_gain(g);
    }
    if let Some(m) = metronome {
        s.set_metronome_volume(m);
    }
}

/// Computer keyboard / on-screen keys, sent as ordinary MIDI to the armed track.
#[tauri::command]
fn note(state: State<AppState>, note: u8, velocity: u8, on: bool) {
    let ev = if on && velocity > 0 {
        MidiEvent::NoteOn {
            channel: 0,
            note: note.min(127),
            velocity: velocity.min(127),
        }
    } else {
        MidiEvent::NoteOff {
            channel: 0,
            note: note.min(127),
            velocity: 0,
        }
    };
    lock(&state.session).midi(ev);
}

#[tauri::command]
fn sustain(state: State<AppState>, down: bool) {
    lock(&state.session).midi(MidiEvent::ControlChange {
        channel: 0,
        controller: 64,
        value: if down { 127 } else { 0 },
    });
}

/// Called by the UI about 30 times per second.
#[tauri::command]
fn poll(state: State<AppState>) -> Value {
    let mut s = lock(&state.session);
    s.poll();
    let st = s.status();
    let (num, den) = s.time_signature();
    let (loop_on, loop_bars) = s.loop_settings();
    let meters: Vec<Value> = st.tracks[..st.track_count as usize]
        .iter()
        .map(|m| json!({ "id": m.id, "l": m.peak_l, "r": m.peak_r }))
        .collect();
    json!({
        "position_ticks": st.position_ticks,
        "peak_l": st.peak_l, "peak_r": st.peak_r,
        "load": st.load, "voices": st.voices,
        "playing": st.playing, "recording": st.recording, "counting_in": st.counting_in,
        "meters": meters,
        "version": s.version(),
        "tempo": s.tempo(), "numerator": num, "denominator": den,
        "metronome": s.metronome(), "count_in": s.count_in(), "overdub": s.overdub(),
        "loop": loop_on, "loop_bars": loop_bars,
        "master": s.master_gain(), "metro_volume": s.metronome_volume(),
        "can_undo": s.can_undo(), "can_redo": s.can_redo(),
        "stream_errors": state.audio.stream_errors(),
        "midi": state.midi.info(),
    })
}

fn export_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    home.map(|h| h.join("Music").join("BeatMaker"))
        .unwrap_or_else(std::env::temp_dir)
}

#[tauri::command]
async fn export_wav(state: State<'_, AppState>, sample_rate: u32, bits: u32, dither: bool) -> Res<Value> {
    let job = lock(&state.session).export_job()?;
    let dir = export_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("beatmaker-{secs}.wav"));
    let depth = match bits {
        16 => BitDepth::Pcm16,
        32 => BitDepth::Float32,
        _ => BitDepth::Pcm24,
    };
    let opts = ExportOptions {
        sample_rate: sample_rate.clamp(8_000, 192_000),
        bit_depth: depth,
        tail_seconds: 3.0,
        dither,
    };
    let p = path.clone();
    let rep = tauri::async_runtime::spawn_blocking(move || job.run(&p, &opts))
        .await
        .map_err(|e| e.to_string())??;
    Ok(json!({ "path": path.display().to_string(), "seconds": rep.seconds, "peak": rep.peak }))
}

fn main() {
    let session: SharedSession = Arc::new(Mutex::new(Session::with_starter_project()));
    let midi = MidiService::spawn(lock(&session).sender());
    let state = AppState {
        session,
        audio: AudioService::spawn(),
        midi,
    };

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            list_audio_devices,
            start_audio,
            stop_audio,
            get_catalog,
            get_project,
            add_track,
            remove_track,
            rename_track,
            arm_track,
            set_instrument,
            apply_preset,
            set_instrument_param,
            add_effect,
            remove_effect,
            move_effect,
            set_effect_bypass,
            set_effect_param,
            set_mix,
            set_lfo,
            set_quantize,
            quantize_track,
            clear_clip,
            undo,
            redo,
            transport,
            set_tempo,
            set_time_signature,
            set_metronome,
            set_count_in,
            set_overdub,
            set_loop,
            set_master,
            note,
            sustain,
            poll,
            export_wav
        ])
        .run(tauri::generate_context!())
        .expect("error while running BeatMaker");
}
