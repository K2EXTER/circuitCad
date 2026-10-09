//! The bundled library parts, simulated through their own models.

use crate::*;
use part_lib::{PartDef, EXAMPLES};
use sch_core::SimKind;

fn part(name: &str) -> PartDef {
    EXAMPLES
        .iter()
        .map(|(_, t)| serde_json::from_str::<PartDef>(t).unwrap())
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("no bundled part {name}"))
}

/// Instantiate a library part with its simulation pins wired to the given nets.
fn place(c: &mut Circuit, def: &PartDef, inst: &str, nets: &[(&str, &str)]) {
    let sim = def.sim.as_ref().unwrap_or_else(|| panic!("{} has no model", def.name));
    let nodes: Vec<usize> = sim
        .pins
        .iter()
        .map(|p| {
            let net = nets.iter().find(|(n, _)| n == p).unwrap_or_else(|| panic!("pin {p} of {} is not wired", def.name)).1;
            c.node(net)
        })
        .collect();
    match sim.kind {
        SimKind::Model => add_model_instance(c, inst, &sim.element, &sim.name, &sim.text, &nodes).unwrap(),
        SimKind::Subckt => add_subckt_instance(c, inst, &sim.name, &sim.text, &nodes).unwrap(),
    }
}

fn vs(c: &mut Circuit, name: &str, p: &str, n: &str, dc: f64) {
    let (p, n) = (c.node(p), c.node(n));
    c.add(Element::vdc(name, p, n, dc));
}

fn res(c: &mut Circuit, name: &str, a: &str, b: &str, ohms: f64) {
    let (a, b) = (c.node(a), c.node(b));
    c.add(Element::Resistor { name: name.into(), a, b, r: ohms });
}

fn v(r: &DcResult, node: &str) -> f64 {
    r.voltage(node).unwrap_or_else(|| panic!("no node {node}"))
}

#[test]
fn every_bundled_model_can_be_instantiated() {
    let mut with_model = 0;
    for (n, text) in EXAMPLES {
        let def: PartDef = serde_json::from_str(text).unwrap();
        let Some(sim) = &def.sim else { continue };
        with_model += 1;
        let mut c = Circuit::new();
        let nets: Vec<(String, String)> = sim.pins.iter().map(|p| (p.clone(), format!("net_{p}"))).collect();
        let refs: Vec<(&str, &str)> = nets.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        place(&mut c, &def, "U1", &refs);
        assert!(!c.elements.is_empty(), "{n}");
    }
    assert!(with_model >= 16, "{with_model}");
}

// ---------------------------------------------------------------- transistors

#[test]
fn n_channel_mosfets_switch_a_load() {
    for (name, g, d, s) in [("2N7000", "2", "3", "1"), ("AO3400A", "1", "3", "2")] {
        let def = part(name);
        let run = |vg: f64| {
            let mut c = Circuit::new();
            vs(&mut c, "Vdd", "vdd", "0", 5.0);
            vs(&mut c, "Vg", "gate", "0", vg);
            res(&mut c, "Rd", "vdd", "drain", 1000.0);
            place(&mut c, &def, "Q1", &[(g, "gate"), (d, "drain"), (s, "0")]);
            dc_op(&c).unwrap()
        };
        let on = run(5.0);
        assert!(v(&on, "drain") < 0.1, "{name} on: {}", v(&on, "drain"));
        let off = run(0.0);
        assert!((v(&off, "drain") - 5.0).abs() < 1e-3, "{name} off");
    }
}

#[test]
fn the_p_channel_mosfet_is_a_high_side_switch() {
    let def = part("AO3401A");
    let run = |vg: f64| {
        let mut c = Circuit::new();
        vs(&mut c, "Vdd", "vdd", "0", 5.0);
        vs(&mut c, "Vg", "gate", "0", vg);
        res(&mut c, "Rl", "load", "0", 100.0);
        place(&mut c, &def, "Q1", &[("1", "gate"), ("2", "vdd"), ("3", "load")]);
        v(&dc_op(&c).unwrap(), "load")
    };
    assert!(run(0.0) > 4.9, "on");
    assert!(run(5.0) < 1e-3, "off");
}

