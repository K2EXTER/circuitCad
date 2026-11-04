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

// ---------------------------------------------------------------------------------------------- the bus parts (H3)

/// A pull-up resistor from a pin to the supply: a pin that lets go (Hi-Z) reads high, a pin that drives low reads low.
fn pull_up(c: &mut Circuit, pin: &str) {
    let (n, v) = (node(c, pin), node(c, "VCC"));
    c.add(Element::Resistor { name: format!("RU_{pin}"), a: n, b: v, r: 10e3 });
}

/// The 74HC595: bits shifted in by SRCLK come out at QA..QH after RCLK (QA the last bit in, QH the first), QH' follows the shift
/// register at once, SRCLR clears only the shift register, and OE high lets the outputs go.
#[test]
fn the_74hc595_shifts_latches_and_goes_hi_z() {
    let bits = [1, 0, 1, 1, 0, 0, 1, 0usize]; // the first bit in ends at QH
    let mut c = Circuit::new();
    place(&mut c, "74HC595");
    supply(&mut c, 5.0);
    // eight SRCLK pulses, rising at 1, 2, ..., 8 us (high for 0.4 us); SER changes 0.5 us before each edge
    let mut ser = String::from("PWL(0 0");
    let mut clk = String::from("PWL(0 0");
    for (k, b) in bits.iter().enumerate() {
        let edge = 1.0 + k as f64;
        ser += &format!(" {}u {} {}u {}", edge - 0.5, *b as f64 * 5.0, edge - 0.49, *b as f64 * 5.0);
        clk += &format!(" {}u 0 {}u 5 {}u 5 {}u 0", edge - 0.01, edge, edge + 0.4, edge + 0.41);
    }
    ser += " 16u 0)";
    clk += " 16u 0)";
    source(&mut c, "Vser", "SER", &ser);
    source(&mut c, "Vsrclk", "SRCLK", &clk);
    // the storage clock once, at 9.5 us; OE low until 11 us, then high; SRCLR high except for a pulse at 13 us
    source(&mut c, "Vrclk", "RCLK", "PWL(0 0 9.5u 0 9.51u 5 9.9u 5 9.91u 0 16u 0)");
    source(&mut c, "Voe", "OE", "PWL(0 0 11u 0 11.01u 5 16u 5)");
    source(&mut c, "Vclr", "SRCLR", "PWL(0 5 13u 5 13.01u 0 13.4u 0 13.41u 5 16u 5)");
    let names = ["QA", "QB", "QC", "QD", "QE", "QF", "QG", "QH"];
    for p in names {
        pull_up(&mut c, p);
    }
    load(&mut c, "QH'");
    let r = transient(&c, &TranSettings { tstop: 16e-6, tstep: 20e-9, uic: false }).unwrap();
    let level = |p: &str, t: f64| at(&r, p, t) > 2.5;
    // QH' follows the shift register: after the 8th edge (8 us) it is the first bit in; before RCLK the outputs are still the
    // power-up value 0 (they read low: driven low through 60 ohm, against the pull-up of 10 k)
    assert_eq!(level("QH'", 8.7e-6), bits[0] == 1, "QH' after eight shifts");
    for p in names {
        assert!(!level(p, 9.0e-6), "{p} is still 0 before RCLK");
    }
    // after RCLK: QA = last bit in, QH = first
    for (i, p) in names.iter().enumerate() {
        assert_eq!(level(p, 10.5e-6), bits[7 - i] == 1, "{p} after RCLK");
    }
    // OE high: every output lets go and the pull-ups win
    for p in names {
        assert!(level(p, 12.0e-6), "{p} is Hi-Z with OE high");
    }
    // SRCLR clears the shift register (QH' goes low) but not the outputs
    assert!(!level("QH'", 13.8e-6), "QH' after SRCLR");
}

/// The 74HC244: a group with its OE low passes A to Y; a group with its OE high lets go.
#[test]
fn the_74hc244_passes_a_group_and_lets_the_other_go() {
    let mut c = Circuit::new();
    place(&mut c, "74HC244");
    supply(&mut c, 5.0);
    source(&mut c, "V1oe", "1OE", "0");
    source(&mut c, "V2oe", "2OE", "5");
    // 1A: 1 0 1 0, 2A: all 0 (they would pull the outputs low if group 2 were on)
    for (k, v) in [5.0, 0.0, 5.0, 0.0].iter().enumerate() {
        source(&mut c, &format!("V1a{k}"), &format!("1A{}", k + 1), &format!("{v}"));
        source(&mut c, &format!("V2a{k}"), &format!("2A{}", k + 1), "0");
        pull_up(&mut c, &format!("1Y{}", k + 1));
        pull_up(&mut c, &format!("2Y{}", k + 1));
    }
    let r = dc_op(&c).unwrap();
    for (k, v) in [5.0, 0.0, 5.0, 0.0].iter().enumerate() {
        let y1 = r.voltage(&format!("p_1Y{}", k + 1)).unwrap();
        assert_eq!(y1 > 2.5, *v > 2.5, "1Y{} follows 1A{}: {y1}", k + 1, k + 1);
        let y2 = r.voltage(&format!("p_2Y{}", k + 1)).unwrap();
        assert!(y2 > 4.5, "2Y{} lets go (2OE high): {y2}", k + 1);
    }
}

