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

/// The ADS124S08's equivalent model draws the data sheet's supply currents (SBAS660C 7.5: AVDD 120 µA with the PGA at gain 1-2
/// plus the positive reference buffer's 35 µA, DVDD and IOVDD together 225 µA), the reference input 5 nA at 2.5 V through its
/// buffer, and nothing from an unpowered supply.
#[test]
fn the_ads124s08_draws_its_supply_currents() {
    let def = part("ADS124S08");
    let mut c = Circuit::new();
    vs(&mut c, "Va", "avdd", "0", 3.3);
    vs(&mut c, "Vd", "dvdd", "0", 3.3);
    vs(&mut c, "Vref", "refp", "0", 2.5);
    vs(&mut c, "Vin", "in", "0", 1.65);
    let mut nets: Vec<(&str, &str)> = vec![("26", "avdd"), ("27", "0"), ("16", "dvdd"), ("15", "dvdd"), ("14", "0"), ("30", "refp"), ("29", "0"), ("23", "refout")];
    nets.extend(["7", "6", "5", "4", "3", "2", "1", "32", "31", "22", "21", "20", "19"].iter().map(|p| (*p, "in")));
    place(&mut c, &def, "U1", &nets);
    let r = dc_op(&c).unwrap();
    let amps = |name: &str| -r.element(name).unwrap().current;
    // 155 µA of its own, and 10 µA (4 µA/V of the 2.5 V reference) out of REFN0 through the REFP0 buffer
    assert!((amps("Va") - 165e-6).abs() < 2e-6, "AVDD {} A", amps("Va"));
    assert!((amps("Vd") - 225e-6).abs() < 3e-6, "DVDD + IOVDD {} A", amps("Vd"));
    assert!((amps("Vref") - 5e-9).abs() < 1e-9, "REFP0 {} A", amps("Vref"));
    assert!(amps("Vin").abs() < 13.0 * 0.2e-9, "the 13 inputs about 0.1 nA each: {} A", amps("Vin"));
    // no supply: no current, and the supply does not go below 0 V
    let mut c = Circuit::new();
    vs(&mut c, "Vd", "dvdd", "0", 3.3);
    nets[0] = ("26", "avdd");
    place(&mut c, &def, "U1", &nets);
    let ra = c.node("avdd");
    c.add(Element::Resistor { name: "Rleak".into(), a: ra, b: 0, r: 1e6 });
    let r = dc_op(&c).unwrap();
    assert!(v(&r, "avdd").abs() < 0.01, "an unpowered AVDD stays near 0 V: {}", v(&r, "avdd"));
}

/// The BAV199's diodes: below the forward voltages' maxima of the data sheet's table 7 and near the fig. 2 typical curve, and a
/// reverse current at 75 V under the table's maximum (the model's own 3 pA plus the simulator's gmin).
#[test]
fn the_bav199_keeps_its_leakage_in_picoamps_and_its_forward_drops() {
    let def = part("BAV199");
    let drop = |amps: f64| {
        let mut c = Circuit::new();
        let n = c.node("a");
        c.add(Element::ISource { name: "If".into(), from: 0, to: n, wave: Waveform::Dc(amps), ac: None });
        place(&mut c, &def, "D1", &[("1", "a"), ("3", "k"), ("2", "x")]);
        res(&mut c, "Rx", "x", "0", 1e15);
        res(&mut c, "Rk", "k", "0", 1e-3);
        v(&dc_op(&c).unwrap(), "a")
    };
    for (amps, max, curve) in [(1e-3, 0.9, 0.745), (10e-3, 1.0, 0.85), (50e-3, 1.1, 0.96), (150e-3, 1.25, 1.05)] {
        let vf = drop(amps);
        assert!(vf < max && (vf - curve).abs() < 0.03, "{amps} A: {vf} V (max {max}, curve about {curve})");
    }
    let mut c = Circuit::new();
    vs(&mut c, "Vr", "k", "0", 75.0);
    place(&mut c, &def, "D1", &[("1", "0"), ("3", "k"), ("2", "x")]);
    res(&mut c, "Rx", "x", "k", 1e-3);
    // the model's own leakage is IS = 3 pA; the simulator's gmin (1e-12 S across the junction and from each node to ground) adds
    // about 75 pA for each place at 75 V, so the reverse current read here is a few hundred pA: under the data sheet's 5 nA
    let leak = dc_op(&c).unwrap().element("Vr").unwrap().current.abs();
    assert!(leak < 5e-9 && leak > 3e-12, "reverse current {leak} A (typ 3 pA, max 5 nA)");
}

