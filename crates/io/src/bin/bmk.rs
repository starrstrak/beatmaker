//! `bmk`: command-line test tool for the BeatMaker engine (no GUI needed).
//!
//!   bmk devices                       list audio outputs and MIDI inputs
//!   bmk demo out.wav [rate] [bits]    render a demo chord loop offline (bits: 16|24|32)
//!   bmk live [--device N] [--rate N] [--buffer N]   play/record with your MIDI keyboard

use beatmaker_engine::quantize::QuantizeSettings;
use beatmaker_engine::render::{demo_clip, demo_drums, BitDepth, ExportOptions};
use beatmaker_engine::session::Session;
use beatmaker_io::{list_output_devices, lock, AudioOptions, AudioService, MidiService};
use std::io::BufRead;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn depth_from(bits: &str) -> BitDepth {
    match bits {
        "16" => BitDepth::Pcm16,
        "32" => BitDepth::Float32,
        _ => BitDepth::Pcm24,
    }
}

fn devices() {
    println!("Audio outputs:");
    for d in list_output_devices() {
        println!(
            "  {}{}  default {} Hz  rates {:?}  buffer {:?}..{:?}",
            d.name,
            if d.is_default { " (default)" } else { "" },
            d.default_sample_rate,
            d.sample_rates,
            d.min_buffer,
            d.max_buffer
        );
    }
    let mut shared = Session::new();
    let midi = MidiService::spawn(shared.sender());
    std::thread::sleep(Duration::from_millis(600));
    println!("MIDI inputs:");
    for n in midi.info().inputs {
        println!("  {n}");
    }
    let _ = &mut shared;
}

fn demo(args: &[String]) -> Result<(), String> {
    let out = args.first().ok_or("usage: bmk demo out.wav [rate] [bits]")?;
    let rate = args.get(1).and_then(|r| r.parse().ok()).unwrap_or(48_000);
    let depth = depth_from(args.get(2).map(String::as_str).unwrap_or("24"));
    let mut s = Session::with_starter_project();
    let ids: Vec<u32> = s.project().tracks.iter().map(|t| t.id).collect();
    s.set_clip(ids[0], demo_clip());
    s.set_clip(ids[2], demo_drums(4));
    let rep = s.export_wav(
        Path::new(out),
        &ExportOptions {
            sample_rate: rate,
            bit_depth: depth,
            tail_seconds: 2.0,
            dither: true,
        },
    )?;
    println!("wrote {out}: {:.1}s at {rate} Hz, peak {:.2}", rep.seconds, rep.peak);
    Ok(())
}

fn live(args: &[String]) -> Result<(), String> {
    let mut opts = AudioOptions::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--device" => opts.device = it.next().cloned(),
            "--rate" => opts.sample_rate = it.next().and_then(|v| v.parse().ok()),
            "--buffer" => opts.buffer_frames = it.next().and_then(|v| v.parse().ok()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    let session = Arc::new(Mutex::new(Session::with_starter_project()));
    let midi = MidiService::spawn(lock(&session).sender());
    let audio = AudioService::spawn();
    let info = audio.start(opts, session.clone())?;
    println!("Audio: {info:?}");

    let quit = Arc::new(AtomicBool::new(false));
    let poller = {
        let (s, q) = (session.clone(), quit.clone());
        std::thread::spawn(move || {
            let mut version = 0;
            while !q.load(Ordering::Relaxed) {
                let mut g = lock(&s);
                g.poll();
                if g.version() != version {
                    version = g.version();
                    let n = g.armed().map(|id| g.notes(id).len()).unwrap_or(0);
                    println!("[project changed: armed track has {n} notes]");
                }
                drop(g);
                std::thread::sleep(Duration::from_millis(20));
            }
        })
    };

    println!("Play your keyboard (goes to the armed track). Commands: l=tracks a <n>=arm track n r=record p=play s=stop z <div>=quantise armed track to 1/div u=undo t <bpm> m=metronome c <beats> e <file.wav> i=info n=notes q=quit");
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let mut parts = line.split_whitespace();
        let mut s = lock(&session);
        match (parts.next(), parts.next()) {
            (Some("r"), _) => {
                s.record();
            }
            (Some("p"), _) => {
                s.play();
            }
            (Some("s"), _) => {
                s.stop();
            }
            (Some("t"), Some(v)) => s.set_tempo(v.parse().unwrap_or(120.0)),
            (Some("m"), _) => {
                let on = !s.metronome();
                s.set_metronome(on);
                println!("metronome {on}");
            }
            (Some("c"), Some(v)) => s.set_count_in(v.parse().unwrap_or(4)),
            (Some("e"), Some(f)) => match s.export_wav(Path::new(f), &ExportOptions::default()) {
                Ok(r) => println!("exported {f}: {:.1}s peak {:.2}", r.seconds, r.peak),
                Err(e) => println!("export failed: {e}"),
            },
            (Some("l"), _) => {
                for (i, t) in s.project().tracks.iter().enumerate() {
                    println!(
                        "  {}: {} ({:?}){}",
                        i + 1,
                        t.name,
                        t.instrument,
                        if s.armed() == Some(t.id) { "  <- armed" } else { "" }
                    );
                }
            }
            (Some("a"), Some(n)) => {
                let id = n
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| s.project().tracks.get(n.wrapping_sub(1)).map(|t| t.id));
                if let Some(id) = id {
                    s.arm(id);
                }
            }
            (Some("z"), Some(d)) => {
                if let Some(id) = s.armed() {
                    s.set_quantize(
                        id,
                        QuantizeSettings {
                            division: d.parse().unwrap_or(16),
                            ..Default::default()
                        },
                    );
                    match s.quantize_track(id) {
                        Ok(()) => println!("quantised to 1/{d}"),
                        Err(e) => println!("{e}"),
                    }
                }
            }
            (Some("u"), _) => println!("undo: {}", s.undo()),
            (Some("n"), _) => {
                for n in s.armed().map(|id| s.notes(id)).unwrap_or_default() {
                    println!("  note {} {}..{} vel {}", n.note, n.start_tick, n.end_tick, n.velocity);
                }
            }
            (Some("i"), _) => {
                let st = s.status();
                let m = midi.info();
                println!(
                    "pos {:.0} ticks, voices {}, load {:.0}%, peak {:.2}/{:.2}, stream errors {}, MIDI {:?} ({} msgs, last {:?})",
                    st.position_ticks, st.voices, st.load * 100.0, st.peak_l, st.peak_r, audio.stream_errors(),
                    m.inputs, m.message_count, m.last_message
                );
            }
            (Some("q"), _) => break,
            _ => {}
        }
    }
    quit.store(true, Ordering::Relaxed);
    let _ = poller.join();
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("devices") => {
            devices();
            Ok(())
        }
        Some("demo") => demo(&args[1..]),
        Some("live") => live(&args[1..]),
        _ => Err("usage: bmk devices | demo out.wav [rate] [bits] | live [--device N] [--rate N] [--buffer N]".into()),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
