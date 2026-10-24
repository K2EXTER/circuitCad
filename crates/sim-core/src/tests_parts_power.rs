//! Power and sensor parts of the bundled library, simulated through their own models.

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
fn temp_in_an_expression_follows_the_circuit_temperature() {
    let mut c = Circuit::new();
    let n = c.node("n");
    let expr = Expr::parse("temp/100", &mut |_: &str| n, &|s: &str| s.to_string()).unwrap();
    c.add(Element::BSource { name: "B1".into(), p: n, n: 0, voltage: true, expr });
    res(&mut c, "R1", "n", "0", 1e3);
    assert!((v(&dc_op(&c).unwrap(), "n") - 0.27).abs() < 1e-6, "27 degrees when nothing is set");
    assert!((v(&dc_op(&c.at_temperature(100.0)).unwrap(), "n") - 1.0).abs() < 1e-6);
    assert!((v(&dc_op(&c.at_temperature(-10.0)).unwrap(), "n") + 0.1).abs() < 1e-6);
}

#[test]
fn the_temperature_sensors_follow_the_temperature() {
    let run = |name: &str, vcc: f64, t: f64| {
        let def = part(name);
        let mut c = Circuit::new();
        vs(&mut c, "Vcc", "vcc", "0", vcc);
        res(&mut c, "Rl", "out", "0", 100e3);
        place(&mut c, &def, "U1", &[("1", "vcc"), ("2", "out"), ("3", "0")]);
        v(&dc_op(&c.at_temperature(t)).unwrap(), "out")
    };
    for (t, want) in [(25.0, 0.25), (100.0, 1.0)] {
        let got = run("LM35", 5.0, t);
        assert!((got - want).abs() < 0.01, "LM35 at {t}: {got}");
    }
    assert!(run("LM35", 5.0, -20.0) < 0.01, "single supply LM35 stops at 0 V");
    assert!(run("LM35", 2.0, 25.0) < 0.05, "LM35 is off below its supply range");
    for (t, want) in [(25.0, 0.75), (100.0, 1.5), (-20.0, 0.3)] {
        let got = run("TMP36", 3.3, t);
        assert!((got - want).abs() < 0.01, "TMP36 at {t}: {got}");
    }
}

#[test]
fn thermistors_change_resistance_with_the_temperature() {
    // 1 V -> thermistor -> 1 kohm to ground: the resistance follows from the divider
    let ohms = |name: &str, t: f64| {
        let def = part(name);
        let mut c = Circuit::new();
        vs(&mut c, "V1", "in", "0", 1.0);
        res(&mut c, "Rs", "x", "0", 1e3);
        place(&mut c, &def, "RT1", &[("1", "in"), ("2", "x")]);
        let vx = v(&dc_op(&c.at_temperature(t)).unwrap(), "x");
        (1.0 - vx) / vx * 1e3
    };
    let r25 = ohms("NTC_10k_B3950", 25.0);
    assert!((r25 - 10e3).abs() < 50.0, "{r25}");
    let r100 = ohms("NTC_10k_B3950", 100.0);
    assert!(r100 > 600.0 && r100 < 760.0, "{r100}");
    assert!(ohms("NTC_10k_B3950", 0.0) > 30e3);
    assert!((ohms("PT100", 0.0) - 100.0).abs() < 0.05);
    assert!((ohms("PT100", 100.0) - 138.5).abs() < 0.2);
}

#[test]
fn the_tl431_holds_its_reference_and_sets_a_voltage_with_a_divider() {
    let def = part("TL431");
    // reference tied to the cathode: 2.495 V
    let mut c = Circuit::new();
    vs(&mut c, "Vin", "in", "0", 12.0);
    res(&mut c, "R1", "in", "k", 1e3);
    place(&mut c, &def, "U1", &[("1", "k"), ("2", "k"), ("3", "0")]);
    let k = v(&dc_op(&c).unwrap(), "k");
    assert!((k - 2.495).abs() < 0.02, "{k}");
    // 2.495 * (1 + 10k/10k) = 5 V
    let mut c = Circuit::new();
    vs(&mut c, "Vin", "in", "0", 12.0);
    res(&mut c, "R1", "in", "k", 1e3);
    res(&mut c, "Ra", "k", "ref", 10e3);
    res(&mut c, "Rb", "ref", "0", 10e3);
    place(&mut c, &def, "U1", &[("1", "k"), ("2", "ref"), ("3", "0")]);
    let k = v(&dc_op(&c).unwrap(), "k");
    assert!((k - 5.0).abs() < 0.1, "{k}");
    // the input falls below what is needed: the cathode just follows it
    let mut c = Circuit::new();
    vs(&mut c, "Vin", "in", "0", 3.0);
    res(&mut c, "R1", "in", "k", 1e3);
    res(&mut c, "Ra", "k", "ref", 10e3);
    res(&mut c, "Rb", "ref", "0", 10e3);
    place(&mut c, &def, "U1", &[("1", "k"), ("2", "ref"), ("3", "0")]);
    assert!(v(&dc_op(&c).unwrap(), "k") < 3.0);
}

