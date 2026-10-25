//! The 74HC logic ICs: switch-level models of CMOS gates.

use crate::*;

fn part_text(ic: &str) -> String {
    let json = match ic {
        "74HC00" => include_str!("../../part-lib/examples/74HC00_DIP-14.part.json"),
        "74HC02" => include_str!("../../part-lib/examples/74HC02_DIP-14.part.json"),
        "74HC04" => include_str!("../../part-lib/examples/74HC04_DIP-14.part.json"),
        "74HC08" => include_str!("../../part-lib/examples/74HC08_DIP-14.part.json"),
        "74HC14" => include_str!("../../part-lib/examples/74HC14_DIP-14.part.json"),
        "74HC32" => include_str!("../../part-lib/examples/74HC32_DIP-14.part.json"),
        "74HC74" => include_str!("../../part-lib/examples/74HC74_DIP-14.part.json"),
        "74HC86" => include_str!("../../part-lib/examples/74HC86_DIP-14.part.json"),
        other => panic!("{other}"),
    };
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    v["sim"]["text"].as_str().unwrap().to_string()
}

/// `U1` with pin number k on node `p{k}`; returns the nodes (index 0 = pin 1).
fn place(c: &mut Circuit, ic: &str) -> Vec<usize> {
    let nodes: Vec<usize> = (1..=14).map(|k| c.node(&format!("p{k}"))).collect();
    add_subckt_instance(c, "U1", ic, &part_text(ic), &nodes).unwrap_or_else(|e| panic!("{ic}: {e}"));
    nodes
}

fn supply(c: &mut Circuit, nodes: &[usize], vcc: f64) {
    c.add(Element::vdc("Vcc", nodes[13], 0, vcc));
    // pin 7 is ground
    c.add(Element::Resistor { name: "Rg".into(), a: nodes[6], b: 0, r: 1e-3 });
}

fn drive(c: &mut Circuit, name: &str, node: usize, v: f64) {
    c.add(Element::vdc(name, node, 0, v));
}

fn level(r: &DcResult, pin: usize, vcc: f64) -> Option<bool> {
    let v = r.voltage(&format!("p{pin}"))?;
    if v > 0.9 * vcc {
        Some(true)
    } else if v < 0.1 * vcc {
        Some(false)
    } else {
        None
    }
}

/// (input pin A, input pin B, output pin) of the four gates of a quad 2-input IC.
const STD: [(usize, usize, usize); 4] = [(1, 2, 3), (4, 5, 6), (9, 10, 8), (12, 13, 11)];
const NOR: [(usize, usize, usize); 4] = [(2, 3, 1), (5, 6, 4), (8, 9, 10), (11, 12, 13)];

fn truth_table(ic: &str, gates: &[(usize, usize, usize); 4], f: impl Fn(bool, bool) -> bool, vcc: f64) {
    for (a, b) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut c = Circuit::new();
        let n = place(&mut c, ic);
        supply(&mut c, &n, vcc);
        for (k, (pa, pb, _)) in gates.iter().enumerate() {
            drive(&mut c, &format!("A{k}"), n[pa - 1], if a { vcc } else { 0.0 });
            drive(&mut c, &format!("B{k}"), n[pb - 1], if b { vcc } else { 0.0 });
        }
        let r = dc_op(&c).unwrap_or_else(|e| panic!("{ic} {a}{b}: {e}"));
        for (k, (_, _, y)) in gates.iter().enumerate() {
            assert_eq!(level(&r, *y, vcc), Some(f(a, b)), "{ic} gate {} with A={a} B={b} at {vcc} V: out = {:?}", k + 1, r.voltage(&format!("p{y}")));
        }
    }
}

#[test]
fn two_input_gates_follow_their_truth_tables() {
    for vcc in [5.0, 3.3] {
        truth_table("74HC00", &STD, |a, b| !(a && b), vcc);
        truth_table("74HC08", &STD, |a, b| a && b, vcc);
        truth_table("74HC32", &STD, |a, b| a || b, vcc);
        truth_table("74HC86", &STD, |a, b| a != b, vcc);
        truth_table("74HC02", &NOR, |a, b| !(a || b), vcc);
    }
}

#[test]
fn inverters_invert_and_a_floating_input_reads_low() {
    for ic in ["74HC04", "74HC14"] {
        let ins = [1, 3, 5, 9, 11, 13];
        let outs = [2, 4, 6, 8, 10, 12];
        for high in [false, true] {
            let mut c = Circuit::new();
            let n = place(&mut c, ic);
            supply(&mut c, &n, 5.0);
            for (k, p) in ins.iter().enumerate() {
                drive(&mut c, &format!("A{k}"), n[p - 1], if high { 5.0 } else { 0.0 });
            }
            let r = dc_op(&c).unwrap();
            for y in outs {
                assert_eq!(level(&r, y, 5.0), Some(!high), "{ic} pin {y}, input high = {high}");
            }
        }
    }
}

