mod common;
use beatmaker_engine::instrument::{InstrumentKind, INSTRUMENT_KINDS};
use beatmaker_engine::messages::Command;
use beatmaker_engine::midi::MidiEvent;
use beatmaker_engine::processor::AudioProcessor;
use beatmaker_engine::render::{demo_clip, demo_drums, BitDepth, ExportOptions};
use beatmaker_engine::session::Session;
use beatmaker_engine::PPQ;
use common::*;

#[test]
fn a4_plays_at_440_hz() {
    let (mut s, mut p, id) = setup();
    pure_sine(&mut s, id);
    s.midi(on(69, 127));
    let x = run(&mut p, SR as usize, 256);
    let hz = rising_crossings(&x[SR as usize / 2..]) as f32 * 2.0;
    assert!((hz - 440.0).abs() < 4.0, "measured {hz} Hz");
}

#[test]
fn block_size_does_not_change_the_sound() {
    let render = |block: usize| {
        let (mut s, mut p, id) = setup();
        let id2 = s.add_track(InstrumentKind::Fm).unwrap();
        s.add_effect(id, beatmaker_engine::effects::EffectKind::Delay).unwrap();
        s.add_effect(id2, beatmaker_engine::effects::EffectKind::Reverb)
            .unwrap();
        s.add_effect(id2, beatmaker_engine::effects::EffectKind::Chorus)
            .unwrap();
        s.set_clip(id, demo_clip());
        s.set_clip(id2, clip_of(&[(0, 800, 60)], 3840));
        s.set_metronome(false);
        s.play();
        run_stereo(&mut p, 40_000, block)
    };
    let a = render(64);
    let b = render(1000);
    let diff = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
    assert!(diff < 1e-4, "max diff {diff}");
}

#[test]
fn note_off_releases_to_silence() {
    let (mut s, mut p, id) = setup();
    pure_sine(&mut s, id);
    s.midi(on(69, 100));
    run(&mut p, 4800, 128);
    s.midi(off(69));
    run(&mut p, SR as usize / 2, 128);
    let tail = run(&mut p, 4800, 128);
    assert!(rms(&tail) < 1e-4, "rms {}", rms(&tail));
}

#[test]
fn sustain_pedal_holds_released_notes() {
    let (mut s, mut p, id) = setup();
    pure_sine(&mut s, id);
    s.midi(MidiEvent::ControlChange {
        channel: 0,
        controller: 64,
        value: 127,
    });
    s.midi(on(69, 100));
    run(&mut p, 4800, 128);
    s.midi(off(69));
    let held = run(&mut p, SR as usize, 128);
    assert!(
        rms(&held[SR as usize / 2..]) > 0.02,
        "note should still sound under pedal"
    );
    s.midi(MidiEvent::ControlChange {
        channel: 0,
        controller: 64,
        value: 0,
    });
    run(&mut p, SR as usize / 2, 128);
    assert!(rms(&run(&mut p, 4800, 128)) < 1e-4);
}

#[test]
fn pitch_bend_shifts_by_range() {
    let (mut s, mut p, id) = setup();
    pure_sine(&mut s, id);
    s.midi(on(69, 127));
    s.midi(MidiEvent::PitchBend {
        channel: 0,
        value: 8191,
    });
    let x = run(&mut p, SR as usize, 256);
    let hz = rising_crossings(&x[SR as usize / 2..]) as f32 * 2.0;
    let expected = 440.0 * (2.0f32 / 12.0).exp2();
    assert!((hz - expected).abs() < 5.0, "measured {hz}, expected {expected}");
}

#[test]
fn aftertouch_and_mod_wheel_stay_finite() {
    let (s, mut p, _id) = setup();
    s.midi(on(60, 100));
    s.midi(MidiEvent::ControlChange {
        channel: 0,
        controller: 1,
        value: 127,
    });
    s.midi(MidiEvent::ChannelPressure {
        channel: 0,
        pressure: 127,
    });
    let x = run(&mut p, SR as usize, 200);
    assert!(x.iter().all(|v| v.is_finite()));
    assert!(rms(&x) > 0.01);
}