/// The 74HC125 (OE active low) and 74HC126 (OE active high) drive only when enabled.
#[test]
fn the_74hc125_and_74hc126_have_opposite_enables() {
    for (part, enabled_by) in [("74HC125", 0.0), ("74HC126", 5.0)] {
        for oe_level in [0.0, 5.0] {
            let mut c = Circuit::new();
            place(&mut c, part);
            supply(&mut c, 5.0);
            for g in 1..=4 {
                source(&mut c, &format!("Voe{g}"), &format!("{g}OE"), &format!("{oe_level}"));
                source(&mut c, &format!("Va{g}"), &format!("{g}A"), "0");
                pull_up(&mut c, &format!("{g}Y"));
            }
            let r = dc_op(&c).unwrap();
            let y = r.voltage("p_1Y").unwrap();
            let drives = oe_level == enabled_by;
            assert_eq!(y < 2.5, drives, "{part} with OE {oe_level}: {y}");
        }
    }
}

/// The 74HC245: DIR high passes A to B, DIR low passes B to A, OE high lets both sides go. Each side is also held by a source
/// through 1 k, to see which side wins where the device drives (60 ohm).
#[test]
fn the_74hc245_passes_data_in_the_direction_of_dir_and_lets_go_with_oe() {
    let a_vals = [5.0, 0.0, 5.0, 0.0, 5.0, 0.0, 5.0, 0.0];
    let b_vals = [0.0, 5.0, 0.0, 5.0, 0.0, 5.0, 0.0, 5.0];
    let mut c = Circuit::new();
    place(&mut c, "74HC245");
    supply(&mut c, 5.0);
    // DIR high until 2 us, low from 2 to 4 us, high again; OE low except 4..6 us (high)
    source(&mut c, "Vdir", "DIR", "PWL(0 5 2u 5 2.01u 0 4u 0 4.01u 5 8u 5)");
    source(&mut c, "Voe", "OE", "PWL(0 0 4u 0 4.01u 5 6u 5 6.01u 0 8u 0)");
    for i in 0..8 {
        for (side, v) in [("A", a_vals[i]), ("B", b_vals[i])] {
            let pin = format!("{side}{}", i + 1);
            let ext = c.node(&format!("ext_{pin}"));
            let n = node(&mut c, &pin);
            c.add(Element::vdc(&format!("V{pin}"), ext, 0, v));
            c.add(Element::Resistor { name: format!("R{pin}"), a: ext, b: n, r: 1e3 });
        }
    }
    let r = transient(&c, &TranSettings { tstop: 8e-6, tstep: 20e-9, uic: false }).unwrap();
    let hi = |p: &str, t: f64| at(&r, p, t) > 2.5;
    for i in 0..8 {
        let (a, b) = (a_vals[i] > 2.5, b_vals[i] > 2.5);
        // DIR high, OE low: B takes A's value, A keeps its own
        assert_eq!(hi(&format!("B{}", i + 1), 1.5e-6), a, "B{} follows A{} (DIR high)", i + 1, i + 1);
        assert_eq!(hi(&format!("A{}", i + 1), 1.5e-6), a, "A{} keeps its value", i + 1);
        // DIR low: A takes B's value
        assert_eq!(hi(&format!("A{}", i + 1), 3.5e-6), b, "A{} follows B{} (DIR low)", i + 1, i + 1);
        assert_eq!(hi(&format!("B{}", i + 1), 3.5e-6), b, "B{} keeps its value", i + 1);
        // OE high: each side goes back to its own source
        assert_eq!(hi(&format!("A{}", i + 1), 5.5e-6), a, "A{} is alone with OE high", i + 1);
        assert_eq!(hi(&format!("B{}", i + 1), 5.5e-6), b, "B{} is alone with OE high", i + 1);
    }
}