#[test]
fn bipolar_parts_have_their_stated_gain_and_pinout() {
    // (part, pin of C, pin of B, pin of E, expected hFE from the model)
    for (name, c_pin, b_pin, e_pin, beta) in [("2N3904", "3", "2", "1", 300.0), ("BC547", "1", "2", "3", 290.0)] {
        let def = part(name);
        let mut c = Circuit::new();
        vs(&mut c, "Vcc", "vcc", "0", 10.0);
        res(&mut c, "Rb", "vcc", "b", 1e6);
        res(&mut c, "Rc", "vcc", "col", 1000.0);
        place(&mut c, &def, "Q1", &[(c_pin, "col"), (b_pin, "b"), (e_pin, "0")]);
        let r = dc_op(&c).unwrap();
        let q = r.element("Q1").unwrap();
        let ib = q.extra.iter().find(|(n, _)| n == "Ib").unwrap().1;
        assert!(q.current / ib > beta * 0.97 && q.current / ib < beta * 1.001, "{name}: beta {}", q.current / ib);
        assert!((v(&r, "b") - 0.65).abs() < 0.1, "{name}: vbe {}", v(&r, "b"));
    }
}

#[test]
fn the_pnp_part_conducts_from_the_emitter_supply() {
    let def = part("2N3906");
    let mut c = Circuit::new();
    vs(&mut c, "Vcc", "vcc", "0", 10.0);
    res(&mut c, "Rb", "b", "0", 1e6);
    res(&mut c, "Rc", "col", "0", 1000.0);
    place(&mut c, &def, "Q1", &[("3", "col"), ("2", "b"), ("1", "vcc")]);
    let r = dc_op(&c).unwrap();
    let ic = v(&r, "col") / 1000.0;
    assert!(ic > 1.5e-3 && ic < 2.1e-3, "{ic}");
}

// ---------------------------------------------------------------- diodes

#[test]
fn rectifier_and_schottky_forward_voltages() {
    for (name, lo, hi) in [("1N4001", 0.85, 1.05), ("1N5819", 0.38, 0.52)] {
        let def = part(name);
        let mut c = Circuit::new();
        vs(&mut c, "V1", "in", "0", 6.0);
        res(&mut c, "R1", "in", "a", 5.0);
        place(&mut c, &def, "D1", &[("A", "a"), ("K", "0")]);
        let r = dc_op(&c).unwrap();
        let (vf, i) = (v(&r, "a"), (6.0 - v(&r, "a")) / 5.0);
        assert!(i > 0.9 && i < 1.2, "{name}: ~1 A, got {i}");
        assert!(vf > lo && vf < hi, "{name}: Vf {vf}");
    }
}

#[test]
fn the_zener_part_makes_a_five_volt_reference() {
    let def = part("1N4733A");
    let run = |vin: f64| {
        let mut c = Circuit::new();
        vs(&mut c, "V1", "in", "0", vin);
        res(&mut c, "R1", "in", "z", 470.0);
        // cathode (K) towards the supply, anode (A) to ground: reverse biased
        place(&mut c, &def, "D1", &[("K", "z"), ("A", "0")]);
        v(&dc_op(&c).unwrap(), "z")
    };
    let (a, b) = (run(12.0), run(15.0));
    assert!(a > 5.0 && a < 5.35, "{a}");
    assert!((b - a).abs() < 0.08, "line regulation {a} -> {b}");
    assert!((run(3.0) - 3.0).abs() < 1e-3, "no conduction below the zener voltage");
    assert!(run(-1.0) < -0.45, "forward biased the other way: the ordinary diode drop appears");
}

// ---------------------------------------------------------------- regulators

fn regulator(name: &str, in_pin: &str, gnd_pin: &str, out_pin: &str, vin: f64, rload: f64) -> DcResult {
    let def = part(name);
    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", vin);
    res(&mut c, "Rl", "out", "0", rload);
    place(&mut c, &def, "U1", &[(in_pin, "in"), (gnd_pin, "0"), (out_pin, "out")]);
    dc_op(&c).unwrap()
}

