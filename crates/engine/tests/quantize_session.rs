mod common;
use beatmaker_engine::instrument::InstrumentKind;
use beatmaker_engine::quantize::{QuantizeSettings, DIVISIONS};
use beatmaker_engine::session::Session;
use common::*;

#[test]
fn quantize_button_snaps_the_track_and_is_undoable() {
    let mut s = Session::new();
    let id = s.add_track(InstrumentKind::Subtractive).unwrap();
    let other = s.add_track(InstrumentKind::Fm).unwrap();
    let original = clip_of(&[(250, 450, 60), (980, 1200, 64), (1900, 2000, 67)], 3840);
    s.set_clip(id, original.clone());
    s.set_clip(other, original.clone());
    s.set_quantize(
        id,
        QuantizeSettings {
            division: 8,
            ..Default::default()
        },
    );
    s.quantize_track(id).unwrap();
    let starts: Vec<u64> = s.notes(id).iter().map(|n| n.start_tick).collect();
    assert_eq!(starts, vec![480, 960, 1920]);
    assert_eq!(s.clip(other).unwrap(), &original, "other tracks are untouched");
    assert!(s.undo());
    assert_eq!(s.clip(id).unwrap(), &original);
}

#[test]
fn every_division_up_to_1_128_works() {
    assert_eq!(*DIVISIONS.last().unwrap(), 128);
    let mut s = Session::new();
    let id = s.add_track(InstrumentKind::Subtractive).unwrap();
    for d in DIVISIONS {
        let c = clip_of(&[(1234, 1500, 60)], 3840);
        s.set_clip(id, c);
        s.set_quantize(
            id,
            QuantizeSettings {
                division: d,
                ..Default::default()
            },
        );
        s.quantize_track(id).unwrap();
        let step = 3840 / d as u64;
        let start = s.notes(id)[0].start_tick;
        assert_eq!(start % step, 0, "1/{d}: {start} is off the {step}-tick grid");
        assert!((start as i64 - 1234).unsigned_abs() <= step / 2 + 1);
    }
}

#[test]
fn quantizing_an_empty_track_is_an_error_and_settings_clamp() {
    let mut s = Session::new();
    let id = s.add_track(InstrumentKind::Subtractive).unwrap();
    assert!(s.quantize_track(id).is_err());
    s.set_quantize(
        id,
        QuantizeSettings {
            division: 999,
            triplet: true,
            strength: 7.0,
            swing: 0.1,
        },
    );
    let q = s.project().track(id).unwrap().quantize;
    assert_eq!((q.division, q.strength, q.swing, q.triplet), (128, 1.0, 0.5, true));
}

#[test]
fn undo_history_is_unlimited() {
    let mut s = Session::new();
    let id = s.add_track(InstrumentKind::Subtractive).unwrap();
    for i in 0..500u64 {
        s.set_clip(id, clip_of(&[(i, i + 10, 60)], 3840));
    }
    let mut n = 0;
    while s.undo() {
        n += 1;
    }
    assert_eq!(n, 500);
    assert!(s.clip(id).unwrap().is_empty());
    for _ in 0..500 {
        assert!(s.redo());
    }
    assert_eq!(s.notes(id)[0].start_tick, 499);
}
