//! The potentiometer of the schematic (`Kind::Pot`): the knob divides the track, the SPICE export says the same, and the rating
//! check finds a wiper that carries more current than the track takes.

use crate::{check_ratings, dc_op, from_schematic, RatingLevel};
use sch_core::{Kind, Label, Schematic};

fn tie(s: &mut Schematic, idx: usize, pin: &str, net: &str) {
    let pt = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap().1;
    s.labels.push(Label { pos: pt, text: net.into() });
}

/// 5 V across a 10k pot (pin 1 on VCC, pin 3 on GND); the wiper `W` loaded by `load` ohms to ground (none: open).
fn divider(value: &str, load: Option<&str>) -> Schematic {
    let mut s = Schematic::default();
    let v = s.add(Kind::VSource, (0, 10), 0);
    let p = s.add(Kind::Pot, (20, 10), 0);
    s.components[p].value = value.into();
    let g = s.add(Kind::Ground, (40, 30), 0);
    tie(&mut s, v, "+", "VCC");
    tie(&mut s, v, "-", "GND");
    tie(&mut s, p, "1", "VCC");
    tie(&mut s, p, "2", "W");
    tie(&mut s, p, "3", "GND");
    tie(&mut s, g, "1", "GND");
    if let Some(r) = load {
        let l = s.add(Kind::Resistor, (60, 10), 0);
        s.components[l].value = r.into();
        tie(&mut s, l, "1", "W");
        tie(&mut s, l, "2", "GND");
    }
    s
}

#[test]
fn the_knob_sets_the_wiper_between_the_ends() {
    // POS is the share of the track from pin 1: the wiper sits that far down from VCC
    for (pos, want) in [(0.0, 5.0), (30.0, 3.5), (50.0, 2.5), (100.0, 0.0)] {
        let s = divider(&format!("10k POS={pos}"), None);
        let dc = dc_op(&from_schematic(&s).unwrap()).unwrap();
        let w = dc.voltage("W").unwrap();
        // the end resistance (0.5 % of the track) keeps the wiper a hair off the rail at either end
        assert!((w - want).abs() < 0.03, "POS={pos}: {w}");
    }
    // no POS: the middle
    assert!((sch_core::pot_position("10k") - 0.5).abs() < 1e-12);
    assert_eq!(sch_core::with_pot_position("10k POS=20", 0.734), "10k POS=73");
    assert_eq!(sch_core::with_pot_position("", 1.0), "10k POS=100");
}

#[test]
fn the_spice_export_splits_the_track_the_same_way() {
    let net = sch_core::to_spice(&divider("10k POS=30", None), "pot");
    assert!(net.contains("RVR1_A VCC W 3e3"), "{net}");
    assert!(net.contains("RVR1_B W 0 7e3") || net.contains("RVR1_B W GND 7e3"), "{net}");
}

#[test]
fn a_wiper_that_feeds_a_load_heats_the_piece_of_track_it_runs_through() {
    // a 1k load on the wiper of a 10k pot: the wiper's current runs through the upper part of the track. At 10 % the wiper sits
    // at 5 × 900 / 1900 = 2.37 V and 2.6 mA runs through the upper 1k, past what a 0.05 W track of 10k takes (√(0.05 / 10k) =
    // 2.24 mA)
    let issue = |value: &str, load: Option<&str>| {
        let s = divider(value, load);
        let dc = dc_op(&from_schematic(&s).unwrap()).unwrap();
        check_ratings(&s, &dc).into_iter().find(|i| i.refdes == "VR1").map(|i| (i.level, i.message))
    };
    let (lv, msg) = issue("10k POS=10", Some("1k")).expect("the track is past its current");
    assert_eq!(lv, RatingLevel::Over, "{msg}");
    assert!(msg.contains("2.24 mA"), "{msg}");
    // as a plain divider (nothing on the wiper) 0.5 mA runs end to end: well inside
    assert_eq!(issue("10k POS=30", None), None);
    // turned all the way to pin 1, the wiper sits on the end: the load's 5 mA does not run through the track
    assert_eq!(issue("10k POS=0", Some("1k")), None);
}
