use beatmaker_engine::clip::Clip;
use beatmaker_engine::messages::Command;
use beatmaker_engine::midi::MidiEvent;
use beatmaker_engine::params::ParamId;
use beatmaker_engine::processor::AudioProcessor;
use beatmaker_engine::render::{demo_clip, render_clip, BitDepth, ExportOptions, Timing};
use beatmaker_engine::session::Session;
use beatmaker_engine::PPQ;

const SR: u32 = 48_000;

fn on(note: u8, velocity: u8) -> MidiEvent {
    MidiEvent::NoteOn {
        channel: 0,
        note,
        velocity,
    }
}
fn off(note: u8) -> MidiEvent {
    MidiEvent::NoteOff {
        channel: 0,
        note,
        velocity: 0,
    }
}

/// Renders `frames` stereo frames in blocks of `block`, returning the left channel.
fn run(p: &mut AudioProcessor, frames: usize, block: usize) -> Vec<f32> {
    let mut left = Vec::with_capacity(frames);
    let mut buf = vec![0.0f32; block * 2];
    let mut done = 0;
    while done < frames {
        let n = block.min(frames - done);
        p.process_stereo(&mut buf[..n * 2]);
        left.extend(buf[..n * 2].chunks(2).map(|c| c[0]));
        done += n;
    }
    left
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn rising_crossings(x: &[f32]) -> usize {
    x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
}

fn pure_sine(session: &mut Session) {
    session.set_param(ParamId::Osc1Wave, 0.0);
    session.set_param(ParamId::Osc2Level, 0.0);
    session.set_param(ParamId::Cutoff, 18000.0);
    session.set_param(ParamId::Resonance, 0.0);
    session.set_param(ParamId::FilterEnvAmount, 0.0);
    session.set_param(ParamId::AmpRelease, 0.05);
    session.set_param(ParamId::BendRange, 2.0);
}

fn setup() -> (Session, AudioProcessor) {
    let mut s = Session::new();
    let p = s.attach(SR);
    (s, p)
}

#[test]
fn a4_plays_at_440_hz() {
    let (mut s, mut p) = setup();
    pure_sine(&mut s);
    s.midi(on(69, 127));
    let x = run(&mut p, SR as usize, 256);
    let hz = rising_crossings(&x[SR as usize / 2..]) as f32 * 2.0;
    assert!((hz - 440.0).abs() < 4.0, "measured {hz} Hz");
}

#[test]
fn block_size_does_not_change_the_sound() {
    let render = |block: usize| {
        let (s, mut p) = setup();
        s.midi(on(60, 100));
        s.midi(on(67, 80));
        run(&mut p, 24_000, block)
    };
    let a = render(64);
    let b = render(1000);
    let diff = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
    assert!(diff < 1e-5, "max diff {diff}");
}

#[test]
fn note_off_releases_to_silence() {
    let (mut s, mut p) = setup();
    pure_sine(&mut s);
    s.midi(on(69, 100));
    run(&mut p, 4800, 128);
    s.midi(off(69));
    run(&mut p, SR as usize / 2, 128);
    let tail = run(&mut p, 4800, 128);
    assert!(rms(&tail) < 1e-4, "rms {}", rms(&tail));
}

#[test]
fn sustain_pedal_holds_released_notes() {
    let (mut s, mut p) = setup();
    pure_sine(&mut s);
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
        rms(&held[SR as usize / 2..]) > 0.05,
        "note should still sound under pedal"
    );
    s.midi(MidiEvent::ControlChange {
        channel: 0,
        controller: 64,
        value: 0,
    });
    run(&mut p, SR as usize / 2, 128);
    let after = run(&mut p, 4800, 128);
    assert!(rms(&after) < 1e-4);
}

#[test]
fn pitch_bend_shifts_by_range() {
    let (mut s, mut p) = setup();
    pure_sine(&mut s);
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
    let (s, mut p) = setup();
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
    let (mut s, mut p) = setup();
    s.set_param(ParamId::Resonance, 0.95);
    s.set_param(ParamId::MasterGain, 1.0);
    let mut seed = 7u32;
    let mut peak = 0.0f32;
    for _ in 0..300 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        s.midi(on(24 + (seed >> 24) as u8 % 72, 127));
        let x = run(&mut p, 480, 160);
        assert!(x.iter().all(|v| v.is_finite()));
        peak = x.iter().fold(peak, |m, v| m.max(v.abs()));
    }
    assert!(peak <= 1.0, "peak {peak}");
    assert!(peak > 0.3);
}