/// The 74HC373: transparent while LE is high, holds when LE falls; OE high lets the outputs go.
#[test]
fn the_74hc373_follows_while_le_is_high_and_holds_after() {
    let mut c = Circuit::new();
    place(&mut c, "74HC373");
    supply(&mut c, 5.0);
    // LE high 1..3 us; D1 is 1 until 2 us then 0 (follows), D1 goes 1 again at 4 us (after LE fell: held)
    source(&mut c, "Vle", "LE", "PWL(0 0 1u 0 1.01u 5 3u 5 3.01u 0 8u 0)");
    source(&mut c, "Vd1", "1D", "PWL(0 5 2u 5 2.01u 0 4u 0 4.01u 5 8u 5)");
    source(&mut c, "Voe", "OE", "PWL(0 0 6u 0 6.01u 5 8u 5)");
    for i in 2..=8 {
        source(&mut c, &format!("Vd{i}"), &format!("{i}D"), "5");
    }
    for i in 1..=8 {
        pull_up(&mut c, &format!("{i}Q"));
    }
    // a load that reads low for a held 0 even when the output drives high is not needed: pull-ups show Hi-Z as high, so test 1Q at 0
    let r = transient(&c, &TranSettings { tstop: 8e-6, tstep: 20e-9, uic: false }).unwrap();
    let q1 = |t: f64| at(&r, "1Q", t);
    assert!(q1(1.8e-6) > 4.5, "follows D1 while LE is high (1): {}", q1(1.8e-6));
    assert!(q1(2.8e-6) < 0.5, "follows D1 while LE is high (0): {}", q1(2.8e-6));
    assert!(q1(5.0e-6) < 0.5, "holds the 0 after LE fell, though D1 rose at 4 us: {}", q1(5.0e-6));
    assert!(q1(7.0e-6) > 4.5, "OE high: the output lets go (the pull-up wins): {}", q1(7.0e-6));
}

/// The 74HC574: a clock edge stores D; OE high lets the outputs go; the data setup time of 20 ns is checked.
#[test]
fn the_74hc574_stores_at_the_clock_edge_and_checks_the_setup_time() {
    let mut c = Circuit::new();
    place(&mut c, "74HC574");
    supply(&mut c, 5.0);
    // clock edges at 1 us and 3 us; D1: 1 at the first edge (set at 0.5 us), 0 at the second (changes at 2.5 us)
    source(&mut c, "Vclk", "CLK", "PWL(0 0 1u 0 1.01u 5 1.4u 5 1.41u 0 3u 0 3.01u 5 3.4u 5 3.41u 0 8u 0)");
    source(&mut c, "Vd1", "1D", "PWL(0 5 2.5u 5 2.51u 0 8u 0)");
    source(&mut c, "Voe", "OE", "PWL(0 0 5u 0 5.01u 5 8u 5)");
    for i in 2..=8 {
        source(&mut c, &format!("Vd{i}"), &format!("{i}D"), "0");
    }
    for i in 1..=8 {
        pull_up(&mut c, &format!("{i}Q"));
    }
    let r = transient(&c, &TranSettings { tstop: 8e-6, tstep: 20e-9, uic: false }).unwrap();
    let q1 = |t: f64| at(&r, "1Q", t);
    assert!(q1(0.8e-6) < 0.5, "power-up value 0 (driven low): {}", q1(0.8e-6));
    assert!(q1(2.0e-6) > 4.5, "stored 1 at the first edge: {}", q1(2.0e-6));
    assert!(q1(4.0e-6) < 0.5, "stored 0 at the second edge: {}", q1(4.0e-6));
    assert!(q1(6.5e-6) > 4.5, "OE high: lets go, the pull-up wins: {}", q1(6.5e-6));
    assert!(r.timing.violations.is_empty(), "the data was stable long before the edges: {:?}", r.timing);
}

/// A data input that changes 10 ns before the clock edge breaks the 74HC574's setup time of 20 ns.
#[test]
fn the_74hc574_reports_a_data_change_just_before_the_clock() {
    let mut c = Circuit::new();
    place(&mut c, "74HC574");
    supply(&mut c, 5.0);
    source(&mut c, "Vclk", "CLK", "PWL(0 0 1u 0 1.001u 5 1.4u 5 1.401u 0 8u 0)");
    // D1 rises at 0.99 us, 10 ns before the edge at 1 us
    source(&mut c, "Vd1", "1D", "PWL(0 0 0.99u 0 0.991u 5 8u 5)");
    source(&mut c, "Voe", "OE", "0");
    for i in 2..=8 {
        source(&mut c, &format!("Vd{i}"), &format!("{i}D"), "0");
    }
    let r = transient(&c, &TranSettings { tstop: 3e-6, tstep: 5e-9, uic: false }).unwrap();
    assert!(r.timing.violations.iter().any(|v| v.kind == crate::TimingKind::Setup), "{:?}", r.timing);
    assert!(r.warnings.iter().any(|w| w.contains("セットアップ時間が足りません")), "{:?}", r.warnings);
}

// ---------------------------------------------------------------------------------------------- the rest of the first list (H2)

/// The clock rises at 0.9 us, 1.9 us, ... (high 0.4 us): the count after the j-th edge is read 0.6 us after it.
fn after_edge(j: usize) -> f64 {
    0.9e-6 + (j as f64 - 1.0) * 1e-6 + 0.6e-6
}

fn count_of(r: &TranResult, pins: &[&str], vcc: f64, t: f64) -> usize {
    pins.iter().enumerate().map(|(i, p)| ((at(r, p, t) > vcc / 2.0) as usize) << i).sum()
}

