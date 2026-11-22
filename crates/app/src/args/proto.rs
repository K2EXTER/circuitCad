//! The hands-on tabs: `--bench ID` (a learning item's checklist), `--proto [R1]` (the prototype tab), `--bench-tab [step]` and
//! `--power-on [mA]`.

use super::*;
use crate::breadboard::{BoardKind, MeterRange, StripTool, Tool};
use sch_core::breadboard::{Hole, Rail};

pub(super) fn apply(app: &mut App, a: &Args) {
    // `--bench ID`: the hands-on checklist of a learning-path item (`bench_guide.rs`: solder, bootload, solder_tqfp100)
    if let Some(id) = a.val("--bench") {
        app.bench.show_guide(id);
    }
    if a.has("--proto") {
        proto_tab(app, a);
    }
    // `--bench-tab [assemble|power_on|measure|judge|repair]`: the bench tab ("実機"), at that step
    if a.has("--bench-tab") {
        app.mode = Mode::Bench;
        if let Some(s) = a.val("--bench-tab").and_then(crate::bench_tab::Step::from_key) {
            app.bench_tab.step = s;
        }
    }
    // `--power-on [mA]`: the first power-on guide for the opened schematic, with a reading typed in
    if a.has("--power-on") {
        app.power_on.open_for(&app.sch);
        if let Some(ma) = a.val("--power-on").filter(|v| v.parse::<f64>().is_ok()) {
            app.power_on.set_measured_main(ma);
        }
    }
}

/// `--proto [R1]`: the prototype tab ("試作", the breadboard), with that part picked.
fn proto_tab(app: &mut App, a: &Args) {
    app.mode = Mode::Proto;
    let bb = &mut app.breadboard;
    bb.picked = a.after("--proto").map(String::from);
    // `--proto-power`: the supply switched on
    bb.powered = a.has("--proto-power");
    // `--proto-kind strip|perf`: the stripboard or perfboard view (B5)
    match a.val("--proto-kind") {
        Some("strip") => bb.kind = BoardKind::Stripboard,
        Some("perf") => bb.kind = BoardKind::Perfboard,
        _ => {}
    }
    // `--proto-plan`: the sections proposed ("区画の案を出す") shown in the panel
    if a.has("--proto-plan") {
        app.breadboard.strip_propose(&app.sch);
    }
    let bb = &mut app.breadboard;
    // `--proto-build 2`: "組む順と通電" open with step 2 (from 1) picked
    if let Some(k) = a.num::<usize>("--proto-build") {
        bb.build.open = true;
        bb.build.step = k.checked_sub(1);
    }
    // `--proto-chips-back`: a perfboard's 0805 / 1206 chips on the copper side
    app.sch.strip_chips_back = a.has("--proto-chips-back");
    // `--proto-sel A1:Z30`: a box picked on the stripboard or perfboard, by the holes at two corners (what the box offers: "揃える")
    if let Some((from, to)) = a.val("--proto-sel").and_then(|s| s.split_once(':')) {
        bb.strip_pick_box(from, to);
    }
    // `--proto-back`: the stripboard or perfboard seen from below (the copper side)
    bb.strip.back = a.has("--proto-back");
    // `--proto-schematic`: the schematic in its small window beside the board
    bb.show_schematic = a.has("--proto-schematic");
    // `--proto-size 100x75`: the stripboard's size in mm (the chips wrap to its width)
    if let Some((w, h)) = a.val("--proto-size").and_then(|s| s.split_once('x')).and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))) {
        app.sch.strip_size = Some((w, h));
    }
    // `--proto-flip SW1,SW2`: those switches moved (push buttons held) once the circuit runs
    if let Some(f) = a.val("--proto-flip") {
        bb.flip_on_start = f.split(',').filter(|r| !r.is_empty()).map(String::from).collect();
    }
    // `--proto-meter`: the meter's probes on the top rails (on a stripboard or perfboard: on the supply's two wires once the board
    // is laid out); `--proto-meter-range amps|ohms`: the meter's range (B6 ③)
    if a.has("--proto-meter") {
        bb.strip.tool = StripTool::Meter;
        bb.strip.probe_supply = true;
        bb.tool = Tool::Meter;
        bb.red = Some(Hole::Rail { rail: Rail::TopPlus, col: 12 });
        bb.black = Some(Hole::Rail { rail: Rail::TopMinus, col: 12 });
        match a.val("--proto-meter-range") {
            Some("amps") => bb.meter_range = MeterRange::Amps,
            Some("ohms") => bb.meter_range = MeterRange::Ohms,
            _ => {}
        }
    }
}