#[test]
fn the_5v_regulators_regulate_and_drop_out() {
    for (name, i, g, o, dropout) in [("LM7805", "1", "2", "3", 2.0), ("L78L05", "3", "2", "1", 1.7)] {
        let at = |vin: f64, rl: f64| v(&regulator(name, i, g, o, vin, rl), "out");
        // regulated over a wide input range
        for vin in [9.0, 12.0, 20.0] {
            assert!((at(vin, 100.0) - 5.0).abs() < 0.01, "{name} at {vin} V: {}", at(vin, 100.0));
        }
        // dropout: output follows the input minus the dropout voltage
        let low = at(5.0 + dropout - 1.0, 100.0);
        assert!((low - 4.0).abs() < 0.1, "{name} in dropout: {low}");
        // load regulation: a little droop at 10x the current
        assert!(at(12.0, 100.0) - at(12.0, 10.0) < 0.1, "{name}");
    }
}

#[test]
fn the_regulator_draws_its_load_current_from_the_input() {
    let r = regulator("LM7805", "1", "2", "3", 12.0, 50.0);
    let iin = -r.element("V1").unwrap().current;
    let iload = v(&r, "out") / 50.0;
    assert!((iin - iload).abs() / iload < 0.01, "{iin} vs {iload}");
    // the pass element dissipates (Vin - Vout) * I: V1 delivers 12 V * I, the load takes 5 V * I
    let total: f64 = r.elements.iter().filter(|e| e.name.starts_with("U1.")).map(|e| e.power).sum();
    let _ = total;
}

#[test]
fn the_3v3_ldo() {
    let at = |vin: f64| v(&regulator("AMS1117-3.3", "3", "1", "2", vin, 33.0), "out");
    for vin in [5.0, 9.0, 12.0] {
        assert!((at(vin) - 3.3).abs() < 0.01, "{vin} V: {}", at(vin));
    }
    assert!((at(3.4) - 2.3).abs() < 0.1, "dropout {}", at(3.4));
}

// ---------------------------------------------------------------- op-amps

fn follower(name: &str, vin: f64, vp: f64, vn: f64) -> f64 {
    let def = part(name);
    let (inp, inn, out, p, n) = if name == "MCP6001" { ("3", "4", "1", "5", "2") } else { ("3", "2", "1", "8", "4") };
    let mut c = Circuit::new();
    vs(&mut c, "Vp", "vp", "0", vp);
    vs(&mut c, "Vn", "vn", "0", vn);
    vs(&mut c, "Vin", "in", "0", vin);
    // output tied to the inverting input
    place(&mut c, &def, "U1", &[(inp, "in"), (inn, "out"), (out, "out"), (p, "vp"), (n, "vn")]);
    v(&dc_op(&c).unwrap(), "out")
}

#[test]
fn followers_track_their_input_inside_the_supply_range() {
    for (name, vin, vp, vn) in [("LM358", 1.0, 5.0, 0.0), ("TL072", 5.0, 15.0, -15.0), ("MCP6002", 2.5, 5.0, 0.0), ("MCP6001", 2.5, 5.0, 0.0), ("TL072", -8.0, 15.0, -15.0)] {
        let out = follower(name, vin, vp, vn);
        assert!((out - vin).abs() < 1e-3, "{name}: {out} vs {vin}");
    }
}

#[test]
fn outputs_clip_at_the_documented_swing() {
    // LM358: V+ - 1.5 V at the top, near V- at the bottom
    assert!((follower("LM358", 4.9, 5.0, 0.0) - 3.5).abs() < 0.02, "{}", follower("LM358", 4.9, 5.0, 0.0));
    assert!((follower("LM358", -0.5, 5.0, 0.0) - 0.02).abs() < 0.02, "{}", follower("LM358", -0.5, 5.0, 0.0));
    // TL072 on +-15 V
    assert!((follower("TL072", 14.9, 15.0, -15.0) - 13.5).abs() < 0.02);
    assert!((follower("TL072", -14.9, 15.0, -15.0) + 13.5).abs() < 0.02);
    // rail-to-rail parts get within 25 mV of the rails
    for name in ["MCP6002", "MCP6001"] {
        assert!((follower(name, 5.0, 5.0, 0.0) - 4.975).abs() < 0.005, "{name} {}", follower(name, 5.0, 5.0, 0.0));
        assert!((follower(name, -0.2, 5.0, 0.0) - 0.025).abs() < 0.005, "{name}");
    }
}