/// The 74HC163 clears at the clock edge (not at once), and counts.
#[test]
fn the_74hc163_clears_at_the_clock_edge() {
    let mut c = Circuit::new();
    place(&mut c, "74HC163");
    supply(&mut c, 5.0);
    source(&mut c, "Vclk", "CLK", "PULSE(0 5 0.9u 1n 1n 0.4u 1u)");
    // CLR low from 5.2 to 5.6 us (between two edges: nothing happens), and from 7.2 to 8.0 us (the 8th edge, at 7.9 us, clears)
    source(&mut c, "Vclr", "CLR", "PWL(0 5 5.2u 5 5.21u 0 5.6u 0 5.61u 5 7.2u 5 7.21u 0 8.0u 0 8.01u 5 40u 5)");
    source(&mut c, "Venp", "ENP", "5");
    source(&mut c, "Vent", "ENT", "5");
    source(&mut c, "Vload", "LOAD", "5");
    for p in ["A", "B", "C", "D"] {
        source(&mut c, &format!("V{p}"), p, "0");
    }
    for p in ["QA", "QB", "QC", "QD", "RCO"] {
        load(&mut c, p);
    }
    let r = transient(&c, &TranSettings { tstop: 12e-6, tstep: 20e-9, uic: false }).unwrap();
    let q = ["QA", "QB", "QC", "QD"];
    assert_eq!(count_of(&r, &q, 5.0, after_edge(1)), 1);
    assert_eq!(count_of(&r, &q, 5.0, after_edge(4)), 4);
    assert_eq!(count_of(&r, &q, 5.0, 5.5e-6), 5, "a CLR pulse between two edges changes nothing");
    assert_eq!(count_of(&r, &q, 5.0, after_edge(7)), 7);
    assert_eq!(count_of(&r, &q, 5.0, after_edge(8)), 0, "the edge with CLR low clears");
    assert_eq!(count_of(&r, &q, 5.0, after_edge(10)), 2, "and the count goes on from 0");
}

/// One 74HC393 counter counts the falling edges of CLK in binary, and CLR (high) clears it at once.
#[test]
fn the_74hc393_counts_falling_edges_and_clears() {
    let mut c = Circuit::new();
    place(&mut c, "74HC393");
    supply(&mut c, 5.0);
    // falling edges at 1.3 us, 2.3 us, ... (the clock is high 0.9..1.3 us): the count after the j-th falling edge is j
    source(&mut c, "Vclk", "1CLK", "PULSE(0 5 0.9u 1n 1n 0.4u 1u)");
    source(&mut c, "Vclr", "1CLR", "PWL(0 0 6.5u 0 6.51u 5 6.9u 5 6.91u 0 20u 0)");
    source(&mut c, "Vclk2", "2CLK", "0");
    source(&mut c, "Vclr2", "2CLR", "0");
    for p in ["1QA", "1QB", "1QC", "1QD", "2QA"] {
        load(&mut c, p);
    }
    let r = transient(&c, &TranSettings { tstop: 14e-6, tstep: 20e-9, uic: false }).unwrap();
    let q = ["1QA", "1QB", "1QC", "1QD"];
    for j in 1..=5usize {
        let t = 1.3e-6 + (j as f64 - 1.0) * 1e-6 + 0.6e-6;
        assert_eq!(count_of(&r, &q, 5.0, t), j, "after {j} falling edges");
    }
    assert_eq!(count_of(&r, &q, 5.0, 6.7e-6), 0, "CLR high clears between edges");
    assert_eq!(count_of(&r, &q, 5.0, 8.0e-6), 1, "the count goes on: the first falling edge after CLR is at 7.3 us");
    assert_eq!(count_of(&r, &q, 5.0, 9.0e-6), 2, "and the next one at 8.3 us");
}

/// The 74HC139: one output of a decoder goes low; G high turns the decoder off; the two decoders are independent.
#[test]
fn the_74hc139_decodes_two_bits() {
    for vcc in [5.0, 3.3] {
        for n in 0..4usize {
            for (g, enabled) in [(0, true), (1, false)] {
                let mut c = Circuit::new();
                place(&mut c, "74HC139");
                supply(&mut c, vcc);
                // decoder 1 gets the test code; decoder 2 gets the opposite code and is always enabled
                for (pin, bit) in [("1A", n & 1), ("1B", n >> 1), ("1G", g), ("2A", (3 - n) & 1), ("2B", (3 - n) >> 1), ("2G", 0)] {
                    source(&mut c, &format!("V{pin}"), pin, &format!("{}", bit as f64 * vcc));
                }
                for k in 0..4 {
                    load(&mut c, &format!("1Y{k}"));
                    load(&mut c, &format!("2Y{k}"));
                }
                let r = dc_op(&c).unwrap();
                for k in 0..4usize {
                    let y1 = r.voltage(&format!("p_1Y{k}")).unwrap() < vcc / 2.0;
                    assert_eq!(y1, enabled && k == n, "74HC139 on {vcc} V, 1Y{k}, code {n}, G {g}");
                    let y2 = r.voltage(&format!("p_2Y{k}")).unwrap() < vcc / 2.0;
                    assert_eq!(y2, k == 3 - n, "2Y{k} follows its own code");
                }
            }
        }
    }
}

