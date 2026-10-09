//! The second batch of bundled library parts, simulated through their own models.

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
fn the_new_mosfets_switch_a_load() {
    for (name, g, d, s) in [("BSS138", "1", "3", "2"), ("IRLZ44N", "1", "2", "3")] {
        let def = part(name);
        let run = |vg: f64| {
            let mut c = Circuit::new();
            vs(&mut c, "Vdd", "vdd", "0", 5.0);
            vs(&mut c, "Vg", "gate", "0", vg);
            res(&mut c, "Rd", "vdd", "drain", 50.0);
            place(&mut c, &def, "Q1", &[(g, "gate"), (d, "drain"), (s, "0")]);
            dc_op(&c).unwrap()
        };
        assert!(v(&run(5.0), "drain") < 0.5, "{name} is on at 5 V gate: {}", v(&run(5.0), "drain"));
        assert!((v(&run(0.0), "drain") - 5.0).abs() < 1e-3, "{name} is off at 0 V");
    }
    // the power MOSFET really is low-resistance: 0.1 A through 50 ohm loses a few mV
    let def = part("IRLZ44N");
    let mut c = Circuit::new();
    vs(&mut c, "Vdd", "vdd", "0", 5.0);
    vs(&mut c, "Vg", "gate", "0", 5.0);
    res(&mut c, "Rd", "vdd", "drain", 50.0);
    place(&mut c, &def, "Q1", &[("1", "gate"), ("2", "drain"), ("3", "0")]);
    assert!(v(&dc_op(&c).unwrap(), "drain") < 0.02);
}

#[test]
fn the_new_bipolar_transistors_switch_a_load() {
    // NPN low-side switch: base through 10k from 5 V
    for (name, c_pin, b_pin, e_pin) in [("PN2222A", "3", "2", "1"), ("S8050", "3", "1", "2")] {
        let def = part(name);
        let mut c = Circuit::new();
        vs(&mut c, "Vcc", "vcc", "0", 5.0);
        res(&mut c, "Rb", "vcc", "base", 10e3);
        res(&mut c, "Rc", "vcc", "col", 1e3);
        place(&mut c, &def, "Q1", &[(c_pin, "col"), (b_pin, "base"), (e_pin, "0")]);
        let r = dc_op(&c).unwrap();
        assert!(v(&r, "col") < 0.3, "{name} saturates: {}", v(&r, "col"));
    }
    // PNP high-side switch: base pulled to ground through 10k
    let def = part("S8550");
    let mut c = Circuit::new();
    vs(&mut c, "Vcc", "vcc", "0", 5.0);
    res(&mut c, "Rb", "base", "0", 10e3);
    res(&mut c, "Rl", "load", "0", 1e3);
    place(&mut c, &def, "Q1", &[("3", "load"), ("1", "base"), ("2", "vcc")]);
    let r = dc_op(&c).unwrap();
    assert!(v(&r, "load") > 4.5, "S8550 passes the supply to the load: {}", v(&r, "load"));
}

#[test]
fn schottky_diodes_drop_less_than_silicon_and_the_zener_clamps() {
    let drop = |name: &str, a: &str, k: &str| {
        let def = part(name);
        let mut c = Circuit::new();
        vs(&mut c, "V1", "in", "0", 5.0);
        res(&mut c, "R1", "in", "anode", 470.0); // about 10 mA
        place(&mut c, &def, "D1", &[(a, "anode"), (k, "0")]);
        v(&dc_op(&c).unwrap(), "anode")
    };
    let (s1, s2, si) = (drop("1N5817", "A", "K"), drop("SS14", "2", "1"), drop("1N4007", "A", "K"));
    assert!(s1 < 0.45 && s2 < 0.45, "schottky {s1} {s2}");
    assert!(si > 0.55 && si < 0.85, "silicon {si}");
    // a 3.3 V zener across the supply through 330 ohm
    let def = part("1N4728A");
    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", 5.0);
    res(&mut c, "R1", "in", "k", 330.0);
    place(&mut c, &def, "D1", &[("A", "0"), ("K", "k")]);
    let z = v(&dc_op(&c).unwrap(), "k");
    assert!((z - 3.3).abs() < 0.4, "{z}");
}

