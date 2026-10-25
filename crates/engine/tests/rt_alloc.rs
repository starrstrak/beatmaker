//! Proves the audio callback never touches the heap: a counting allocator fails the
//! test if `process_*` allocates while notes play, the transport records and a clip loads.

use beatmaker_engine::midi::MidiEvent;
use beatmaker_engine::render::demo_clip;
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

#[test]
fn process_never_allocates() {
    let mut s = Session::new();
    let mut p = s.attach(48_000);
    s.set_clip(demo_clip());
    s.set_count_in(1);
    let mut buf = vec![0.0f32; 2 * 256];
    let mut six = vec![0.0f32; 6 * 256];

    // Everything that allocates (queue pushes of boxed clips, etc.) happens before tracking.
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

    TRACK.with(|t| t.set(true));
    for _ in 0..400 {
        p.process_stereo(&mut buf);
    }
    p.process_device(&mut six, 6);
    TRACK.with(|t| t.set(false));

    assert_eq!(ALLOCS.load(Ordering::SeqCst), 0, "audio thread allocated");
}

#[test]
fn playback_and_clip_swap_never_allocate_on_audio_thread() {
    let mut s = Session::new();
    let mut p = s.attach(48_000);
    s.set_clip(demo_clip());
    s.set_tempo(300.0);
    s.play();
    s.set_clip(demo_clip()); // swap while playing: old box must be handed back, not freed
    let mut buf = vec![0.0f32; 2 * 512];
    TRACK.with(|t| t.set(true));
    for _ in 0..300 {
        p.process_stereo(&mut buf);
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.load(Ordering::SeqCst), 0, "audio thread allocated or freed");
}