/// The TQ2-L2-5V: 40 mA in each coil from 5 V (125 Ω), and contacts held by a latch: a pulse on the set coil moves COM to SET and
/// it stays there when the pulse ends; a pulse on the reset coil moves it back. It starts reset.
#[test]
fn the_tq2_l2_5v_latches_on_a_pulse_and_stays() {
    let def = part("TQ2-L2-5V");
    let wire = |c: &mut Circuit| {
        place(c, &def, "K1", &[("1", "s"), ("5", "0"), ("10", "r"), ("6", "0"), ("3", "com"), ("4", "set1"), ("2", "rst1"), ("8", "com"), ("7", "set2"), ("9", "rst2")]);
        vs(c, "Vc", "com", "0", 1.0);
        for n in ["set1", "rst1", "set2", "rst2"] {
            res(c, &format!("R{n}"), n, "0", 1000.0);
        }
    };
    // the coils at 5 V
    let mut c = Circuit::new();
    vs(&mut c, "Vs", "s", "0", 5.0);
    vs(&mut c, "Vr", "r", "0", 0.0);
    wire(&mut c);
    let r = dc_op(&c).unwrap();
    assert!((-r.element("Vs").unwrap().current - 0.040).abs() < 1e-4, "set coil {} A", -r.element("Vs").unwrap().current);
    // pulses: set at 1 ms for 2 ms, reset at 5 ms for 2 ms
    let pulse = |td: f64| Waveform::Pulse { v1: 0.0, v2: 5.0, td, tr: 1e-5, tf: 1e-5, pw: 2e-3, per: 1.0 };
    let mut c = Circuit::new();
    let (s, r) = (c.node("s"), c.node("r"));
    c.add(Element::VSource { name: "Vs".into(), p: s, n: 0, wave: pulse(1e-3), ac: None });
    c.add(Element::VSource { name: "Vr".into(), p: r, n: 0, wave: pulse(5e-3), ac: None });
    wire(&mut c);
    let t = transient(&c, &TranSettings { tstop: 10e-3, tstep: 1e-5, uic: false }).unwrap();
    let at = |node: &str, when: f64| {
        let sig = t.signal(&format!("V({node})")).unwrap();
        let k = t.time.iter().position(|x| *x >= when).unwrap();
        sig.values[k]
    };
    // the contacts move 3 ms after the coil reaches its set / reset voltage (set / reset time: max 3 ms)
    for (when, set) in [(0.5e-3, false), (3.5e-3, false), (4.5e-3, true), (7.5e-3, true), (9.5e-3, false)] {
        let (on, off) = if set { ("set1", "rst1") } else { ("rst1", "set1") };
        assert!(at(on, when) > 0.99 && at(off, when) < 1e-3, "{} ms: {on} {} V, {off} {} V", when * 1e3, at(on, when), at(off, when));
    }
    assert!(at("set2", 4.5e-3) > 0.99 && at("rst2", 4.5e-3) < 1e-3, "the second contact moves with the first");
}

