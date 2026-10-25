use beatmaker_engine::midi::MidiEvent;
use beatmaker_engine::session::SharedSender;
use midir::{MidiInput, MidiInputConnection};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const SCAN_INTERVAL: Duration = Duration::from_millis(1500);

#[derive(Default)]
struct Shared {
    inputs: Mutex<Vec<String>>,
    last: Mutex<Option<MidiEvent>>,
    count: AtomicU64,
}

#[derive(Clone, Debug, Serialize)]
pub struct MidiInfo {
    /// Names of the MIDI inputs currently connected (auto-detected).
    pub inputs: Vec<String>,
    /// Total messages received; the UI blinks an activity light when it changes.
    pub message_count: u64,
    pub last_message: Option<String>,
}

/// Connects every available MIDI input automatically and re-scans for hot-plugged devices.
pub struct MidiService {
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

fn describe(ev: &MidiEvent) -> String {
    match *ev {
        MidiEvent::NoteOn {
            channel,
            note,
            velocity,
        } => format!("Note on {note} vel {velocity} ch{}", channel + 1),
        MidiEvent::NoteOff { channel, note, .. } => format!("Note off {note} ch{}", channel + 1),
        MidiEvent::ControlChange {
            channel,
            controller,
            value,
        } => format!("CC{controller}={value} ch{}", channel + 1),
        MidiEvent::PitchBend { value, .. } => format!("Pitch bend {value}"),
        MidiEvent::ChannelPressure { pressure, .. } => format!("Aftertouch {pressure}"),
        MidiEvent::PolyPressure { note, pressure, .. } => format!("Poly aftertouch {note}={pressure}"),
    }
}

fn scan_names() -> Vec<String> {
    let Ok(scanner) = MidiInput::new("beatmaker-scan") else {
        return Vec::new();
    };
    let mut seen: HashMap<String, u32> = HashMap::new();
    scanner
        .ports()
        .iter()
        .filter_map(|p| scanner.port_name(p).ok())
        .map(|n| {
            // Two identical devices get distinct keys: "Name", "Name #2".
            let c = seen.entry(n.clone()).or_insert(0);
            *c += 1;
            if *c == 1 {
                n
            } else {
                format!("{n} #{c}")
            }
        })
        .collect()
}

fn connect(name: &str, sender: SharedSender, shared: Arc<Shared>) -> Option<MidiInputConnection<()>> {
    let input = MidiInput::new("beatmaker-in").ok()?;
    let (base, nth) = match name.rsplit_once(" #") {
        Some((b, n)) if n.parse::<usize>().is_ok() => (b, n.parse::<usize>().unwrap()),
        _ => (name, 1),
    };
    let port = input
        .ports()
        .into_iter()
        .filter(|p| input.port_name(p).ok().as_deref() == Some(base))
        .nth(nth - 1)?;
    input
        .connect(
            &port,
            "beatmaker-input",
            move |_ts, bytes, _| {
                if let Some(ev) = MidiEvent::from_bytes(bytes) {
                    sender.midi(ev);
                    shared.count.fetch_add(1, Ordering::Relaxed);
                    if let Ok(mut last) = shared.last.lock() {
                        *last = Some(ev);
                    }
                }
            },
            (),
        )
        .ok()
}

impl MidiService {
    pub fn spawn(sender: SharedSender) -> Self {
        let shared = Arc::new(Shared::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (sh, st) = (shared.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("midi-scan".into())
            .spawn(move || {
                let mut conns: HashMap<String, MidiInputConnection<()>> = HashMap::new();
                while !st.load(Ordering::Relaxed) {
                    let names = scan_names();
                    conns.retain(|n, _| names.contains(n));
                    for n in &names {
                        if !conns.contains_key(n) {
                            if let Some(c) = connect(n, sender.clone(), sh.clone()) {
                                conns.insert(n.clone(), c);
                            }
                        }
                    }
                    let mut connected: Vec<String> = conns.keys().cloned().collect();
                    connected.sort();
                    *sh.inputs.lock().unwrap_or_else(|e| e.into_inner()) = connected;
                    // Sleep in small steps so Drop is quick.
                    let mut waited = Duration::ZERO;
                    while waited < SCAN_INTERVAL && !st.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(50));
                        waited += Duration::from_millis(50);
                    }
                }
            })
            .expect("spawn midi thread");
        Self {
            shared,
            stop,
            thread: Some(thread),
        }
    }

    pub fn info(&self) -> MidiInfo {
        MidiInfo {
            inputs: self.shared.inputs.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            message_count: self.shared.count.load(Ordering::Relaxed),
            last_message: self.shared.last.lock().ok().and_then(|l| l.as_ref().map(describe)),
        }
    }
}

impl Drop for MidiService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
