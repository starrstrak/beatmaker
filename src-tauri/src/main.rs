// Prevents an extra console window on Windows in release; harmless on macOS.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use beatmaker_engine::midi::MidiEvent;
use beatmaker_engine::params::{ParamId, PARAMS};
use beatmaker_engine::render::{BitDepth, ExportOptions};
use beatmaker_engine::session::Session;
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

/// Static parameter metadata plus current values, for building the synth panel.
#[tauri::command]
fn get_params(state: State<AppState>) -> Value {
    let s = lock(&state.session);
    let list: Vec<Value> = PARAMS
        .iter()
        .map(|p| {
            let mut v = serde_json::to_value(p).unwrap_or(Value::Null);
            v["value"] = json!(s.params().get(p.id));
            v
        })
        .collect();
    json!(list)
}

#[tauri::command]
fn set_param(state: State<AppState>, key: String, value: f32) -> Res<()> {
    let id = ParamId::from_key(&key).ok_or_else(|| format!("unknown parameter {key}"))?;
    lock(&state.session).set_param(id, value);
    Ok(())
}

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
        Err("audio is not running: start the audio engine first".into())
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

/// Computer keyboard / on-screen keys, sent as ordinary MIDI.
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
    let midi = state.midi.info();
    json!({
        "position_ticks": st.position_ticks,
        "peak_l": st.peak_l, "peak_r": st.peak_r,
        "load": st.load, "voices": st.voices,
        "playing": st.playing, "recording": st.recording, "counting_in": st.counting_in,
        "clip_version": s.clip_version(),
        "tempo": s.tempo(), "numerator": num, "denominator": den,
        "metronome": s.metronome(), "count_in": s.count_in(),
        "stream_errors": state.audio.stream_errors(),
        "midi": midi,
    })
}

#[tauri::command]
fn get_clip(state: State<AppState>) -> Value {
    let s = lock(&state.session);
    json!({ "version": s.clip_version(), "length_ticks": s.clip().length_ticks, "notes": s.notes() })
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
        tail_seconds: 2.0,
        dither,
    };
    let p = path.clone();
    let rep = tauri::async_runtime::spawn_blocking(move || job.run(&p, &opts))
        .await
        .map_err(|e| e.to_string())??;
    Ok(json!({ "path": path.display().to_string(), "seconds": rep.seconds, "peak": rep.peak }))
}

fn main() {
    let session: SharedSession = Arc::new(Mutex::new(Session::new()));
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
            get_params,
            set_param,
            transport,
            set_tempo,
            set_time_signature,
            set_metronome,
            set_count_in,
            note,
            sustain,
            poll,
            get_clip,
            export_wav
        ])
        .run(tauri::generate_context!())
        .expect("error while running BeatMaker");
}
