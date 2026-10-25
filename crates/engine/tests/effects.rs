mod common;
use beatmaker_engine::effects::{EffectKind, EFFECT_KINDS};
use beatmaker_engine::instrument::InstrumentKind;
use beatmaker_engine::session::Session;
use common::*;

/// One short sine note (A4, 50 ms) through an optional effect; returns the left channel.
fn blip(configure: impl FnOnce(&mut Session, u32), seconds: f32) -> Vec<f32> {
    let (mut s, mut p, id) = setup();
    pure_sine(&mut s, id);
    set(&mut s, id, "amp_release", 0.005);
    set(&mut s, id, "amp_attack", 0.001);
    configure(&mut s, id);
    s.midi(on(69, 127));
    let mut x = run(&mut p, secs(0.05), 128);
    s.midi(off(69));
    x.extend(run(&mut p, secs(seconds - 0.05), 128));
    x
}

#[test]
fn delay_produces_a_late_echo() {
    let dry = blip(|_, _| {}, 0.6);
    let wet = blip(
        |s, id| {
            let slot = s.add_effect(id, EffectKind::Delay).unwrap();
            set_fx(s, id, slot, "time", 200.0);
            set_fx(s, id, slot, "feedback", 0.0);
            set_fx(s, id, slot, "mix", 1.0);
            set_fx(s, id, slot, "pingpong", 0.0);
        },
        0.6,
    );
    // 50 ms note delayed by 200 ms: the echo occupies 0.20..0.25 s.
    let window = secs(0.21)..secs(0.24);
    assert!(rms(&dry[window.clone()]) < 1e-4);
    assert!(
        rms(&wet[window]) > 0.03,
        "echo missing: {}",
        rms(&wet[secs(0.21)..secs(0.24)])
    );
}

#[test]
fn delay_feedback_repeats_decay() {
    let wet = blip(
        |s, id| {
            let slot = s.add_effect(id, EffectKind::Delay).unwrap();
            set_fx(s, id, slot, "time", 100.0);
            set_fx(s, id, slot, "feedback", 0.5);
            set_fx(s, id, slot, "mix", 1.0);
            set_fx(s, id, slot, "tone", 16000.0);
            set_fx(s, id, slot, "pingpong", 0.0);
        },
        1.0,
    );
    let e = |k: usize| rms(&wet[secs(0.02) + k * 4800..secs(0.07) + k * 4800]);
    assert!(e(1) > e(2) && e(2) > e(3) && e(3) > 0.0, "{} {} {}", e(1), e(2), e(3));
}

#[test]
fn reverb_adds_a_decaying_tail() {
    let dry = blip(|_, _| {}, 2.0);
    let wet = blip(
        |s, id| {
            let slot = s.add_effect(id, EffectKind::Reverb).unwrap();
            set_fx(s, id, slot, "mix", 0.6);
            set_fx(s, id, slot, "size", 0.8);
        },
        2.0,
    );
    assert!(rms(&dry[secs(0.4)..secs(0.5)]) < 1e-4);
    let early = rms(&wet[secs(0.3)..secs(0.4)]);
    let late = rms(&wet[secs(1.8)..secs(1.9)]);
    assert!(early > 0.002, "no tail: {early}");
    assert!(late < early, "tail must decay: {early} -> {late}");
    assert!(wet.iter().all(|v| v.is_finite()));
}

#[test]
fn compressor_reduces_loud_signals() {
    let loud = |configure: bool| {
        let (mut s, mut p, id) = setup();
        pure_sine(&mut s, id);
        set(&mut s, id, "amp_sustain", 1.0);
        if configure {
            let slot = s.add_effect(id, EffectKind::Compressor).unwrap();
            set_fx(&mut s, id, slot, "threshold", -30.0);
            set_fx(&mut s, id, slot, "ratio", 20.0);
            set_fx(&mut s, id, slot, "makeup", 0.0);
            set_fx(&mut s, id, slot, "attack", 0.5);
        }
        s.midi(on(69, 127));
        run(&mut p, secs(0.5), 256)[secs(0.2)..].to_vec()
    };
    let (plain, comp) = (rms(&loud(false)), rms(&loud(true)));
    assert!(comp < plain * 0.5, "plain {plain} compressed {comp}");
}