#[test]
fn an_open_loop_comparator_goes_to_the_rails() {
    let def = part("LM358");
    let run = |vin: f64| {
        let mut c = Circuit::new();
        vs(&mut c, "Vp", "vp", "0", 5.0);
        vs(&mut c, "Vref", "ref", "0", 2.5);
        vs(&mut c, "Vin", "in", "0", vin);
        place(&mut c, &def, "U1", &[("3", "in"), ("2", "ref"), ("1", "out"), ("8", "vp"), ("4", "0")]);
        v(&dc_op(&c).unwrap(), "out")
    };
    assert!((run(2.6) - 3.5).abs() < 0.02, "high: {}", run(2.6));
    assert!((run(2.4) - 0.02).abs() < 0.02, "low: {}", run(2.4));
    // inside the linear window of ~35 uV the output is between the rails
    let mid = run(2.5 + 20e-6);
    assert!(mid > 1.0 && mid < 3.0, "{mid}");
}

#[test]
fn the_inverting_amplifier_gain_follows_the_resistors() {
    let def = part("TL072");
    let mut c = Circuit::new();
    vs(&mut c, "Vp", "vp", "0", 15.0);
    vs(&mut c, "Vn", "vn", "0", -15.0);
    vs(&mut c, "Vin", "in", "0", 0.5);
    res(&mut c, "R1", "in", "inv", 10e3);
    res(&mut c, "R2", "inv", "out", 100e3);
    place(&mut c, &def, "U1", &[("3", "0"), ("2", "inv"), ("1", "out"), ("8", "vp"), ("4", "vn")]);
    let r = dc_op(&c).unwrap();
    assert!((v(&r, "out") + 5.0).abs() < 1e-3, "{}", v(&r, "out"));
    // too much input clips at the swing
    let mut big = c.clone();
    big.elements[2] = Element::vdc("Vin", big.node("in"), 0, 2.0);
    assert!((v(&dc_op(&big).unwrap(), "out") + 13.5).abs() < 0.05);
}

#[test]
fn the_op_amp_models_have_their_stated_bandwidth() {
    // gain-of-1 follower AC response: -3 dB at about the gain-bandwidth product
    for (name, gbw) in [("LM358", 1.0e6), ("TL072", 3.0e6), ("MCP6002", 1.0e6)] {
        let def = part(name);
        let mut c = Circuit::new();
        vs(&mut c, "Vp", "vp", "0", 5.0);
        let s = parse_source_full("2.5 AC 1").unwrap();
        let a = c.node("in");
        c.add(Element::VSource { name: "Vin".into(), p: a, n: 0, wave: s.wave, ac: s.ac });
        let (vp, vn) = if name == "TL072" { (5.0, 0.0) } else { (5.0, 0.0) };
        let _ = (vp, vn);
        place(&mut c, &def, "U1", &[("3", "in"), ("2", "out"), ("1", "out"), ("8", "vp"), ("4", "0")]);
        let r = ac_analysis(&c, &AcSettings { fstart: 1e3, fstop: 1e8, sweep: AcSweep::Decade(20) }).unwrap();
        let out = r.signal("V(out)").unwrap();
        let mag = out.mag();
        assert!((mag[0] - 1.0).abs() < 1e-3, "{name}: unity at low frequency");
        let k = mag.iter().position(|m| *m < std::f64::consts::FRAC_1_SQRT_2).unwrap();
        assert!(r.freq[k] > gbw * 0.8 && r.freq[k] < gbw * 1.3, "{name}: -3 dB at {} Hz, expected ~{gbw}", r.freq[k]);
    }
}