#[test]
fn the_adjustable_regulator_follows_the_divider_formula() {
    let def = part("LM317");
    let run = |vin: f64, r1: f64, r2: f64, load: f64| {
        let mut c = Circuit::new();
        vs(&mut c, "Vin", "in", "0", vin);
        res(&mut c, "R1", "out", "adj", r1);
        res(&mut c, "R2", "adj", "0", r2);
        res(&mut c, "Rl", "out", "0", load);
        place(&mut c, &def, "U1", &[("3", "in"), ("1", "adj"), ("2", "out")]);
        dc_op(&c).unwrap()
    };
    // 1.25 * (1 + 720/240) + 50 uA * 720 = 5.04 V
    let r = run(12.0, 240.0, 720.0, 100.0);
    assert!((v(&r, "out") - 5.04).abs() < 0.1, "{}", v(&r, "out"));
    // 1.25 * (1 + 1.5k/120) = 16.9 V from 24 V
    let r = run(24.0, 120.0, 1500.0, 1e3);
    assert!((v(&r, "out") - 16.9).abs() < 0.5, "{}", v(&r, "out"));
    // too little input: the output follows about 2 V below it
    let r = run(4.0, 240.0, 720.0, 100.0);
    assert!(v(&r, "out") < 2.3 && v(&r, "out") > 1.5, "{}", v(&r, "out"));
}

#[test]
fn the_fixed_regulators_hold_their_voltage_and_drop_out() {
    let cases = [("LM7812", ["1", "2", "3"], 15.0, 12.0), ("AMS1117-5.0", ["3", "1", "2"], 7.0, 5.0), ("MCP1700-3302", ["3", "1", "2"], 5.0, 3.3)];
    for (name, [inp, gnd, out], vin, vout) in cases {
        let def = part(name);
        let run = |vin: f64| {
            let mut c = Circuit::new();
            vs(&mut c, "Vin", "in", "0", vin);
            res(&mut c, "Rl", "out", "0", vout * 100.0); // ~10 mA
            place(&mut c, &def, "U1", &[(inp, "in"), (gnd, "0"), (out, "out")]);
            dc_op(&c).unwrap()
        };
        assert!((v(&run(vin), "out") - vout).abs() < 0.05, "{name}: {}", v(&run(vin), "out"));
        assert!(v(&run(vout + 0.05), "out") < vout - 0.05, "{name} drops out when the input is too low");
    }
}

#[test]
fn the_comparator_pulls_low_and_the_op_amps_buffer() {
    let lm393 = part("LM393");
    let run = |inp: f64, inn: f64| {
        let mut c = Circuit::new();
        vs(&mut c, "Vcc", "vcc", "0", 5.0);
        vs(&mut c, "Vp", "p", "0", inp);
        vs(&mut c, "Vn", "n", "0", inn);
        res(&mut c, "Rp", "vcc", "out", 10e3);
        place(&mut c, &lm393, "U1", &[("3", "p"), ("2", "n"), ("1", "out"), ("8", "vcc"), ("4", "0")]);
        v(&dc_op(&c).unwrap(), "out")
    };
    assert!(run(2.0, 1.0) > 4.9, "IN+ above IN-: the open collector lets the pull-up win");
    assert!(run(1.0, 2.0) < 0.2, "IN- above IN+: output pulled low");

    // voltage followers on +-5 V supplies
    for (name, pins) in [("NE5532", ["3", "2", "1", "8", "4"]), ("LM324", ["3", "2", "1", "4", "11"])] {
        let def = part(name);
        let mut c = Circuit::new();
        vs(&mut c, "Vp", "vp", "0", 5.0);
        vs(&mut c, "Vn", "0", "vn", 5.0);
        vs(&mut c, "Vi", "in", "0", 1.5);
        res(&mut c, "Rl", "out", "0", 10e3);
        place(&mut c, &def, "U1", &[(pins[0], "in"), (pins[1], "out"), (pins[2], "out"), (pins[3], "vp"), (pins[4], "vn")]);
        let r = dc_op(&c).unwrap();
        assert!((v(&r, "out") - 1.5).abs() < 0.01, "{name}: {}", v(&r, "out"));
    }
}