#[test]
fn stress_polyphony_never_clips_or_nans() {
    let (mut s, mut p, id) = setup();
    let id2 = s.add_track(InstrumentKind::Fm).unwrap();
    set(&mut s, id, "resonance", 0.95);
    s.set_master_gain(1.0);
    s.set_volume(id, 1.5);
    s.set_volume(id2, 1.5);
    let mut seed = 7u32;
    let mut peak = 0.0f32;
    for k in 0..300 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        s.arm(if k % 2 == 0 { id } else { id2 });
        s.midi(on(24 + (seed >> 24) as u8 % 72, 127));
        let x = run_stereo(&mut p, 480, 160);
        assert!(x.iter().all(|v| v.is_finite()));
        peak = x.iter().fold(peak, |m, v| m.max(v.abs()));
    }
    assert!(peak <= 1.0, "peak {peak}");
    assert!(peak > 0.3);
}

#[test]
fn metronome_emits_one_click_per_count_in_beat() {
    let (mut s, mut p, _id) = setup();
    s.set_tempo(120.0);
    s.set_count_in(4);
    s.set_metronome(true);
    s.record();
    let x = run(&mut p, SR as usize * 2 - 200, 128);
    let mut onsets = 1; // the first click starts at sample 0
    let mut quiet = 0usize;
    for v in &x {
        if v.abs() < 1e-3 {
            quiet += 1;
        } else {
            if quiet > 2000 {
                onsets += 1;
            }
            quiet = 0;
        }
    }
    assert_eq!(onsets, 4);
    s.poll();
    assert!(s.status().counting_in || s.status().recording);
}

#[test]
fn record_flow_captures_notes_into_the_armed_track() {
    let (mut s, mut p, id) = setup();
    let other = s.add_track(InstrumentKind::Fm).unwrap();
    s.arm(id);
    s.set_tempo(120.0);
    s.set_count_in(2); // 1 s
    s.record();
    run(&mut p, SR as usize / 4, 256);
    s.midi(on(40, 100)); // too early: inside the count-in
    s.midi(off(40));
    run(&mut p, SR as usize * 3 / 4, 256); // downbeat
    run(&mut p, SR as usize / 2, 256); // +1 beat
    s.midi(on(64, 90));
    run(&mut p, SR as usize / 2, 256);
    s.midi(off(64));
    run(&mut p, SR as usize / 4, 256);
    s.midi(on(67, 70)); // left hanging on purpose
    run(&mut p, SR as usize / 4, 256);
    s.stop();
    run(&mut p, 512, 256);
    s.poll();

    let notes = s.notes(id);
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert_eq!(notes[0].note, 64);
    let ppq = PPQ as i64;
    assert!(
        (notes[0].start_tick as i64 - ppq).abs() < 30,
        "start {}",
        notes[0].start_tick
    );
    assert!((notes[0].end_tick as i64 - 2 * ppq).abs() < 30);
    assert_eq!(notes[0].velocity, 90);
    assert_eq!(notes[1].note, 67, "hanging note must be closed");
    assert!(notes[1].end_tick > notes[1].start_tick);
    assert_eq!(s.clip(id).unwrap().length_ticks % (PPQ as u64 * 4), 0);
    assert!(s.notes(other).is_empty(), "other tracks must not receive the take");
}

#[test]
fn other_tracks_keep_playing_while_recording() {
    let (mut s, mut p, a) = setup();
    let b = s.add_track(InstrumentKind::Subtractive).unwrap();
    s.set_clip(a, demo_clip());
    s.arm(b);
    s.set_count_in(0);
    s.set_metronome(false);
    s.record();
    let x = run(&mut p, SR as usize / 2, 256);
    assert!(rms(&x) > 0.02, "track A should be audible during the take");
}

#[test]
fn empty_take_keeps_previous_clip() {
    let (mut s, mut p, id) = setup();
    s.set_clip(id, demo_clip());
    s.set_count_in(0);
    s.record();
    run(&mut p, 4800, 256);
    s.stop();
    run(&mut p, 256, 256);
    s.poll();
    assert_eq!(s.clip(id).unwrap(), &demo_clip());
}

