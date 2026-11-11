//! The current source of the schematic (`Kind::ISource`): its direction, its waveforms, and the ERC that stops a circuit it cannot
//! drive.

use crate::{dc_op, from_schematic, transient, TranSettings};
use sch_core::{Kind, Label, Schematic};

/// Puts a label of `net` on pin `pin` of the part at `idx`.
fn tie(s: &mut Schematic, idx: usize, pin: &str, net: &str) {
    let pt = s.components[idx].pins().into_iter().find(|(n, _)| n == pin).unwrap().1;
    s.labels.push(Label { pos: pt, text: net.into() });
}

/// A current source into a resistor to ground: I1 + → OUT, R1 from OUT to GND.
fn into_resistor(value: &str) -> Schematic {
    let mut s = Schematic::default();
    let i = s.add(Kind::ISource, (10, 10), 0);
    s.components[i].value = value.into();
    let r = s.add(Kind::Resistor, (20, 10), 0);
    s.components[r].value = "1k".into();
    let g = s.add(Kind::Ground, (30, 20), 0);
    tie(&mut s, i, "+", "OUT");
    tie(&mut s, i, "-", "GND");
    tie(&mut s, r, "1", "OUT");
    tie(&mut s, r, "2", "GND");
    tie(&mut s, g, "1", "GND");
    s
}

#[test]
fn a_positive_current_leaves_by_the_plus_pin() {
    // 10 mA pushed out of + into 1 kΩ: +10 V across it
    let c = from_schematic(&into_resistor("10m")).unwrap();
    let r = dc_op(&c).unwrap();
    assert!((r.voltage("OUT").unwrap() - 10.0).abs() < 1e-6, "{:?}", r.voltage("OUT"));
    // and the SPICE netlist says the same to ngspice (SPICE's current leaves by its second node)
    let net = sch_core::to_spice(&into_resistor("10m"), "t");
    assert!(net.lines().any(|l| l.starts_with("I1 ") && l.contains("DC 10m") && l.split_whitespace().nth(2).is_some_and(|n| n.eq_ignore_ascii_case("OUT"))), "{net}");
}

#[test]
fn a_pulse_current_steps_the_voltage() {
    let c = from_schematic(&into_resistor("PULSE(1m 5m 1m 1u 1u 2m 4m)")).unwrap();
    let r = transient(&c, &TranSettings { tstop: 2.5e-3, tstep: 10e-6, uic: false }).unwrap();
    let v = &r.signal("V(OUT)").unwrap().values;
    let at = |t: f64| v[r.time.iter().position(|x| *x >= t).unwrap()];
    assert!((at(0.5e-3) - 1.0).abs() < 1e-6 && (at(2.0e-3) - 5.0).abs() < 1e-6, "{} {}", at(0.5e-3), at(2.0e-3));
}

#[test]
fn the_erc_stops_a_current_source_with_nowhere_to_go() {
    // open: + is on a net of its own
    let mut s = Schematic::default();
    let i = s.add(Kind::ISource, (10, 10), 0);
    let g = s.add(Kind::Ground, (30, 20), 0);
    tie(&mut s, i, "+", "LONELY");
    tie(&mut s, i, "-", "GND");
    tie(&mut s, g, "1", "GND");
    let p = sch_core::erc(&s);
    assert!(p.iter().any(|x| x.message.contains("電流の行き先がありません")), "{:?}", p.iter().map(|x| &x.message).collect::<Vec<_>>());
    // two in series: the net between them has only the two sources
    let j = s.add(Kind::ISource, (20, 10), 0);
    tie(&mut s, j, "-", "LONELY");
    tie(&mut s, j, "+", "GND");
    let p = sch_core::erc(&s);
    assert!(p.iter().any(|x| x.message.contains("直列")), "{:?}", p.iter().map(|x| &x.message).collect::<Vec<_>>());
    // and a well-tied one has neither
    let p = sch_core::erc(&into_resistor("1m"));
    assert!(!p.iter().any(|x| x.message.contains("電流源")), "{:?}", p.iter().map(|x| &x.message).collect::<Vec<_>>());
}

#[test]
fn a_placed_current_source_stays_off_the_board() {
    let s = into_resistor("1m");
    let c = s.components.iter().find(|c| c.kind == Kind::ISource).unwrap();
    assert!(c.sim_only && c.footprint.is_empty());
    assert_eq!(c.refdes, "I1");
}

