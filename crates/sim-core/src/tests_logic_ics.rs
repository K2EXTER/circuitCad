//! The counters, decoders and selectors added after the first logic ICs (74HC161, 74HC138, 74HC151).

use crate::*;
use part_lib::{PartDef, EXAMPLES};

fn part(name: &str) -> PartDef {
    EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<PartDef>(t).unwrap()).find(|p| p.name == name).unwrap_or_else(|| panic!("no bundled part {name}"))
}

/// `U1` of `name` with the pins wired to nets named after the pins.
fn place(c: &mut Circuit, name: &str) -> PartDef {
    let def = part(name);
    let sim = def.sim.as_ref().unwrap();
    let nodes: Vec<usize> = sim.pins.iter().map(|n| c.node(&format!("p_{}", def.pins.iter().find(|p| &p.number == n).unwrap().name))).collect();
    add_subckt_instance(c, "U1", &sim.name, &sim.text, &nodes).unwrap_or_else(|e| panic!("{name}: {e}"));
    def
}

fn node(c: &mut Circuit, pin: &str) -> usize {
    c.node(&format!("p_{pin}"))
}

fn source(c: &mut Circuit, name: &str, pin: &str, text: &str) {
    let n = node(c, pin);
    let s = parse_source_full(text).unwrap();
    c.add(Element::VSource { name: name.into(), p: n, n: 0, wave: s.wave, ac: s.ac });
}

fn supply(c: &mut Circuit, vcc: f64) {
    let (v, g) = (node(c, "VCC"), node(c, "GND"));
    c.add(Element::vdc("Vcc", v, 0, vcc));
    c.add(Element::Resistor { name: "Rg".into(), a: g, b: 0, r: 1e-3 });
}

fn load(c: &mut Circuit, pin: &str) {
    let n = node(c, pin);
    c.add(Element::Resistor { name: format!("RL_{pin}"), a: n, b: 0, r: 100e3 });
}

fn at(r: &TranResult, pin: &str, t: f64) -> f64 {
    let k = r.time.iter().position(|x| *x >= t).unwrap();
    r.signal(&format!("V(p_{pin})")).unwrap().values[k]
}

/// The 74HC161 counts 0..15 and wraps, RCO is high only at 15 (with ENT), LOAD loads A..D, CLR clears at once.
#[test]
fn the_74hc161_counts_loads_and_clears() {
    for vcc in [5.0, 3.3] {
        let h = format!("{vcc}");
        let mut c = Circuit::new();
        place(&mut c, "74HC161");
        supply(&mut c, vcc);
        // the clock rises at 0.9 us, 1.9 us, ...; CLR is low until 0.5 us; ENP and ENT are high
        source(&mut c, "Vclk", "CLK", &format!("PULSE(0 {h} 0.9u 1n 1n 0.4u 1u)"));
        source(&mut c, "Vclr", "CLR", &format!("PWL(0 0 0.5u 0 0.51u {h} 40u {h})"));
        source(&mut c, "Venp", "ENP", &format!("{vcc}"));
        source(&mut c, "Vent", "ENT", &format!("{vcc}"));
        // LOAD is low from 20.3 us to 21.4 us: the edge at 20.9 us loads 0b1010 = 10 (A=0 B=1 C=0 D=1)
        source(&mut c, "Vload", "LOAD", &format!("PWL(0 {h} 20.3u {h} 20.31u 0 21.4u 0 21.41u {h} 40u {h})"));
        source(&mut c, "Va", "A", "0");
        source(&mut c, "Vb", "B", &format!("{vcc}"));
        source(&mut c, "Vc", "C", "0");
        source(&mut c, "Vd", "D", &format!("{vcc}"));
        for p in ["QA", "QB", "QC", "QD", "RCO"] {
            load(&mut c, p);
        }
        let r = transient(&c, &TranSettings { tstop: 30e-6, tstep: 20e-9, uic: false }).unwrap();
        let value = |t: f64| -> usize { ["QA", "QB", "QC", "QD"].iter().enumerate().map(|(i, p)| ((at(&r, p, t) > vcc / 2.0) as usize) << i).sum() };
        // after the j-th rising edge the count is j, from the cleared 0, up to the edge that loads
        for j in 1..=19usize {
            let t = 0.9e-6 + (j as f64 - 1.0) * 1e-6 + 0.6e-6;
            assert_eq!(value(t), j % 16, "after {j} edges on {vcc} V");
            assert_eq!(at(&r, "RCO", t) > vcc / 2.0, j % 16 == 15, "RCO after {j} edges");
        }
        assert_eq!(value(21.5e-6), 10, "the loaded value on {vcc} V");
        assert_eq!(value(22.5e-6), 11, "and it goes on from there");
    }
}