#[test]
fn overdub_merges_with_existing_clip() {
    let (mut s, mut p, id) = setup();
    s.set_clip(id, clip_of(&[(0, 400, 50)], 3840));
    s.set_overdub(true);
    s.set_count_in(0);
    s.set_metronome(false);
    s.record();
    run(&mut p, SR as usize / 4, 256);
    s.midi(on(72, 100));
    run(&mut p, SR as usize / 4, 256);
    s.midi(off(72));
    s.stop();
    run(&mut p, 256, 256);
    s.poll();
    let notes = s.notes(id);
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert!(notes.iter().any(|n| n.note == 50) && notes.iter().any(|n| n.note == 72));
}

#[test]
fn replace_recording_discards_old_clip_and_undo_restores_it() {
    let (mut s, mut p, id) = setup();
    s.set_clip(id, demo_clip());
    s.set_count_in(0);
    s.set_metronome(false);
    s.record();
    s.midi(on(72, 100));
    run(&mut p, SR as usize / 4, 256);
    s.midi(off(72));
    s.stop();
    run(&mut p, 256, 256);
    s.poll();
    assert_eq!(s.notes(id).len(), 1);
    assert!(s.undo());
    assert_eq!(s.clip(id).unwrap(), &demo_clip());
    assert!(s.redo());
    assert_eq!(s.notes(id).len(), 1);
    assert!(!s.redo());
}

#[test]
fn playback_is_audible_and_stops_at_clip_end() {
    let (mut s, mut p, id) = setup();
    s.set_metronome(false);
    s.set_tempo(240.0); // demo clip: 16 beats -> 4 s
    s.set_clip(id, demo_clip());
    s.play();
    let first = run(&mut p, SR as usize, 256);
    assert!(rms(&first) > 0.02);
    s.poll();
    assert!(s.status().playing);
    run(&mut p, SR as usize * 4, 256);
    s.poll();
    assert!(!s.status().playing, "transport should auto-stop at clip end");
}

#[test]
fn tempo_scales_playback_length() {
    let measure = |bpm: f32| {
        let (mut s, mut p, id) = setup();
        s.set_metronome(false);
        s.set_tempo(bpm);
        s.set_clip(id, clip_of(&[(0, PPQ as u64, 60)], PPQ as u64 * 4));
        s.play();
        let x = run(&mut p, SR as usize * 4, 256);
        x.iter().rposition(|v| v.abs() > 0.01).unwrap_or(0) as f32 / SR as f32
    };
    let (slow, fast) = (measure(60.0), measure(120.0));
    assert!(slow > fast + 0.4, "slow {slow} fast {fast}");
}

#[test]
fn loop_repeats_the_region() {
    let mut s = Session::new();
    let mut p = s.attach(SR);
    let id = s.add_track(InstrumentKind::Drums).unwrap();
    s.set_metronome(false);
    s.set_tempo(240.0); // one 4/4 bar = 1 s
    s.set_clip(id, clip_of(&[(0, 60, 36)], PPQ as u64 * 4));
    s.set_loop(true, 1);
    s.play();
    let x = run(&mut p, SR as usize * 3, 300);
    s.poll();
    assert!(s.status().playing, "looping transport never stops by itself");
    for k in 0..3 {
        let from = secs(k as f32) + 200;
        assert!(rms(&x[from..from + 1200]) > 0.03, "kick missing at loop {k}");
    }
    assert!(rms(&x[secs(2.8)..secs(2.9)]) < rms(&x[secs(2.0) + 200..secs(2.0) + 1400]) * 0.5);
}