#[test]
fn filter_lowpass_removes_highs_and_highpass_removes_lows() {
    let play = |ty: f32, note: u8| {
        let (mut s, mut p, id) = setup();
        pure_sine(&mut s, id);
        set(&mut s, id, "amp_sustain", 1.0);
        let slot = s.add_effect(id, EffectKind::Filter).unwrap();
        set_fx(&mut s, id, slot, "type", ty);
        set_fx(&mut s, id, slot, "cutoff", 1000.0);
        set_fx(&mut s, id, slot, "resonance", 0.0);
        s.midi(on(note, 127));
        rms(&run(&mut p, secs(0.4), 256)[secs(0.2)..])
    };
    let reference = {
        let (mut s, mut p, id) = setup();
        pure_sine(&mut s, id);
        set(&mut s, id, "amp_sustain", 1.0);
        s.midi(on(69, 127));
        rms(&run(&mut p, secs(0.4), 256)[secs(0.2)..])
    };
    assert!(play(0.0, 117) < reference * 0.05, "LP should cut 7 kHz");
    assert!(play(0.0, 57) > reference * 0.7, "LP should pass 220 Hz");
    assert!(play(1.0, 45) < reference * 0.1, "HP should cut 110 Hz");
    assert!(play(1.0, 105) > reference * 0.7, "HP should pass 3.5 kHz");
}

#[test]
fn drive_and_chorus_stay_finite_and_change_the_sound() {
    for kind in [EffectKind::Drive, EffectKind::Chorus] {
        let plain = blip(|_, _| {}, 0.3);
        let fx = blip(
            |s, id| {
                let slot = s.add_effect(id, kind).unwrap();
                if kind == EffectKind::Drive {
                    set_fx(s, id, slot, "drive", 1.0);
                }
            },
            0.3,
        );
        assert!(fx.iter().all(|v| v.is_finite() && v.abs() <= 1.0), "{kind:?}");
        assert!(
            plain.iter().zip(&fx).any(|(a, b)| (a - b).abs() > 0.01),
            "{kind:?} did nothing"
        );
    }
}

#[test]
fn bypass_is_bit_exact_and_remove_restores_the_dry_signal() {
    let dry = blip(|_, _| {}, 0.3);
    let bypassed = blip(
        |s, id| {
            for k in EFFECT_KINDS {
                let slot = s.add_effect(id, k).unwrap();
                s.set_effect_bypass(id, slot, true);
            }
        },
        0.3,
    );
    assert_eq!(dry, bypassed);
    let removed = blip(
        |s, id| {
            s.add_effect(id, EffectKind::Reverb).unwrap();
            s.add_effect(id, EffectKind::Delay).unwrap();
            s.remove_effect(id, 0);
            s.remove_effect(id, 0);
        },
        0.3,
    );
    assert_eq!(dry, removed);
}

#[test]
fn effect_order_matters_and_can_be_changed() {
    let mut s = Session::new();
    let id = s.add_track(InstrumentKind::Subtractive).unwrap();
    s.add_effect(id, EffectKind::Drive).unwrap();
    s.add_effect(id, EffectKind::Delay).unwrap();
    s.add_effect(id, EffectKind::Reverb).unwrap();
    s.move_effect(id, 2, 0);
    let order: Vec<_> = s.project().track(id).unwrap().effects.iter().map(|e| e.kind).collect();
    assert_eq!(order, vec![EffectKind::Reverb, EffectKind::Drive, EffectKind::Delay]);
    s.move_effect(id, 0, 2);
    let order: Vec<_> = s.project().track(id).unwrap().effects.iter().map(|e| e.kind).collect();
    assert_eq!(order, vec![EffectKind::Drive, EffectKind::Delay, EffectKind::Reverb]);
}

#[test]
fn effect_defaults_are_valid_and_parameters_clamp() {
    for k in EFFECT_KINDS {
        let infos = k.params();
        assert!(!infos.is_empty());
        for i in infos {
            assert!(i.default >= i.min && i.default <= i.max, "{k:?}/{}", i.key);
        }
    }
    let mut s = Session::new();
    let id = s.add_track(InstrumentKind::Subtractive).unwrap();
    let slot = s.add_effect(id, EffectKind::Delay).unwrap();
    s.set_effect_param(id, slot, 1, 99.0);
    assert_eq!(s.project().track(id).unwrap().effects[slot].params[1], 0.95);
}
