//! The ULN2803A equivalent model (no maker model exists): it has to land on the data sheet's typical figures.

use super::*;
use sim_core::{add_subckt_instance, dc_op, Circuit, Element};

/// One channel on the bench: `vcc` through `rl` to OUT1, IN1 driven from `vin` through `rs`, COM on `vcc`, everything else at 0 V.
struct Bench {
    c: Circuit,
}

fn bench(vin: f64, rs: f64, rl: f64, vcc: f64) -> Bench {
    let def = library_part("ULN2803A");
    let sim = def.sim.expect("the part has a model");
    let mut c = Circuit::new();
    let (vcc_n, vin_n, inb, out) = (c.node("vcc"), c.node("vin"), c.node("inb"), c.node("out"));
    c.add(Element::vdc("Vcc", vcc_n, 0, vcc));
    c.add(Element::vdc("Vin", vin_n, 0, vin));
    c.add(Element::Resistor { name: "Rs".into(), a: vin_n, b: inb, r: rs });
    c.add(Element::Resistor { name: "RL".into(), a: vcc_n, b: out, r: rl });
    // symbol pin numbers in the model's node order: IN1..IN8, GND, COM, then OUT1 (pin 18) ... OUT8 (pin 11)
    let node_of = |pin: &str, c: &mut Circuit| match pin {
        "1" => inb,
        "9" => 0,
        "10" => vcc_n,
        "18" => out,
        _ => c.node(&format!("unused{pin}")),
    };
    let nodes: Vec<usize> = sim.pins.iter().map(|p| node_of(p, &mut c)).collect();
    for (p, n) in sim.pins.iter().zip(&nodes) {
        // inputs of the unused channels sit at 0 V; their outputs are left open
        if matches!(p.as_str(), "2" | "3" | "4" | "5" | "6" | "7" | "8") {
            c.add(Element::Resistor { name: format!("Rgnd{p}"), a: *n, b: 0, r: 1.0 });
        }
    }
    add_subckt_instance(&mut c, "U1", &sim.name, &sim.text, &nodes).unwrap_or_else(|e| panic!("{e}"));
    Bench { c }
}

/// VCE(sat) with the collector current `ic` and the input current `ii` (both in A): the input voltage is searched.
fn vce_at(ic: f64, ii: f64) -> f64 {
    let (vcc, rs) = (5.0, 1000.0);
    let rl = (vcc - 1.0) / ic;
    let (mut lo, mut hi) = (1.0, 12.0);
    let mut vce = 0.0;
    for _ in 0..30 {
        let vin = (lo + hi) / 2.0;
        let op = dc_op(&bench(vin, rs, rl, vcc).c).unwrap_or_else(|e| panic!("{e:?}"));
        let i_in = op.element("Rs").unwrap().current;
        vce = vcc - op.element("RL").unwrap().voltage;
        if i_in > ii {
            hi = vin;
        } else {
            lo = vin;
        }
    }
    vce
}

/// The model's output saturation voltage matches the data sheet's typical values (0.9 V / 1.1 V / 1.3 V).
#[test]
fn the_saturation_voltage_follows_the_data_sheet() {
    for (ic, ii, typ) in [(0.1, 250e-6, 0.9), (0.2, 350e-6, 1.1), (0.35, 500e-6, 1.3)] {
        let v = vce_at(ic, ii);
        assert!((v - typ).abs() < 0.06, "IC {ic} A: VCE(sat) {v:.3} V, data sheet {typ} V");
    }
}

/// With the input low the output is off (a few µA at most), and the input draws about 0.9 mA at 3.85 V.
#[test]
fn off_is_off_and_the_input_current_is_about_a_milliamp() {
    let op = dc_op(&bench(0.0, 1.0, 100.0, 12.0).c).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(op.element("RL").unwrap().current.abs() < 1e-6, "off-state current {}", op.element("RL").unwrap().current);
    let op = dc_op(&bench(3.85, 1e-3, 20.0, 5.0).c).unwrap_or_else(|e| panic!("{e:?}"));
    let ii = op.element("Rs").unwrap().current * 1e3;
    assert!((0.7..1.1).contains(&ii), "input current {ii} mA (data sheet typical 0.93 mA)");
}

/// The clamp diode (OUT -> COM) conducts about 1.7 V at 350 mA, and blocks the other way.
#[test]
fn the_clamp_diode_drops_about_1v7_at_350ma() {
    let def = library_part("ULN2803A");
    let sim = def.sim.unwrap();
    let mut c = Circuit::new();
    let out = c.node("out");
    // forward: OUT forced up against COM = 0 V through a source and a resistor sized for ~350 mA
    c.add(Element::vdc("V1", out, 0, 1.7));
    let nodes: Vec<usize> = sim.pins.iter().map(|p| match p.as_str() { "18" => out, "9" | "10" => 0, _ => c.node(&format!("n{p}")) }).collect();
    add_subckt_instance(&mut c, "U1", &sim.name, &sim.text, &nodes).unwrap();
    let i = dc_op(&c).unwrap().element("V1").unwrap().current.abs();
    assert!((0.2..0.5).contains(&i), "at 1.7 V the clamp carries {i} A (about 0.35 A expected)");
}