#[test]
fn the_tl431_converges_from_zero_at_any_supply_and_holds_the_reference_within_a_millivolt() {
    // The cathode current used to be clipped at both ends (flat, so Newton's iteration had no slope to follow) and the iterate jumped
    // between the two flat parts for ever whenever a second nonlinear part (a comparator, an op-amp macro model) shared the circuit.
    let def = part("TL431");
    for vin in [3.5, 4.5, 5.0, 7.0, 12.0, 20.0, 36.0] {
        let mut c = Circuit::new();
        vs(&mut c, "Vin", "in", "0", vin);
        res(&mut c, "R1", "in", "k", 2.2e3);
        place(&mut c, &def, "U1", &[("1", "k"), ("2", "k"), ("3", "0")]);
        let k = v(&dc_op(&c).unwrap_or_else(|e| panic!("{vin} V: {e}")), "k");
        // from about 1 mA up the reference sits on 2.495 V
        if (vin - 2.495) / 2.2e3 > 1e-3 {
            assert!((k - 2.495).abs() < 0.002, "{vin} V: {k}");
        }
    }
}

#[test]
fn the_lm7809_gives_nine_volts() {
    let def = part("LM7809");
    let mut c = Circuit::new();
    vs(&mut c, "Vin", "in", "0", 12.0);
    res(&mut c, "Rl", "out", "0", 100.0);
    place(&mut c, &def, "U1", &[("1", "in"), ("2", "0"), ("3", "out")]);
    let o = v(&dc_op(&c).unwrap(), "out");
    assert!((o - 9.0).abs() < 0.2, "{o}");
}

#[test]
fn the_power_mosfets_switch_and_their_body_diodes_conduct_backwards() {
    // low-side switch: 12 V across 100 ohm, gate 0 V / 10 V
    for name in ["IRF540N", "IRFZ44N"] {
        let def = part(name);
        let run = |vg: f64, vdd: f64| {
            let mut c = Circuit::new();
            vs(&mut c, "Vdd", "vdd", "0", vdd);
            vs(&mut c, "Vg", "g", "0", vg);
            res(&mut c, "Rd", "vdd", "d", 100.0);
            place(&mut c, &def, "Q1", &[("1", "g"), ("2", "d"), ("3", "0")]);
            dc_op(&c).unwrap()
        };
        assert!(v(&run(10.0, 12.0), "d") < 0.1, "{name} on: {}", v(&run(10.0, 12.0), "d"));
        assert!((v(&run(0.0, 12.0), "d") - 12.0).abs() < 1e-3, "{name} off");
        // drain pulled below the source: the body diode clamps near -0.7 V
        let d = v(&run(0.0, -5.0), "d");
        assert!(d < -0.4 && d > -1.2, "{name} body diode: {d}");
    }
    // P-channel high-side switch: source on 12 V, gate 12 V (off) / 2 V (on)
    let def = part("IRF9540N");
    let run = |vg: f64| {
        let mut c = Circuit::new();
        vs(&mut c, "Vdd", "vdd", "0", 12.0);
        vs(&mut c, "Vg", "g", "0", vg);
        res(&mut c, "Rl", "d", "0", 100.0);
        place(&mut c, &def, "Q1", &[("1", "g"), ("2", "d"), ("3", "vdd")]);
        dc_op(&c).unwrap()
    };
    assert!(v(&run(2.0), "d") > 11.5, "on: {}", v(&run(2.0), "d"));
    assert!(v(&run(12.0), "d").abs() < 1e-3, "off");
}