#[test]
fn mute_solo_and_pan() {
    let (mut s, mut p, a) = setup();
    let b = s.add_track(InstrumentKind::Subtractive).unwrap();
    pure_sine(&mut s, a);
    pure_sine(&mut s, b);
    s.set_clip(a, clip_of(&[(0, 15000, 69)], 3840 * 4)); // 440 Hz
    s.set_clip(b, clip_of(&[(0, 15000, 76)], 3840 * 4)); // ~659 Hz
    s.set_metronome(false);
    s.set_tempo(240.0);
    s.set_solo(b, true);
    s.play();
    let x = run(&mut p, SR as usize, 256);
    let hz = rising_crossings(&x[SR as usize / 2..]) as f32 * 2.0;
    assert!((hz - 659.0).abs() < 8.0, "solo should isolate track b, measured {hz}");
    s.set_solo(b, false);
    s.set_mute(a, true);
    s.set_mute(b, true);
    run(&mut p, SR as usize / 4, 256);
    assert!(rms(&run(&mut p, 4800, 256)) < 1e-4, "both muted");
    s.set_mute(a, false);
    s.set_pan(a, -1.0);
    run(&mut p, SR as usize / 4, 256);
    let st = run_stereo(&mut p, 4800, 256);
    assert!(rms(&left(&st)) > 0.03);
    assert!(rms(&right(&st)) < 1e-3, "hard-left pan must silence the right channel");
}

#[test]
fn lfo_volume_tremolo_and_pan_move_the_signal() {
    use beatmaker_engine::track::{LfoSettings, LfoShape, LfoTarget};
    let (mut s, mut p, id) = setup();
    pure_sine(&mut s, id);
    set(&mut s, id, "amp_sustain", 1.0);
    s.set_lfo(
        id,
        LfoSettings {
            shape: LfoShape::Sine,
            target: LfoTarget::Volume,
            rate_hz: 4.0,
            depth: 1.0,
        },
    );
    s.midi(on(69, 127));
    let x = run(&mut p, SR as usize * 2, 256);
    let win = |i: usize| rms(&x[SR as usize / 2 + i * 1200..SR as usize / 2 + (i + 1) * 1200]);
    let (mut lo, mut hi) = (f32::MAX, 0.0f32);
    for i in 0..60 {
        lo = lo.min(win(i));
        hi = hi.max(win(i));
    }
    assert!(hi > lo * 4.0, "tremolo depth too small: {lo}..{hi}");

    s.set_lfo(
        id,
        LfoSettings {
            shape: LfoShape::Triangle,
            target: LfoTarget::Pan,
            rate_hz: 2.0,
            depth: 1.0,
        },
    );
    run(&mut p, 4800, 256);
    let st = run_stereo(&mut p, SR as usize, 256);
    let (l, r) = (left(&st), right(&st));
    let dominance: Vec<f32> = (0..20)
        .map(|i| rms(&l[i * 2400..(i + 1) * 2400]) - rms(&r[i * 2400..(i + 1) * 2400]))
        .collect();
    assert!(
        dominance.iter().any(|d| *d > 0.02) && dominance.iter().any(|d| *d < -0.02),
        "{dominance:?}"
    );
}

#[test]
fn lfo_pitch_modulates_frequency() {
    use beatmaker_engine::track::{LfoSettings, LfoShape, LfoTarget};
    let (mut s, mut p, id) = setup();
    pure_sine(&mut s, id);
    set(&mut s, id, "amp_sustain", 1.0);
    s.set_lfo(
        id,
        LfoSettings {
            shape: LfoShape::Square,
            target: LfoTarget::Pitch,
            rate_hz: 1.0,
            depth: 1.0,
        },
    );
    s.midi(on(69, 127));
    let x = run(&mut p, SR as usize * 2, 256);
    let f = |from: f32| rising_crossings(&x[secs(from)..secs(from + 0.4)]) as f32 / 0.4;
    let (a, b) = (f(0.05), f(0.55));
    assert!(
        (a - b).abs() > 30.0,
        "square pitch LFO should alternate between two pitches: {a} vs {b}"
    );
}

#[test]
fn every_instrument_and_preset_makes_finite_sound() {
    for kind in INSTRUMENT_KINDS {
        for preset in kind.presets() {
            let mut s = Session::new();
            let mut p = s.attach(SR);
            let id = s.add_track(kind).unwrap();
            s.apply_preset(id, preset.name).unwrap();
            for n in [36u8, 38, 42, 46, 49, 60, 72] {
                s.midi(on(n, 100));
            }
            let x = run_stereo(&mut p, SR as usize / 2, 256);
            assert!(x.iter().all(|v| v.is_finite()), "{kind:?}/{}", preset.name);
            assert!(rms(&x) > 0.003, "{kind:?}/{} is silent", preset.name);
            assert!(x.iter().all(|v| v.abs() <= 1.0));
        }
    }
}