#[test]
fn the_load_step_sample_dips_by_the_zeners_resistance() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/load_step.schjson");
    let s: Schematic = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let c = from_schematic(&s).unwrap();
    let r = transient(&c, &TranSettings { tstop: 2.9e-3, tstep: 2e-6, uic: false }).unwrap();
    let v = &r.signal("V(VOUT)").unwrap().values;
    let m = crate::load_step(&r.time, v, 0.0, 2.9e-3).unwrap();
    // 10 mA more through ~7.7 Ω (the zener's slope in parallel with 150 Ω): about 77 mV, reached through C1
    assert!((m.excursion() + 0.0775).abs() < 0.005, "{}", m.excursion());
    assert!((m.after - m.extreme).abs() < 0.002, "a zener and a capacitor do not overshoot");
    assert!(m.recovery.is_some_and(|t| t > 50e-6 && t < 500e-6), "{:?}", m.recovery);
}

/// The motor sample (`DC_MOTOR` driven by a MOSFET from 5 ms; 1 mN·m of load from a current source on its speed pin from 50 ms).
fn motor_sample() -> (Schematic, crate::Circuit) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/motor_driver.schjson");
    let s: Schematic = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let c = from_schematic(&s).unwrap();
    (s, c)
}

#[test]
fn the_motor_turns_and_slows_under_its_load() {
    let (_, c) = motor_sample();
    let r = transient(&c, &TranSettings { tstop: 0.1, tstep: 50e-6, uic: false }).unwrap();
    let spd = &r.signal("V(SPD)").unwrap().values;
    let i = &r.signal("I(V1)").unwrap().values;
    // off before the gate turns on at 5 ms; about 8.8 krpm (1 V = 1000 rpm) unloaded by 45 ms
    let at = |t: f64| spd[r.time.iter().position(|x| *x >= t).unwrap()];
    assert!(at(4e-3).abs() < 0.01);
    assert!((at(48e-3) - 8.8).abs() < 0.2, "{}", at(48e-3));
    // 1 mN·m of load from 50 ms (a current source of 1 mA on the speed pin): slower, and more current
    let end = *spd.last().unwrap();
    assert!((end - 7.34).abs() < 0.15, "{end}");
    // the start draws far more than the run: the winding sees nearly the stall current until the back EMF builds
    let peak = i.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    assert!(peak > 1.3 && (i.last().unwrap().abs() - 0.456).abs() < 0.03, "peak {peak}, end {}", i.last().unwrap());
}

#[test]
fn a_locked_shaft_burns_the_winding_open_in_the_chain() {
    let (s, c) = motor_sample();
    assert!(crate::breakable(&c).iter().any(|n| n == "M1.LW") && crate::breakable(&c).iter().any(|n| n == "M1.CJ"));
    assert_eq!(crate::describe_fault(&c, "M1.CJ", crate::Fault::Short), "モーターの軸がロック (回らない)");
    let rep = crate::chain_failure(&s, &c, "M1.CJ", crate::Fault::Short, &TranSettings { tstop: 0.1, tstep: 50e-6, uic: false }).unwrap();
    let parts: Vec<String> = rep.steps.iter().map(|x| format!("{} {}", x.part, x.what)).collect();
    // about 2 A through 1.5 Ω: 6 W in the winding, well past its 1.5 W
    assert!(parts.iter().any(|p| p.starts_with("M1 ") && p.contains("巻線")), "{parts:?}");
    // a healthy motor under its load is inside its rating: nothing burns
    let ok = crate::check_ratings(&s, &crate::dc_op(&c).unwrap());
    assert!(ok.iter().all(|x| x.refdes != "M1"), "{:?}", ok.iter().map(|x| &x.message).collect::<Vec<_>>());
}

#[test]
fn a_layer_short_in_the_motor_winding_lowers_its_resistance() {
    let (_, c) = motor_sample();
    let b = crate::with_fault(&c, "M1.LW", crate::Fault::TurnShort(0.5)).unwrap();
    let r = |c: &crate::Circuit| c.elements.iter().find_map(|e| match e { crate::Element::Resistor { name, r, .. } if name == "M1.RW" => Some(*r), _ => None }).unwrap();
    assert!((r(&b) - 0.75).abs() < 1e-9 && (r(&c) - 1.5).abs() < 1e-9);
}