#[test]
fn the_darlingtons_and_power_transistors_switch_a_load() {
    // NPN low-side: base through 10k from 5 V, 12 V across 100 ohm (max 120 mA)
    for (name, c_pin, b_pin, e_pin) in [("TIP120", "2", "1", "3"), ("BD139", "2", "3", "1")] {
        let def = part(name);
        let mut c = Circuit::new();
        vs(&mut c, "Vcc", "vcc", "0", 12.0);
        vs(&mut c, "Vb", "vb", "0", 5.0);
        res(&mut c, "Rb", "vb", "base", 10e3);
        res(&mut c, "Rc", "vcc", "col", 100.0);
        place(&mut c, &def, "Q1", &[(c_pin, "col"), (b_pin, "base"), (e_pin, "0")]);
        let r = dc_op(&c).unwrap();
        // BD139 only gets ~0.4 mA of base current: about 50 mA at hFE 120
        let limit = if name == "TIP120" { 1.0 } else { 8.0 };
        assert!(v(&r, "col") < limit, "{name} collector: {}", v(&r, "col"));
        if name == "TIP120" {
            // two base-emitter drops
            assert!(v(&r, "base") > 1.1 && v(&r, "base") < 1.9, "TIP120 base: {}", v(&r, "base"));
        }
    }
    // PNP high-side: base pulled to ground through 1k
    for (name, c_pin, b_pin, e_pin) in [("TIP125", "2", "1", "3"), ("BD140", "2", "3", "1")] {
        let def = part(name);
        let mut c = Circuit::new();
        vs(&mut c, "Vcc", "vcc", "0", 12.0);
        res(&mut c, "Rb", "base", "0", 1e3);
        res(&mut c, "Rl", "load", "0", 100.0);
        place(&mut c, &def, "Q1", &[(c_pin, "load"), (b_pin, "base"), (e_pin, "vcc")]);
        let r = dc_op(&c).unwrap();
        assert!(v(&r, "load") > 9.0, "{name} passes the supply: {}", v(&r, "load"));
    }
    // off with the base at the emitter level
    let def = part("TIP120");
    let mut c = Circuit::new();
    vs(&mut c, "Vcc", "vcc", "0", 12.0);
    res(&mut c, "Rc", "vcc", "col", 100.0);
    place(&mut c, &def, "Q1", &[("2", "col"), ("1", "0"), ("3", "0")]);
    assert!((v(&dc_op(&c).unwrap(), "col") - 12.0).abs() < 0.01);
}

#[test]
fn the_new_diodes_have_the_right_forward_drops_and_the_12v_zener_clamps() {
    let drop = |name: &str, a: &str, k: &str, vin: f64, r: f64| {
        let def = part(name);
        let mut c = Circuit::new();
        vs(&mut c, "V1", "in", "0", vin);
        res(&mut c, "R1", "in", "anode", r);
        place(&mut c, &def, "D1", &[(a, "anode"), (k, "0")]);
        v(&dc_op(&c).unwrap(), "anode")
    };
    // the 3 A diodes at about 1 A
    let sch = drop("1N5822", "A", "K", 5.0, 4.5);
    assert!(sch > 0.2 && sch < 0.5, "1N5822 {sch}");
    let si = drop("1N5408", "A", "K", 5.0, 4.0);
    assert!(si > 0.75 && si < 1.1, "1N5408 {si}");
    let fast = drop("UF4007", "A", "K", 5.0, 4.0);
    assert!(fast > 1.0 && fast < 1.8, "UF4007 {fast}");
    // BAT54 at 10 mA
    let b = drop("BAT54", "1", "3", 5.0, 470.0);
    assert!(b > 0.3 && b < 0.5, "BAT54 {b}");
    // 12 V zener
    let def = part("1N4742A");
    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", 24.0);
    res(&mut c, "R1", "in", "k", 470.0);
    place(&mut c, &def, "D1", &[("A", "0"), ("K", "k")]);
    let z = v(&dc_op(&c).unwrap(), "k");
    assert!((z - 12.0).abs() < 0.6, "{z}");
}