#[test]
fn preset_values_stay_inside_parameter_ranges() {
    for kind in INSTRUMENT_KINDS {
        let infos = kind.params();
        for preset in kind.presets() {
            for (key, _) in preset.values {
                assert!(
                    infos.iter().any(|i| i.key == *key),
                    "{kind:?}/{}: unknown key {key}",
                    preset.name
                );
            }
            let v = kind.preset_values(preset.name).unwrap();
            for (i, x) in v.iter().enumerate() {
                assert!(*x >= infos[i].min && *x <= infos[i].max);
            }
        }
    }
}

#[test]
fn drum_kit_ignores_unmapped_notes_and_kick_is_low() {
    let mut s = Session::new();
    let mut p = s.attach(SR);
    let _ = s.add_track(InstrumentKind::Drums).unwrap();
    s.midi(on(20, 127));
    assert!(rms(&run(&mut p, 4800, 256)) < 1e-6);
    s.midi(on(36, 127));
    let x = run(&mut p, SR as usize / 2, 256);
    assert!(rms(&x) > 0.05);
    // Mostly low-frequency content: few zero crossings per second after the transient.
    let hz = rising_crossings(&x[2400..12000]) as f32 / 0.2;
    assert!(hz < 150.0, "kick should be low, measured {hz} Hz");
}

#[test]
fn switching_instrument_keeps_the_track_alive() {
    let (mut s, mut p, id) = setup();
    s.midi(on(60, 100));
    assert!(rms(&run(&mut p, 4800, 256)) > 0.01);
    s.set_instrument(id, InstrumentKind::Fm);
    s.apply_preset(id, "Cloche").unwrap();
    run(&mut p, 256, 256);
    s.midi(on(72, 100));
    assert!(rms(&run(&mut p, 9600, 256)) > 0.01);
    assert_eq!(s.project().track(id).unwrap().instrument, InstrumentKind::Fm);
}

#[test]
fn track_and_effect_limits_are_enforced() {
    let mut s = Session::new();
    for _ in 0..32 {
        s.add_track(InstrumentKind::Subtractive).unwrap();
    }
    assert!(s.add_track(InstrumentKind::Subtractive).is_err());
    let id = s.project().tracks[0].id;
    for _ in 0..8 {
        s.add_effect(id, beatmaker_engine::effects::EffectKind::Drive).unwrap();
    }
    assert!(s.add_effect(id, beatmaker_engine::effects::EffectKind::Drive).is_err());
    s.remove_track(id);
    assert_eq!(s.project().tracks.len(), 31);
    assert!(
        s.armed().is_some(),
        "arming moves to another track when the armed one is removed"
    );
}

#[test]
fn attach_replays_the_whole_project_at_any_sample_rate() {
    let mut s = Session::new();
    let a = s.add_track(InstrumentKind::Subtractive).unwrap();
    s.add_effect(a, beatmaker_engine::effects::EffectKind::Delay).unwrap();
    s.set_clip(a, demo_clip());
    let b = s.add_track(InstrumentKind::Drums).unwrap();
    s.set_clip(b, demo_drums(4));
    s.set_metronome(false);
    for sr in [44_100u32, 96_000] {
        let mut p = s.attach(sr);
        s.play();
        let x = run_stereo(&mut p, sr as usize / 2, 512);
        assert!(rms(&x) > 0.02, "silent at {sr} Hz");
        s.stop();
        run_stereo(&mut p, 512, 512);
        s.poll();
    }
}