/// The 74HC153 passes C0..C3 of each half by the shared select (A, B); G high forces Y low.
#[test]
fn the_74hc153_selects_one_of_four_per_half() {
    for vcc in [5.0, 3.3] {
        for sel in 0..4usize {
            for (g, data_bit) in [(0, 0usize), (0, 1), (1, 1)] {
                let mut c = Circuit::new();
                place(&mut c, "74HC153");
                supply(&mut c, vcc);
                for (pin, bit) in [("A", sel & 1), ("B", sel >> 1), ("1G", g), ("2G", 0)] {
                    source(&mut c, &format!("V{pin}"), pin, &format!("{}", bit as f64 * vcc));
                }
                for k in 0..4usize {
                    // the selected input carries the test value in half 1 and its inverse in half 2; the others carry the opposite
                    let b1 = if k == sel { data_bit } else { 1 - data_bit };
                    let b2 = if k == sel { 1 - data_bit } else { data_bit };
                    source(&mut c, &format!("V1C{k}"), &format!("1C{k}"), &format!("{}", b1 as f64 * vcc));
                    source(&mut c, &format!("V2C{k}"), &format!("2C{k}"), &format!("{}", b2 as f64 * vcc));
                }
                load(&mut c, "1Y");
                load(&mut c, "2Y");
                let r = dc_op(&c).unwrap();
                let (y1, y2) = (r.voltage("p_1Y").unwrap() > vcc / 2.0, r.voltage("p_2Y").unwrap() > vcc / 2.0);
                assert_eq!(y1, g == 0 && data_bit == 1, "74HC153 on {vcc} V, half 1, select {sel}, data {data_bit}, G {g}");
                assert_eq!(y2, g == 0 && data_bit == 0, "half 2 (G low), select {sel}, data {data_bit}");
            }
        }
    }
}

/// The 74HC157: A/B picks the A or the B inputs; G high forces every Y low.
#[test]
fn the_74hc157_selects_a_or_b_for_four_channels() {
    for vcc in [5.0, 3.3] {
        for sel in [0, 1] {
            for g in [0, 1] {
                let mut c = Circuit::new();
                place(&mut c, "74HC157");
                supply(&mut c, vcc);
                for (pin, bit) in [("A/B", sel), ("G", g)] {
                    source(&mut c, &format!("V{}", pin.replace('/', "_")), pin, &format!("{}", bit as f64 * vcc));
                }
                // channel k: A = bit k of 0b0101, B = its inverse
                for k in 1..=4usize {
                    let a = (k + 1) % 2; // 0,1,0,1 for k = 1..4... a(1)=0 a(2)=1 a(3)=0 a(4)=1
                    source(&mut c, &format!("V{k}A"), &format!("{k}A"), &format!("{}", a as f64 * vcc));
                    source(&mut c, &format!("V{k}B"), &format!("{k}B"), &format!("{}", (1 - a) as f64 * vcc));
                    load(&mut c, &format!("{k}Y"));
                }
                let r = dc_op(&c).unwrap();
                for k in 1..=4usize {
                    let a = (k + 1) % 2;
                    let want = g == 0 && (if sel == 0 { a } else { 1 - a }) == 1;
                    assert_eq!(r.voltage(&format!("p_{k}Y")).unwrap() > vcc / 2.0, want, "74HC157 on {vcc} V, channel {k}, A/B {sel}, G {g}");
                }
            }
        }
    }
}

/// The 74HC164 shifts A AND B in at each rising clock edge; CLR clears at once.
#[test]
fn the_74hc164_shifts_the_and_of_its_two_inputs() {
    let mut c = Circuit::new();
    place(&mut c, "74HC164");
    supply(&mut c, 5.0);
    source(&mut c, "Vclk", "CLK", "PULSE(0 5 0.9u 1n 1n 0.4u 1u)");
    // A is 1 throughout; B is 1 for the first three edges (until 3.5 us), then 0
    source(&mut c, "Va", "A", "5");
    source(&mut c, "Vb", "B", "PWL(0 5 3.5u 5 3.51u 0 20u 0)");
    source(&mut c, "Vclr", "CLR", "PWL(0 0 0.3u 0 0.31u 5 9.5u 5 9.51u 0 10.4u 0 10.41u 5 20u 5)");
    for p in ["QA", "QB", "QC", "QD", "QE", "QF", "QG", "QH"] {
        load(&mut c, p);
    }
    let r = transient(&c, &TranSettings { tstop: 12e-6, tstep: 20e-9, uic: false }).unwrap();
    let q = ["QA", "QB", "QC", "QD", "QE", "QF", "QG", "QH"];
    // three 1s go in, then 0s: after j edges the lowest min(j,3) outputs are shifted up: j = 1: QA; 2: QA QB; 3: QA QB QC; 4: QB QC QD; ...
    assert_eq!(count_of(&r, &q, 5.0, after_edge(1)), 0b0000_0001);
    assert_eq!(count_of(&r, &q, 5.0, after_edge(3)), 0b0000_0111);
    assert_eq!(count_of(&r, &q, 5.0, after_edge(4)), 0b0000_1110);
    assert_eq!(count_of(&r, &q, 5.0, after_edge(6)), 0b0011_1000);
    assert_eq!(count_of(&r, &q, 5.0, 9.4e-6), 0b1100_0000, "after nine edges the first 1 has gone out through QH; the other two are at QG and QH");
    assert_eq!(count_of(&r, &q, 5.0, 9.7e-6), 0, "CLR low clears at once, between edges");
    assert_eq!(count_of(&r, &q, 5.0, after_edge(10)), 0, "the edge at 9.9 us finds CLR low: nothing is shifted in");
}