#[test]
fn output_drive_is_a_few_tens_of_ohms() {
    let mut c = Circuit::new();
    let n = place(&mut c, "74HC04");
    supply(&mut c, &n, 5.0);
    drive(&mut c, "A", n[0], 0.0); // output 1Y high
    c.add(Element::Resistor { name: "Rl".into(), a: n[1], b: 0, r: 1e3 });
    let r = dc_op(&c).unwrap();
    let v = r.voltage("p2").unwrap();
    // 80 ohm high-side switch against a 1k load
    assert!((v - 5.0 * 1e3 / 1080.0).abs() < 0.03, "{v}");
    // and sinking: input high, load to the supply
    let mut c = Circuit::new();
    let n = place(&mut c, "74HC04");
    supply(&mut c, &n, 5.0);
    drive(&mut c, "A", n[0], 5.0);
    c.add(Element::Resistor { name: "Rl".into(), a: n[1], b: n[13], r: 1e3 });
    let v = dc_op(&c).unwrap().voltage("p2").unwrap();
    assert!((v - 5.0 * 40.0 / 1040.0).abs() < 0.03, "{v}");
}

#[test]
fn inputs_switch_at_half_the_supply_for_hc04_and_with_hysteresis_for_hc14() {
    let thresholds = |ic: &str| -> (f64, f64) {
        let mut c = Circuit::new();
        let n = place(&mut c, ic);
        supply(&mut c, &n, 5.0);
        let ramp = parse_source_full("PWL(0 0 1 5 2 0)").unwrap();
        c.add(Element::VSource { name: "Vin".into(), p: n[0], n: 0, wave: ramp.wave, ac: ramp.ac });
        let r = transient(&c, &TranSettings { tstop: 2.0, tstep: 2e-3, uic: false }).unwrap();
        let (vin, out) = (r.signal("V(p1)").unwrap(), r.signal("V(p2)").unwrap());
        let (mut up, mut down) = (f64::NAN, f64::NAN);
        for k in 1..out.values.len() {
            if out.values[k - 1] > 2.5 && out.values[k] <= 2.5 && up.is_nan() {
                up = vin.values[k];
            }
            if out.values[k - 1] < 2.5 && out.values[k] >= 2.5 && !up.is_nan() {
                down = vin.values[k];
            }
        }
        (up, down)
    };
    let (up, down) = thresholds("74HC04");
    assert!((up - 2.5).abs() < 0.12 && (down - 2.5).abs() < 0.12, "74HC04: {up} {down}");
    let (up, down) = thresholds("74HC14");
    assert!((up - 3.0).abs() < 0.15, "rising threshold {up}");
    assert!((down - 2.0).abs() < 0.15, "falling threshold {down}");
    assert!(up - down > 0.8, "hysteresis {}", up - down);
}

/// One 74HC74 flip-flop, rising-edge clock at t = 1, 3, 5; D goes 1 -> 0 at 1.5 (clock high),
/// back to 1 at 3.5 (clock high); asynchronous clear 0..0.5 and a preset pulse 5.6..5.9.
fn dff_run() -> TranResult {
    let mut c = Circuit::new();
    let n = place(&mut c, "74HC74");
    supply(&mut c, &n, 5.0);
    let mut pwl = |name: &str, node: usize, text: &str| {
        let s = parse_source_full(text).unwrap();
        c.add(Element::VSource { name: name.into(), p: node, n: 0, wave: s.wave, ac: s.ac });
    };
    pwl("Vclk", n[2], "PWL(0 0 1 0 1.01 5 2 5 2.01 0 3 0 3.01 5 4 5 4.01 0 5 0 5.01 5 5.5 5 5.51 0 7 0)");
    pwl("Vd", n[1], "PWL(0 5 1.5 5 1.51 0 3.5 0 3.51 5 7 5)");
    pwl("Vclr", n[0], "PWL(0 0 0.5 0 0.51 5 7 5)");
    pwl("Vpre", n[3], "PWL(0 5 5.6 5 5.61 0 5.9 0 5.91 5 7 5)");
    transient(&c, &TranSettings { tstop: 6.5, tstep: 5e-3, uic: false }).unwrap()
}

fn at(r: &TranResult, name: &str, t: f64) -> f64 {
    let k = r.time.iter().position(|x| *x >= t).unwrap();
    r.signal(name).unwrap().values[k]
}

#[test]
fn d_flip_flop_samples_d_on_the_rising_edge_only() {
    let r = dff_run();
    let q = |t: f64| at(&r, "V(p5)", t) > 2.5;
    let qb = |t: f64| at(&r, "V(p6)", t) > 2.5;
    assert!(!q(0.4) && qb(0.4), "asynchronous clear");
    assert!(!q(0.9), "stays cleared until the first edge (D = 1 but no clock)");
    assert!(q(1.2) && !qb(1.2), "edge at t=1 with D=1");
    assert!(q(1.8), "D fell while the clock was high: no change");
    assert!(q(2.5), "falling clock edge: no change");
    assert!(!q(3.2) && qb(3.2), "edge at t=3 with D=0");
    assert!(!q(4.2), "D rose while the clock was high: no change");
    assert!(q(5.2), "edge at t=5 with D=1");
    assert!(q(5.45) && q(6.2));
    // levels are clean
    assert!(at(&r, "V(p5)", 1.2) > 4.5 && at(&r, "V(p6)", 1.2) < 0.5);
}