/// Clear is asynchronous: a low CLR clears the count between clock edges, and ENP low stops the count.
#[test]
fn the_74hc161_clear_is_asynchronous_and_the_enables_hold_the_count() {
    let mut c = Circuit::new();
    place(&mut c, "74HC161");
    supply(&mut c, 5.0);
    source(&mut c, "Vclk", "CLK", "PULSE(0 5 0.9u 1n 1n 0.4u 1u)");
    // CLR is low at the start and from 6.2 to 6.6 us (between two clock edges)
    source(&mut c, "Vclr", "CLR", "PWL(0 0 0.5u 0 0.51u 5 6.2u 5 6.21u 0 6.6u 0 6.61u 5 40u 5)");
    // ENP goes low at 9.5 us and stays low
    source(&mut c, "Venp", "ENP", "PWL(0 5 9.5u 5 9.51u 0 40u 0)");
    source(&mut c, "Vent", "ENT", "5");
    source(&mut c, "Vload", "LOAD", "5");
    for p in ["A", "B", "C", "D"] {
        source(&mut c, &format!("V{p}"), p, "0");
    }
    for p in ["QA", "QB", "QC", "QD"] {
        load(&mut c, p);
    }
    let r = transient(&c, &TranSettings { tstop: 14e-6, tstep: 20e-9, uic: false }).unwrap();
    let value = |t: f64| -> usize { ["QA", "QB", "QC", "QD"].iter().enumerate().map(|(i, p)| ((at(&r, p, t) > 2.5) as usize) << i).sum() };
    assert_eq!(value(5.6e-6), 5, "the count before the clear");
    assert_eq!(value(6.5e-6), 0, "a low CLR clears at once, with no clock edge");
    assert_eq!(value(7.5e-6), 1, "and the count goes on after it");
    assert_eq!(value(9.4e-6), 3);
    assert_eq!(value(12.5e-6), 3, "ENP low holds the count");
}

/// The 74HC138 pulls the selected output low, only when G1 is high and G2A, G2B are low.
#[test]
fn the_74hc138_selects_one_output_and_obeys_its_enables() {
    for vcc in [5.0, 3.3] {
        for (g1, g2a, g2b, enabled) in [(1, 0, 0, true), (0, 0, 0, false), (1, 1, 0, false), (1, 0, 1, false)] {
            for n in 0..8usize {
                let mut c = Circuit::new();
                place(&mut c, "74HC138");
                supply(&mut c, vcc);
                for (pin, bit) in [("A", n & 1), ("B", (n >> 1) & 1), ("C", (n >> 2) & 1), ("G1", g1), ("G2A", g2a), ("G2B", g2b)] {
                    source(&mut c, &format!("V{pin}"), pin, &format!("{}", bit as f64 * vcc));
                }
                for k in 0..8 {
                    load(&mut c, &format!("Y{k}"));
                }
                let r = dc_op(&c).unwrap();
                for k in 0..8usize {
                    let v = r.voltage(&format!("p_Y{k}")).unwrap();
                    let low = enabled && k == n;
                    assert_eq!(v < vcc / 2.0, low, "74HC138 on {vcc} V, select {n}, enables {g1}{g2a}{g2b}, Y{k} = {v}");
                }
            }
        }
    }
}

/// The 74HC151 passes the selected data input to Y (and its inverse to W) when G is low; G high forces Y low and W high.
#[test]
fn the_74hc151_selects_one_data_input_and_obeys_the_strobe() {
    for vcc in [5.0, 3.3] {
        for strobe in [0, 1] {
            for sel in 0..8usize {
                for data_bit in [0usize, 1] {
                    // only the selected input carries the test value; the others carry its inverse
                    let mut c = Circuit::new();
                    place(&mut c, "74HC151");
                    supply(&mut c, vcc);
                    for (pin, bit) in [("A", sel & 1), ("B", (sel >> 1) & 1), ("C", (sel >> 2) & 1), ("G", strobe)] {
                        source(&mut c, &format!("V{pin}"), pin, &format!("{}", bit as f64 * vcc));
                    }
                    for k in 0..8usize {
                        let bit = if k == sel { data_bit } else { 1 - data_bit };
                        source(&mut c, &format!("VD{k}"), &format!("D{k}"), &format!("{}", bit as f64 * vcc));
                    }
                    load(&mut c, "Y");
                    load(&mut c, "W");
                    let r = dc_op(&c).unwrap();
                    let (y, w) = (r.voltage("p_Y").unwrap() > vcc / 2.0, r.voltage("p_W").unwrap() > vcc / 2.0);
                    let want_y = strobe == 0 && data_bit == 1;
                    assert_eq!((y, w), (want_y, !want_y), "74HC151 on {vcc} V, select {sel}, data {data_bit}, strobe {strobe}");
                }
            }
        }
    }
}