/// The 74HC165 loads A..H at once with SH/LD low (asynchronously) and shifts them out through QH.
#[test]
fn the_74hc165_loads_in_parallel_and_shifts_out_serially() {
    let bits = [1, 0, 1, 1, 0, 0, 1, 0usize]; // A..H
    let mut c = Circuit::new();
    place(&mut c, "74HC165");
    supply(&mut c, 5.0);
    // SH/LD low from 0.3 to 1.2 us (loads between the clock edges: asynchronous), then high; the clock rises at 1.9, 2.9, ... us
    source(&mut c, "Vshld", "SH/LD", "PWL(0 5 0.3u 5 0.31u 0 1.2u 0 1.21u 5 20u 5)");
    source(&mut c, "Vclk", "CLK", "PULSE(0 5 1.9u 1n 1n 0.4u 1u)");
    source(&mut c, "Vinh", "CLK_INH", "0");
    source(&mut c, "Vser", "SER", "0");
    for (i, p) in ["A", "B", "C", "D", "E", "F", "G", "H"].iter().enumerate() {
        source(&mut c, &format!("V{p}"), p, &format!("{}", bits[i] as f64 * 5.0));
    }
    load(&mut c, "QH");
    load(&mut c, "QH_N");
    let r = transient(&c, &TranSettings { tstop: 12e-6, tstep: 20e-9, uic: false }).unwrap();
    let qh = |t: f64| at(&r, "QH", t) > 2.5;
    // after the load (1.0 us) QH is H (the last bit); each rising edge then brings the next bit towards QH: after edge 1 it is G...
    assert_eq!(qh(1.0e-6), bits[7] == 1, "QH shows H right after the load");
    for j in 1..=7usize {
        let t = 1.9e-6 + (j as f64 - 1.0) * 1e-6 + 0.6e-6;
        assert_eq!(qh(t), bits[7 - j] == 1, "QH after {j} clock edges");
    }
    assert_eq!(at(&r, "QH_N", 1.0e-6) > 2.5, bits[7] != 1, "QH_N is the inverse");
}

/// The 74HC283 adds two 4-bit numbers and a carry in.
#[test]
fn the_74hc283_adds() {
    for vcc in [5.0, 3.3] {
        for (a, b, cin) in [(0usize, 0usize, 0usize), (5, 3, 0), (9, 6, 1), (15, 15, 1), (7, 8, 0), (12, 4, 0), (1, 14, 1)] {
            let mut c = Circuit::new();
            place(&mut c, "74HC283");
            supply(&mut c, vcc);
            for i in 0..4 {
                source(&mut c, &format!("VA{i}"), &format!("A{i}"), &format!("{}", ((a >> i) & 1) as f64 * vcc));
                source(&mut c, &format!("VB{i}"), &format!("B{i}"), &format!("{}", ((b >> i) & 1) as f64 * vcc));
                load(&mut c, &format!("S{i}"));
            }
            source(&mut c, "VCIN", "CIN", &format!("{}", cin as f64 * vcc));
            load(&mut c, "COUT");
            let r = dc_op(&c).unwrap();
            let mut sum = 0usize;
            for i in 0..4 {
                sum |= ((r.voltage(&format!("p_S{i}")).unwrap() > vcc / 2.0) as usize) << i;
            }
            sum |= ((r.voltage("p_COUT").unwrap() > vcc / 2.0) as usize) << 4;
            assert_eq!(sum, a + b + cin, "74HC283 on {vcc} V: {a} + {b} + {cin}");
        }
    }
}