#[test]
fn the_stimulus_sensors_respond_to_their_input_pin() {
    // photocell: 1 V = 100 lx
    let ldr = |light: f64| {
        let def = part("LDR_GL5528");
        let mut c = Circuit::new();
        vs(&mut c, "V1", "in", "0", 1.0);
        vs(&mut c, "Vl", "l", "0", light);
        res(&mut c, "Rs", "x", "0", 1e3);
        place(&mut c, &def, "RL1", &[("1", "in"), ("2", "x"), ("L", "l")]);
        let vx = v(&dc_op(&c).unwrap(), "x");
        (1.0 - vx) / vx * 1e3
    };
    assert!((ldr(0.1) - 15e3).abs() < 200.0, "10 lx: {}", ldr(0.1));
    assert!((ldr(1.0) - 3.77e3).abs() < 100.0, "100 lx: {}", ldr(1.0));
    assert!(ldr(0.0) > 0.9e6, "dark: {}", ldr(0.0));

    // photodiode, reverse biased through 10k: 75 uA per mW/cm2
    let pd = |irr: f64| {
        let def = part("BPW34");
        let mut c = Circuit::new();
        vs(&mut c, "V1", "vcc", "0", 5.0);
        vs(&mut c, "Vl", "l", "0", irr);
        res(&mut c, "R1", "vcc", "k", 10e3);
        place(&mut c, &def, "D1", &[("K", "k"), ("A", "0"), ("L", "l")]);
        v(&dc_op(&c).unwrap(), "k")
    };
    assert!(pd(0.0) > 4.99, "dark: {}", pd(0.0));
    assert!((pd(1.0) - 4.25).abs() < 0.03, "1 mW/cm2: {}", pd(1.0));

    // Hall sensor: Vs/2 with no field, 1.4 mV/G at 5 V, saturates near the rails
    let hall = |b: f64, vcc: f64| {
        let def = part("SS49E");
        let mut c = Circuit::new();
        vs(&mut c, "Vcc", "vcc", "0", vcc);
        vs(&mut c, "Vb", "b", "0", b);
        res(&mut c, "Rl", "out", "0", 100e3);
        place(&mut c, &def, "U1", &[("1", "vcc"), ("2", "0"), ("3", "out"), ("B", "b")]);
        v(&dc_op(&c).unwrap(), "out")
    };
    assert!((hall(0.0, 5.0) - 2.5).abs() < 0.01);
    assert!((hall(1.0, 5.0) - 2.64).abs() < 0.01, "{}", hall(1.0, 5.0));
    assert!((hall(-1.0, 5.0) - 2.36).abs() < 0.01);
    assert!((hall(100.0, 5.0) - 4.8).abs() < 0.02, "{}", hall(100.0, 5.0));
    assert!((hall(0.0, 3.3) - 1.65).abs() < 0.01, "ratiometric");

    // ACS712: Vcc/2 + 185 mV/A
    let acs = |vin: f64| {
        let def = part("ACS712-05B");
        let mut c = Circuit::new();
        vs(&mut c, "Vcc", "vcc", "0", 5.0);
        vs(&mut c, "Vt", "t", "0", vin);
        res(&mut c, "Rt", "t", "ip", 0.5);
        res(&mut c, "Rl", "out", "0", 100e3);
        place(&mut c, &def, "U1", &[("1", "ip"), ("3", "0"), ("5", "0"), ("6", "filt"), ("7", "out"), ("8", "vcc")]);
        v(&dc_op(&c).unwrap(), "out")
    };
    assert!((acs(0.0) - 2.5).abs() < 0.01);
    let i = 1.0 / (0.5 + 0.0012);
    assert!((acs(1.0) - (2.5 + 0.185 * i)).abs() < 0.01, "{}", acs(1.0));
    assert!((acs(-1.0) - (2.5 - 0.185 * i)).abs() < 0.01);

    // INA180A2: 30 mV across a 10 mohm shunt -> 1.5 V
    for (name, gain) in [("INA180A1", 20.0), ("INA180A2", 50.0), ("INA180A3", 100.0)] {
        let def = part(name);
        let mut c = Circuit::new();
        vs(&mut c, "Vcc", "vcc", "0", 5.0);
        vs(&mut c, "Vs", "bus", "0", 12.0);
        res(&mut c, "Rsh", "bus", "load", 0.01);
        res(&mut c, "Rload", "load", "0", 12.0 / 3.0 - 0.01);
        res(&mut c, "Rout", "out", "0", 100e3);
        place(&mut c, &def, "U1", &[("3", "bus"), ("4", "load"), ("5", "vcc"), ("2", "0"), ("1", "out")]);
        let o = v(&dc_op(&c).unwrap(), "out");
        let want = (gain * 0.03_f64).min(4.8);
        assert!((o - want).abs() < 0.03, "{name}: {o} vs {want}");
    }
}
