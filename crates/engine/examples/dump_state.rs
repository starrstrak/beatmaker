//! Prints the engine catalogue and the starter project as JSON (used to test the UI headlessly).
use beatmaker_engine::effects::EFFECT_KINDS;
use beatmaker_engine::instrument::INSTRUMENT_KINDS;
use beatmaker_engine::quantize::DIVISIONS;
use beatmaker_engine::render::{demo_clip, demo_drums};
use beatmaker_engine::session::Session;
use serde_json::json;

fn main() {
    let instruments: Vec<_> = INSTRUMENT_KINDS
        .iter()
        .map(|k| json!({ "key": k.key(), "params": k.params(), "presets": k.presets().iter().map(|p| p.name).collect::<Vec<_>>() }))
        .collect();
    let effects: Vec<_> = EFFECT_KINDS
        .iter()
        .map(|k| json!({ "key": k.key(), "params": k.params() }))
        .collect();
    let mut s = Session::with_starter_project();
    let ids: Vec<u32> = s.project().tracks.iter().map(|t| t.id).collect();
    s.set_clip(ids[0], demo_clip());
    s.set_clip(ids[2], demo_drums(4));
    println!(
        "{}",
        json!({ "catalog": { "instruments": instruments, "effects": effects, "divisions": DIVISIONS }, "tracks": s.project_view() })
    );
}