/// The 74HC4040 divides by two at each stage and counts the falling edges of CP in binary up to 4095; MR clears.
#[test]
fn the_74hc4040_is_a_twelve_stage_ripple_counter() {
    let mut c = Circuit::new();
    place(&mut c, "74HC4040");
    supply(&mut c, 5.0);
    source(&mut c, "Vcp", "CP", "PULSE(0 5 0.9u 1n 1n 0.4u 1u)");
    source(&mut c, "Vmr", "MR", "PWL(0 0 9.5u 0 9.51u 5 9.9u 5 9.91u 0 40u 0)");
    let q: Vec<String> = (1..=12).map(|k| format!("Q{k}")).collect();
    for p in &q {
        load(&mut c, p);
    }
    let r = transient(&c, &TranSettings { tstop: 14e-6, tstep: 20e-9, uic: false }).unwrap();
    let pins: Vec<&str> = q.iter().map(|s| s.as_str()).collect();
    for j in 1..=8usize {
        // the falling edge of CP number j is at 1.3 + (j - 1) us; the count settles within about 50 ns of it
        let t = 1.3e-6 + (j as f64 - 1.0) * 1e-6 + 0.6e-6;
        assert_eq!(count_of(&r, &pins, 5.0, t), j, "after {j} falling edges");
    }
    assert_eq!(count_of(&r, &pins, 5.0, 9.7e-6), 0, "MR high clears at once");
    assert_eq!(count_of(&r, &pins, 5.0, 11.6e-6), 2, "and the count goes on (the falling edges at 10.3 us and 11.3 us)");
}

/// The CD4511B: the codes 0..9 light the segments of the data sheet, LT lights all, BL blanks, LE holds the code, codes above 9 are blank.
#[test]
fn the_cd4511b_decodes_bcd_to_seven_segments() {
    // a b c d e f g for each digit (the data sheet truth table)
    let table = ["abcdef", "bc", "abdeg", "abcdg", "bcfg", "acdfg", "cdefg", "abc", "abcdefg", "abcfg"];
    for vdd in [5.0, 10.0] {
        let run = |code: usize, lt: f64, bl: f64, le: f64| {
            let mut c = Circuit::new();
            place(&mut c, "CD4511B");
            let (v, g) = (node(&mut c, "VDD"), node(&mut c, "VSS"));
            c.add(Element::vdc("Vdd", v, 0, vdd));
            c.add(Element::Resistor { name: "Rg".into(), a: g, b: 0, r: 1e-3 });
            for (pin, bit) in [("A", code & 1), ("B", (code >> 1) & 1), ("C", (code >> 2) & 1), ("D", (code >> 3) & 1)] {
                source(&mut c, &format!("V{pin}"), pin, &format!("{}", bit as f64 * vdd));
            }
            source(&mut c, "Vlt", "LT", &format!("{}", lt * vdd));
            source(&mut c, "Vbl", "BL", &format!("{}", bl * vdd));
            source(&mut c, "Vle", "LE", &format!("{}", le * vdd));
            for s in "ABCDEFG".chars() {
                load(&mut c, &format!("SEG_{s}"));
            }
            let r = dc_op(&c).unwrap();
            "ABCDEFG".chars().map(|s| r.voltage(&format!("p_SEG_{s}")).unwrap() > vdd / 2.0).collect::<Vec<bool>>()
        };
        for (digit, segs) in table.iter().enumerate() {
            let want: Vec<bool> = "abcdefg".chars().map(|s| segs.contains(s)).collect();
            assert_eq!(run(digit, 1.0, 1.0, 0.0), want, "digit {digit} on {vdd} V");
        }
        for code in 10..16 {
            assert_eq!(run(code, 1.0, 1.0, 0.0), vec![false; 7], "code {code} is blank");
        }
        assert_eq!(run(3, 0.0, 1.0, 0.0), vec![true; 7], "lamp test lights everything");
        assert_eq!(run(3, 0.0, 0.0, 0.0), vec![true; 7], "lamp test wins over blanking");
        assert_eq!(run(3, 1.0, 0.0, 0.0), vec![false; 7], "BL low blanks");
    }
}

/// With LE high the CD4511B keeps the code it had when LE rose.
#[test]
fn the_cd4511b_latches_the_code_with_le() {
    let mut c = Circuit::new();
    place(&mut c, "CD4511B");
    let (v, g) = (node(&mut c, "VDD"), node(&mut c, "VSS"));
    c.add(Element::vdc("Vdd", v, 0, 5.0));
    c.add(Element::Resistor { name: "Rg".into(), a: g, b: 0, r: 1e-3 });
    // the code is 1 (A) until 5 us, then 2 (B); LE rises at 3 us: it holds 1; LE falls at 8 us: it follows 2
    source(&mut c, "VA", "A", "PWL(0 5 5u 5 5.01u 0 20u 0)");
    source(&mut c, "VB", "B", "PWL(0 0 5u 0 5.01u 5 20u 5)");
    source(&mut c, "VC", "C", "0");
    source(&mut c, "VD", "D", "0");
    source(&mut c, "Vlt", "LT", "5");
    source(&mut c, "Vbl", "BL", "5");
    source(&mut c, "Vle", "LE", "PWL(0 0 3u 0 3.01u 5 8u 5 8.01u 0 20u 0)");
    for s in "ABCDEFG".chars() {
        load(&mut c, &format!("SEG_{s}"));
    }
    let r = transient(&c, &TranSettings { tstop: 12e-6, tstep: 50e-9, uic: false }).unwrap();
    let seg = |t: f64| -> String { "ABCDEFG".chars().filter(|s| at(&r, &format!("SEG_{s}"), t) > 2.5).collect() };
    assert_eq!(seg(2.5e-6), "BC", "code 1 shows b and c (the delay is 0.7 us at 5 V)");
    assert_eq!(seg(7.0e-6), "BC", "LE high holds the 1 after the code changed to 2");
    assert_eq!(seg(11.0e-6), "ABDEG", "LE low: it follows the code 2");
}