#[test]
fn metronome_emits_one_click_per_count_in_beat() {
    let (mut s, mut p) = setup();
    s.set_tempo(120.0);
    s.set_count_in(4);
    s.set_metronome(true);
    s.record();
    // 4 beats @120 = 2 s. Count onsets from silence.
    let x = run(&mut p, SR as usize * 2 - 200, 128);
    let mut onsets = 0;
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
    // The very first click starts at sample 0 (no preceding silence): +1.
    assert_eq!(onsets + 1, 4);
    s.poll();
    assert!(s.status().counting_in || s.status().recording);
}

#[test]
fn record_flow_captures_notes_with_count_in() {
    let (mut s, mut p) = setup();
    s.set_tempo(120.0);
    s.set_count_in(2); // 1 s
    s.record();
    // Count-in: nothing is recorded, note played inside it is ignored (too early).
    run(&mut p, SR as usize / 4, 256);
    s.midi(on(40, 100));
    s.midi(off(40));
    run(&mut p, SR as usize * 3 / 4, 256); // now at downbeat (t=1.0 s)
    run(&mut p, SR as usize / 2, 256); // +0.5 s = 1 beat
    s.midi(on(64, 90));
    run(&mut p, SR as usize / 2, 256);
    s.midi(off(64));
    run(&mut p, SR as usize / 4, 256);
    s.midi(on(67, 70)); // left hanging on purpose
    run(&mut p, SR as usize / 4, 256);
    s.stop();
    run(&mut p, 512, 256);
    s.poll();

    let notes = s.notes();
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert_eq!(notes[0].note, 64);
    let ppq = PPQ as i64;
    assert!(
        (notes[0].start_tick as i64 - ppq).abs() < 30,
        "start {}",
        notes[0].start_tick
    );
    assert!((notes[0].end_tick as i64 - (ppq + ppq)).abs() < 30);
    assert_eq!(notes[0].velocity, 90);
    assert_eq!(notes[1].note, 67, "hanging note must be closed");
    assert!(notes[1].end_tick > notes[1].start_tick);
    assert_eq!(s.clip().length_ticks % (PPQ as u64 * 4), 0);
    assert_eq!(s.clip_version(), 1);
}

#[test]
fn empty_take_keeps_previous_clip() {
    let (mut s, mut p) = setup();
    s.set_clip(demo_clip());
    s.set_count_in(0);
    s.record();
    run(&mut p, 4800, 256);
    s.stop();
    run(&mut p, 256, 256);
    s.poll();
    assert_eq!(s.clip(), &demo_clip());
}

#[test]
fn playback_is_audible_and_stops_at_clip_end() {
    let (mut s, mut p) = setup();
    s.set_metronome(false);
    s.set_tempo(240.0); // demo clip: 16 beats -> 4 s
    s.set_clip(demo_clip());
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
        let (mut s, mut p) = setup();
        s.set_metronome(false);
        s.set_tempo(bpm);
        let mut clip = Clip::default();
        clip.events.push(beatmaker_engine::clip::TimedEvent {
            tick: 0,
            event: on(60, 100),
        });
        clip.events.push(beatmaker_engine::clip::TimedEvent {
            tick: PPQ as u64,
            event: off(60),
        });
        clip.length_ticks = PPQ as u64 * 4;
        s.set_clip(clip);
        s.play();
        let x = run(&mut p, SR as usize * 4, 256);
        // Index where the note is released and audio dies out.
        x.iter().rposition(|v| v.abs() > 0.01).unwrap_or(0) as f32 / SR as f32
    };
    let slow = measure(60.0);
    let fast = measure(120.0);
    assert!(slow > fast + 0.4, "slow {slow} fast {fast}");
}

#[test]
fn offline_render_is_deterministic() {
    let timing = Timing {
        bpm: 120.0,
        numerator: 4,
        denominator: 4,
    };
    let params = Session::new().params().to_owned();
    let a = render_clip(&demo_clip(), &params, timing, 44_100, 1.0);
    let b = render_clip(&demo_clip(), &params, timing, 44_100, 1.0);
    assert_eq!(a, b);
    assert!(rms(&a) > 0.02);
}

#[test]
fn exports_wav_at_192khz_24bit() {
    let mut s = Session::new();
    s.set_clip(demo_clip());
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
    // 16 beats @120 BPM = 8 s + 1 s tail.
    assert_eq!(r.duration() as u64, rep.frames);
    assert!((rep.seconds - 9.0).abs() < 0.01, "{}", rep.seconds);
}

#[test]
fn exports_16bit_and_float() {
    let mut s = Session::new();
    s.set_clip(demo_clip());
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
fn export_of_empty_session_is_an_error() {
    let s = Session::new();
    let err = s.export_wav(&std::env::temp_dir().join("never.wav"), &ExportOptions::default());
    assert!(err.is_err());
}

#[test]
fn device_channel_mapping() {
    let (s, mut p) = setup();
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
    tx.push(Command::Midi(on(69, 127))).ok().unwrap();
    let x = run(&mut p, 4800, 256);
    assert!(rms(&x) > 0.01);
}
