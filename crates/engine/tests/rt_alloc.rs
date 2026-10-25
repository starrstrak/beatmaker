//! Proves the audio callback never touches the heap: a counting allocator fails the
//! test if `process_*` allocates or frees while a full project plays, records, loops,
//! swaps instruments, inserts and removes effects, and adds and removes tracks.

use beatmaker_engine::effects::EFFECT_KINDS;
use beatmaker_engine::instrument::{InstrumentKind, INSTRUMENT_KINDS};
use beatmaker_engine::midi::MidiEvent;
use beatmaker_engine::render::{demo_clip, demo_drums};
use beatmaker_engine::session::Session;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
thread_local! { static TRACK: Cell<bool> = const { Cell::new(false) }; }

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if TRACK.with(|t| t.get()) {
            ALLOCS.fetch_add(1, Ordering::SeqCst);
        }
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if TRACK.with(|t| t.get()) {
            ALLOCS.fetch_add(1, Ordering::SeqCst);
        }
        System.dealloc(p, l)
    }
}

#[global_allocator]
static A: Counting = Counting;

fn tracked(f: impl FnOnce()) -> usize {
    ALLOCS.store(0, Ordering::SeqCst);
    TRACK.with(|t| t.set(true));
    f();
    TRACK.with(|t| t.set(false));
    ALLOCS.load(Ordering::SeqCst)
}

#[test]
fn full_project_never_allocates_on_the_audio_thread() {
    let mut s = Session::new();
    let mut p = s.attach(48_000);
    for kind in INSTRUMENT_KINDS {
        let id = s.add_track(kind).unwrap();
        for e in EFFECT_KINDS {
            s.add_effect(id, e).unwrap();
        }
        s.set_clip(
            id,
            if kind == InstrumentKind::Drums {
                demo_drums(4)
            } else {
                demo_clip()
            },
        );
    }
    s.set_loop(true, 2);
    s.set_count_in(1);
    let mut buf = vec![0.0f32; 2 * 256];
    let mut six = vec![0.0f32; 6 * 256];

    s.record();
    for n in 0..16u8 {
        s.midi(MidiEvent::NoteOn {
            channel: 0,
            note: 40 + n * 3,
            velocity: 110,
        });
    }
    s.midi(MidiEvent::ControlChange {
        channel: 0,
        controller: 64,
        value: 127,
    });
    s.midi(MidiEvent::PitchBend {
        channel: 0,
        value: 4000,
    });
    let n = tracked(|| {
        for _ in 0..600 {
            p.process_stereo(&mut buf);
        }
        p.process_device(&mut six, 6);
    });
    assert_eq!(n, 0, "audio thread allocated during record/loop playback");
}

#[test]
fn live_edits_never_allocate_on_the_audio_thread() {
    let mut s = Session::new();
    let mut p = s.attach(48_000);
    let a = s.add_track(InstrumentKind::Subtractive).unwrap();
    s.set_clip(a, demo_clip());
    s.set_tempo(300.0);
    s.play();
    let mut buf = vec![0.0f32; 2 * 512];

    // Every edit below allocates on this (control) thread, then the callback consumes it.
    let b = s.add_track(InstrumentKind::Fm).unwrap();
    s.add_effect(a, EFFECT_KINDS[3]).unwrap();
    s.add_effect(a, EFFECT_KINDS[4]).unwrap();
    s.set_instrument(a, InstrumentKind::Drums);
    s.set_clip(a, demo_drums(2)); // swap while playing: old box must be handed back, not freed
    s.remove_effect(a, 0);
    s.move_effect(a, 0, 0);
    s.remove_track(b);
    s.apply_preset(a, "Kit 808").unwrap();
    let n = tracked(|| {
        for _ in 0..300 {
            p.process_stereo(&mut buf);
        }
    });
    assert_eq!(n, 0, "audio thread allocated or freed while applying live edits");
    s.poll(); // retired objects are dropped here, on the control thread
}

#[test]
fn detector_catches_a_real_allocation() {
    let n = tracked(|| {
        let v = std::hint::black_box(vec![1u8; 16]);
        drop(v);
    });
    assert!(n >= 1, "the counting allocator must observe allocations");
}