/// The ROM32x8 holds one sine period; an R-2R ladder on D0..D7 turns each word into a voltage of word / 256 of the supply.
#[test]
fn the_rom32x8_through_an_r_2r_ladder_gives_a_sine() {
    for addr in 0..32usize {
        let mut c = Circuit::new();
        place(&mut c, "ROM32x8");
        supply(&mut c, 5.0);
        source(&mut c, "Voe", "OE", "0");
        for k in 0..5 {
            source(&mut c, &format!("Va{k}"), &format!("A{k}"), if addr >> k & 1 == 1 { "5" } else { "0" });
        }
        let nodes: Vec<usize> = (0..8).map(|k| c.node(&format!("ladder{k}"))).collect();
        for k in 0..8 {
            let d = node(&mut c, &format!("D{k}"));
            c.add(Element::Resistor { name: format!("R2_{k}"), a: d, b: nodes[k], r: 20e3 });
            if k + 1 < 8 {
                c.add(Element::Resistor { name: format!("R1_{k}"), a: nodes[k], b: nodes[k + 1], r: 10e3 });
            }
        }
        c.add(Element::Resistor { name: "Rterm".into(), a: nodes[0], b: 0, r: 20e3 });
        let r = dc_op(&c).unwrap();
        let want = (127.5 + 127.5 * (2.0 * std::f64::consts::PI * addr as f64 / 32.0).sin()).round() / 256.0 * 5.0;
        let v = r.voltage("ladder7").unwrap();
        assert!((v - want).abs() < 0.03, "address {addr}: {v} V instead of {want} V");
    }
}

/// A high OE lets go of the data outputs.
#[test]
fn the_rom32x8_outputs_are_off_when_oe_is_high() {
    let mut c = Circuit::new();
    place(&mut c, "ROM32x8");
    supply(&mut c, 5.0);
    source(&mut c, "Voe", "OE", "5");
    for k in 0..5 {
        source(&mut c, &format!("Va{k}"), &format!("A{k}"), "5");
    }
    // address 31 holds 103 (0b01100111): D0 would be high; pulled down it stays low when the ROM is off
    let d0 = node(&mut c, "D0");
    c.add(Element::Resistor { name: "Rpd".into(), a: d0, b: 0, r: 10e3 });
    let r = dc_op(&c).unwrap();
    assert!(r.voltage("p_D0").unwrap() < 0.5, "{}", r.voltage("p_D0").unwrap());
}

/// The 4-bit flash converter reads floor(16 * VIN / supply) at each rising clock edge, on 5 V and on 3.3 V.
#[test]
fn the_adc4_flash_reads_floor_of_sixteen_times_the_input_over_the_supply() {
    for vcc in [5.0, 3.3] {
        let mut c = Circuit::new();
        place(&mut c, "ADC4_FLASH");
        supply(&mut c, vcc);
        // a staircase through the middle of every code, 16 steps of 2 us; the clock rises 1.5 us into each step
        let mut pwl = String::from("PWL(0 0");
        for j in 0..16 {
            let (t0, v) = (j as f64 * 2e-6, (j as f64 + 0.5) / 16.0 * vcc);
            pwl += &format!(" {} {} {} {}", t0 + 0.1e-6, v, t0 + 2e-6, v);
        }
        pwl.push(')');
        source(&mut c, "Vin", "AIN", &pwl);
        source(&mut c, "Vclk", "CLK", &format!("PULSE(0 {vcc} 1.5u 1n 1n 0.3u 2u)"));
        for p in ["D0", "D1", "D2", "D3"] {
            load(&mut c, p);
        }
        let r = transient(&c, &TranSettings { tstop: 33e-6, tstep: 20e-9, uic: false }).unwrap();
        for j in 0..16usize {
            let t = j as f64 * 2e-6 + 1.9e-6;
            let code: usize = (0..4).map(|k| usize::from(at(&r, &format!("D{k}"), t) > vcc / 2.0) << k).sum();
            assert_eq!(code, j, "step {j} on {vcc} V");
        }
    }
}