#[test]
fn preset_wins_over_the_clock_data() {
    let mut c = Circuit::new();
    let n = place(&mut c, "74HC74");
    supply(&mut c, &n, 5.0);
    // Q starts low (clear), then preset forces it high with D = 0 and the clock idle
    for (name, node, text) in [("Vd", n[1], "0"), ("Vclk", n[2], "0"), ("Vclr", n[0], "PWL(0 0 0.2 0 0.21 5 3 5)"), ("Vpre", n[3], "PWL(0 5 1 5 1.01 0 1.5 0 1.51 5 3 5)")] {
        let s = parse_source_full(text).unwrap();
        c.add(Element::VSource { name: name.into(), p: node, n: 0, wave: s.wave, ac: s.ac });
    }
    let r = transient(&c, &TranSettings { tstop: 3.0, tstep: 5e-3, uic: false }).unwrap();
    assert!(at(&r, "V(p5)", 0.5) < 0.5);
    assert!(at(&r, "V(p5)", 1.25) > 4.5, "preset");
    assert!(at(&r, "V(p5)", 2.5) > 4.5, "and it stays set after the preset is released");
}

#[test]
fn all_logic_parts_load() {
    for ic in ["74HC00", "74HC02", "74HC04", "74HC08", "74HC14", "74HC32", "74HC74", "74HC86"] {
        let mut c = Circuit::new();
        place(&mut c, ic);
        assert!(c.elements.iter().any(|e| matches!(e, Element::Switch { .. })), "{ic}");
    }
}

#[test]
fn logic_blinker_example_divides_the_oscillator_by_two_and_alternates_the_leds() {
    let sch: sch_core::Schematic = serde_json::from_str(include_str!("../../../docs/examples/logic_blinker.schjson")).unwrap();
    let c = from_schematic(&sch).unwrap();
    let r = transient(&c, &TranSettings { tstop: 4.0, tstep: 2e-3, uic: false }).unwrap();
    let edges = |name: &str, level: f64| {
        let s = r.signal(name).unwrap();
        s.values.windows(2).filter(|w| (w[0] > level) != (w[1] > level)).count()
    };
    // the Schmitt oscillator: T = RC (ln 1.5 + ln 1.5) = 0.81 * 0.47 s = 0.38 s -> about 10 cycles in 4 s
    let osc = edges("V(OSC)", 2.5);
    assert!((17..=23).contains(&osc), "{osc} edges on OSC (two per cycle)");
    // the flip-flop output toggles once per clock cycle: half as many edges as the oscillator
    let q = edges("V(Q1)", 2.5);
    assert!((9..=12).contains(&q), "{q} edges on Q1");
    // Q and Q-bar are complementary
    let (a, b) = (r.signal("V(Q1)").unwrap(), r.signal("V(QB1)").unwrap());
    let agree = a.values.iter().zip(&b.values).filter(|(x, y)| (**x > 2.5) == (**y > 2.5)).count();
    assert!(agree < a.values.len() / 20, "{agree} samples where Q equals Q-bar");
    // the LED current is a few milliamps while lit
    let i = r.signal("I(D1)").unwrap().values.iter().fold(0.0f64, |m, v| m.max(*v));
    assert!(i > 2e-3 && i < 8e-3, "{i}");
}

#[test]
fn a_flip_flop_circuit_says_its_dc_answer_is_one_state_and_the_start_state_picks_another() {
    let mut c = Circuit::new();
    let (d, clk, q, qb) = (c.node("d"), c.node("clk"), c.node("q"), c.node("qb"));
    let levels = crate::SeqLevels { vlow: 0.0, vhigh: 5.0, vt: 2.5, vh: 0.0, set_low: false, reset_low: false };
    c.add(Element::Seq { name: "FF1".into(), kind: crate::SeqKind::Dff, ins: vec![d, clk, 0, 0], q, qb, levels });
    for (name, node) in [("Rd", d), ("Rclk", clk)] {
        c.add(Element::Resistor { name: name.into(), a: node, b: 0, r: 1e3 });
    }
    let r = dc_op(&c).unwrap();
    assert_eq!(r.seq_states, vec![("FF1".to_string(), false)], "a stored state starts at 0");
    assert!(r.warnings.iter().any(|w| w.contains("記憶素子")), "{:?}", r.warnings);
    assert!(r.voltage("q").unwrap() < 0.1);
    c.set_initial_voltage("FF1", 1.0);
    let r1 = dc_op(&c).unwrap();
    assert_eq!(r1.seq_states, vec![("FF1".to_string(), true)], "the circuit's initial state is the stored state");
    assert!((r1.voltage("q").unwrap() - 5.0).abs() < 0.1);
}