#[test]
fn offline_render_is_deterministic_and_respects_mute() {
    let mut s = Session::new();
    let a = s.add_track(InstrumentKind::Subtractive).unwrap();
    let b = s.add_track(InstrumentKind::Drums).unwrap();
    s.set_clip(a, demo_clip());
    s.set_clip(b, demo_drums(4));
    let dir = std::env::temp_dir().join("beatmaker-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let opts = ExportOptions {
        sample_rate: 44_100,
        bit_depth: BitDepth::Float32,
        tail_seconds: 1.0,
        dither: false,
    };
    let read = |name: &str, s: &Session| {
        let path = dir.join(name);
        s.export_wav(&path, &opts).unwrap();
        hound::WavReader::open(&path)
            .unwrap()
            .samples::<f32>()
            .map(|x| x.unwrap())
            .collect::<Vec<f32>>()
    };
    let one = read("d1.wav", &s);
    let two = read("d2.wav", &s);
    assert_eq!(one, two);
    s.set_mute(b, true);
    let muted = read("d3.wav", &s);
    assert!(rms(&muted) < rms(&one), "muting the drums must lower the level");
}

#[test]
fn exports_wav_at_192khz_24bit() {
    let mut s = Session::new();
    let id = s.add_track(InstrumentKind::Subtractive).unwrap();
    s.set_clip(id, demo_clip());
    let dir = std::env::temp_dir().join("beatmaker-tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hd.wav");
    let opts = ExportOptions {
        sample_rate: 192_000,
        bit_depth: BitDepth::Pcm24,
        tail_seconds: 1.0,
        dither: true,
    };
    let rep = s.export_wav(&path, &opts).unwrap();
    assert!(rep.peak > 0.05 && rep.peak <= 1.0);
    let r = hound::WavReader::open(&path).unwrap();
    let spec = r.spec();
    assert_eq!(
        (spec.sample_rate, spec.bits_per_sample, spec.channels),
        (192_000, 24, 2)
    );
    assert_eq!(r.duration() as u64, rep.frames);
    assert!((rep.seconds - 9.0).abs() < 0.01, "{}", rep.seconds);
}

#[test]
fn exports_16bit_and_float() {
    let mut s = Session::new();
    let id = s.add_track(InstrumentKind::Fm).unwrap();
    s.set_clip(id, demo_clip());
    let dir = std::env::temp_dir().join("beatmaker-tests");
    std::fs::create_dir_all(&dir).unwrap();
    for (depth, bits) in [(BitDepth::Pcm16, 16), (BitDepth::Float32, 32)] {
        let path = dir.join(format!("fmt{bits}.wav"));
        let opts = ExportOptions {
            sample_rate: 44_100,
            bit_depth: depth,
            tail_seconds: 0.5,
            dither: true,
        };
        s.export_wav(&path, &opts).unwrap();
        let spec = hound::WavReader::open(&path).unwrap().spec();
        assert_eq!((spec.sample_rate, spec.bits_per_sample), (44_100, bits));
    }
}

#[test]
fn export_of_empty_project_is_an_error() {
    let mut s = Session::new();
    s.add_track(InstrumentKind::Subtractive).unwrap();
    assert!(s
        .export_wav(&std::env::temp_dir().join("never.wav"), &ExportOptions::default())
        .is_err());
}

#[test]
fn device_channel_mapping() {
    let (s, mut p, _id) = setup();
    s.midi(on(60, 100));
    let mut six = vec![9.0f32; 6 * 512];
    p.process_device(&mut six, 6);
    assert!(six.chunks(6).all(|f| f[2..].iter().all(|&v| v == 0.0)));
    assert!(six.chunks(6).any(|f| f[0].abs() > 0.001));
    let mut mono = vec![0.0f32; 512];
    p.process_device(&mut mono, 1);
    assert!(mono.iter().any(|v| v.abs() > 0.001));
}

#[test]
fn commands_before_attach_are_rejected_not_lost_silently() {
    let s = Session::new();
    assert!(!s.midi(on(60, 100)));
}

#[test]
fn raw_command_queue_roundtrip() {
    let (mut p, mut tx, _rx) = AudioProcessor::new(SR);
    let t = beatmaker_engine::project::build_track(
        &beatmaker_engine::project::TrackState::new(1, "t".into(), 0, InstrumentKind::Subtractive),
        SR as f32,
    );
    tx.push(Command::AddTrack(t)).ok().unwrap();
    tx.push(Command::SetArmed(Some(1))).ok().unwrap();
    tx.push(Command::Midi(on(69, 127))).ok().unwrap();
    assert!(rms(&run(&mut p, 4800, 256)) > 0.01);
}