/// Bourns MF-MSMF150/24X (a PTC, resettable): 1 A (below its 1.5 A hold) flows on; 5 A (over its 3 A trip) trips it within a few
/// seconds, and then only what keeps it near 1 W flows (about 0.2 A at 5 V).
#[test]
fn the_mf_msmf150_trips_over_its_trip_current_and_holds_below() {
    let def = part("MF-MSMF150/24X");
    let run = |load: f64| {
        let mut c = Circuit::new();
        vs(&mut c, "V1", "in", "0", 5.0);
        place(&mut c, &def, "F1", &[("1", "in"), ("2", "out")]);
        res(&mut c, "RL", "out", "0", load);
        let t = transient(&c, &TranSettings { tstop: 20.0, tstep: 0.01, uic: false }).unwrap();
        let v = t.signal("V(out)").unwrap();
        let at = |when: f64| v.values[t.time.iter().position(|x| *x >= when).unwrap()] / load;
        (at(0.05), at(19.9))
    };
    let (start, end) = run(5.0);
    assert!((start - 1.0).abs() < 0.02 && (end - 1.0).abs() < 0.02, "1 A holds: {start} A, then {end} A");
    let (start, end) = run(1.0);
    assert!(start > 4.5 && end < 0.3, "5 A trips: {start} A, then {end} A");
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

    // photodiode, reverse biased through 10k: 50 uA per mW/cm2 (the data sheet's Ira at 950 nm)
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
    assert!((pd(1.0) - 4.5).abs() < 0.03, "1 mW/cm2: {}", pd(1.0));

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

#[test]
fn the_buck_demo_works_with_its_output_as_the_system_ground() {
    // The IC sits on a floating negative rail: the battery's minus is the gnd pin, the inductor's far end is the circuit's 0 V.
    // Every voltage inside the model used to be taken against node 0, so this wiring did not regulate.
    let def = part("BUCK_DEMO");
    let mut c = Circuit::new();
    vs(&mut c, "Vbat", "bat", "neg", 9.0);
    place(&mut c, &def, "U1", &[("1", "bat"), ("2", "neg"), ("3", "sw"), ("4", "fb")]);
    let (sw, out, neg, fb, gnd) = (c.node("sw"), c.node("0"), c.node("neg"), c.node("fb"), 0);
    let _ = out;
    c.add(Element::Inductor { name: "L1".into(), a: sw, b: gnd, l: 33e-6 });
    c.add(Element::Diode { name: "D1".into(), a: neg, k: sw, model: DiodeModel { rs: 0.05, ..DiodeModel::default() } });
    c.add(Element::Capacitor { name: "C1".into(), a: gnd, b: neg, c: 470e-6 });
    res(&mut c, "Rt", "0", "fb", 4.64e3);
    res(&mut c, "Rb", "fb", "neg", 10e3);
    res(&mut c, "Rl", "0", "neg", 1.8);
    let _ = fb;
    let r = transient(&c, &TranSettings { tstop: 8e-3, tstep: 1e-7, uic: false }).unwrap();
    let vneg = &r.signal("V(neg)").unwrap().values;
    let tail: Vec<f64> = vneg.iter().zip(&r.time).filter(|(_, t)| **t > 6e-3).map(|(v, _)| -v).collect();
    let mean = tail.iter().sum::<f64>() / tail.len() as f64;
    assert!((mean - 1.8).abs() < 0.05, "the output should settle at 1.8 V, not {mean}");
}

#[test]
fn the_74hc4017_counts_one_output_at_a_time_and_wraps_after_ten() {
    let def = part("74HC4017");
    let pin = |n: &str| def.pins.iter().find(|p| p.name == n).unwrap().number.clone();
    let mut c = Circuit::new();
    vs(&mut c, "Vcc", "vcc", "0", 5.0);
    let a = c.node("cp");
    let s = parse_source_full("PULSE(0 5 100u 1u 1u 50u 100u)").unwrap();
    c.add(Element::VSource { name: "Vcp".into(), p: a, n: 0, wave: s.wave, ac: s.ac });
    // MR is high at the start (the operating point is then a reset counter) and falls at 40 us
    let m = c.node("mr");
    let s = parse_source_full("PULSE(5 0 40u 1u 1u 1 2)").unwrap();
    c.add(Element::VSource { name: "Vmr".into(), p: m, n: 0, wave: s.wave, ac: s.ac });
    let names: Vec<String> = (0..10).map(|k| format!("q{k}")).collect();
    let mut nets: Vec<(String, String)> = vec![(pin("VCC"), "vcc".into()), (pin("GND"), "0".into()), (pin("CE"), "0".into()), (pin("CP"), "cp".into()), (pin("MR"), "mr".into()), (pin("CO"), "co".into())];
    for (k, n) in names.iter().enumerate() {
        nets.push((pin(&format!("Q{k}")), n.clone()));
    }
    let wiring: Vec<(&str, &str)> = nets.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    place(&mut c, &def, "U1", &wiring);
    let r = transient(&c, &TranSettings { tstop: 1.45e-3, tstep: 1e-6, uic: false }).unwrap();
    let at = |net: &str, t: f64| {
        let i = r.time.iter().position(|x| *x >= t).unwrap();
        r.signal(&format!("V({net})")).unwrap().values[i]
    };
    // the clock rises at 100 us, 200 us, ...: after n rising edges output n mod 10 is the only one that is high
    for edges in 0..=12usize {
        let t = 100e-6 * edges as f64 + 90e-6;
        if t < 60e-6 {
            continue;
        }
        let want = edges % 10;
        let high: Vec<usize> = (0..10).filter(|k| at(&names[*k], t) > 2.5).collect();
        assert_eq!(high, vec![want], "after {edges} clock edges (t = {t})");
    }
    // carry out: high while the count is 0..4
    assert!(at("co", 100e-6 * 2.0 + 90e-6) > 2.5 && at("co", 100e-6 * 7.0 + 90e-6) < 2.5);
}

/// The 74HC4017 counts on a 3.3 V supply and on a floating one (GND pin at -1.8 V, VCC pin at +3.0 V): the levels follow the supply pins.
#[test]
fn the_74hc4017_counts_on_3v3_and_on_a_floating_supply() {
    for (vss, vcc) in [(0.0, 3.3), (-1.8, 3.0)] {
        let def = part("74HC4017");
        let pin = |n: &str| def.pins.iter().find(|p| p.name == n).unwrap().number.clone();
        let mut c = Circuit::new();
        vs(&mut c, "Vss", "vss", "0", vss);
        vs(&mut c, "Vcc", "vcc", "0", vcc);
        let span = vcc - vss;
        let a = c.node("cp");
        let s = parse_source_full(&format!("PULSE({vss} {} 100u 1u 1u 50u 100u)", vss + span)).unwrap();
        c.add(Element::VSource { name: "Vcp".into(), p: a, n: 0, wave: s.wave, ac: s.ac });
        let m = c.node("mr");
        let s = parse_source_full(&format!("PULSE({} {vss} 40u 1u 1u 1 2)", vss + span)).unwrap();
        c.add(Element::VSource { name: "Vmr".into(), p: m, n: 0, wave: s.wave, ac: s.ac });
        let names: Vec<String> = (0..10).map(|k| format!("q{k}")).collect();
        let mut nets: Vec<(String, String)> = vec![(pin("VCC"), "vcc".into()), (pin("GND"), "vss".into()), (pin("CE"), "vss".into()), (pin("CP"), "cp".into()), (pin("MR"), "mr".into()), (pin("CO"), "co".into())];
        for (k, n) in names.iter().enumerate() {
            nets.push((pin(&format!("Q{k}")), n.clone()));
        }
        let wiring: Vec<(&str, &str)> = nets.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        place(&mut c, &def, "U1", &wiring);
        let r = transient(&c, &TranSettings { tstop: 1.45e-3, tstep: 1e-6, uic: false }).unwrap();
        let at = |net: &str, t: f64| {
            let i = r.time.iter().position(|x| *x >= t).unwrap();
            r.signal(&format!("V({net})")).unwrap().values[i]
        };
        for edges in 1..=12usize {
            let t = 100e-6 * edges as f64 + 90e-6;
            let want = edges % 10;
            let high: Vec<usize> = (0..10).filter(|k| at(&names[*k], t) > vss + 0.5 * span).collect();
            assert_eq!(high, vec![want], "after {edges} clock edges on {vss}..{vcc}");
            // and the high level is the supply, not 5 V
            assert!((at(&names[want], t) - vcc).abs() < 0.15, "high output {} on {vss}..{vcc}", at(&names[want], t));
        }
    }
}
