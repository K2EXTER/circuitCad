//! Digital cells (`Y` lines, `Element::Cell`): gates and flip-flops evaluated outside the circuit equations.

use crate::*;

/// A subcircuit `CELLS` with the ports `vcc gnd` plus `ports`, whose body is `body`, placed as `U1` on nodes named like the ports.
fn place(c: &mut Circuit, ports: &str, body: &str) -> Vec<usize> {
    let names: Vec<&str> = ports.split_whitespace().collect();
    let text = format!(".subckt CELLS {ports}\n{body}\n.ends CELLS");
    let nodes: Vec<usize> = names.iter().map(|n| c.node(n)).collect();
    add_subckt_instance(c, "U1", "CELLS", &text, &nodes).unwrap_or_else(|e| panic!("{e}"));
    nodes
}

fn at(r: &TranResult, name: &str, t: f64) -> f64 {
    let k = r.time.iter().position(|x| *x >= t).unwrap();
    r.signal(name).unwrap().values[k]
}

/// NAND with an analog load: the output swings between the supply pins (a floating supply too), behind its output resistance.
#[test]
fn a_nand_cell_follows_its_supply_and_drives_a_load_through_rout() {
    for (vss, vcc) in [(0.0, 5.0), (0.0, 3.3), (-1.8, 3.0)] {
        for (a, b, want_high) in [(0.0, 0.0, true), (1.0, 0.0, true), (0.0, 1.0, true), (1.0, 1.0, false)] {
            let mut c = Circuit::new();
            let n = place(&mut c, "vcc vss a b y", "Y1 nand y 0 vss vcc 60 0.5 0 0 0 a b");
            c.add(Element::vdc("Vvss", n[1], 0, vss));
            c.add(Element::vdc("Vcc", n[0], 0, vcc));
            // the inputs relative to the supply
            c.add(Element::vdc("Va", n[2], 0, vss + a * (vcc - vss)));
            c.add(Element::vdc("Vb", n[3], 0, vss + b * (vcc - vss)));
            c.add(Element::Resistor { name: "Rl".into(), a: n[4], b: n[1], r: 1e3 });
            let r = dc_op(&c).unwrap();
            let y = r.voltage("y").unwrap();
            let want = if want_high { vss + (vcc - vss) * 1e3 / 1060.0 } else { vss };
            assert!((y - want).abs() < 0.02 * (vcc - vss), "nand({a},{b}) on {vss}..{vcc}: {y} instead of {want}");
        }
    }
}

/// A toggling flip-flop is a divide-by-two; two of them joined by a net only cells touch are a divide-by-four, and the
/// nets between them are purely digital (they are still in the result, at the supply levels).
#[test]
fn two_toggle_flip_flops_on_a_digital_net_divide_by_four() {
    let mut c = Circuit::new();
    let n = place(&mut c, "vcc vss clk q1 q2 qb1 qb2", "Y1 dff q1 qb1 vss vcc 60 0.5 0 0 0 qb1 clk 0 0\nY2 dff q2 qb2 vss vcc 60 0.5 0 0 0 qb2 q1 0 0");
    c.add(Element::vdc("Vcc", n[0], 0, 5.0));
    c.add(Element::Resistor { name: "Rg".into(), a: n[1], b: 0, r: 1e-3 });
    let s = parse_source_full("PULSE(0 5 1u 1n 1n 4u 10u)").unwrap();
    c.add(Element::VSource { name: "Vclk".into(), p: n[2], n: 0, wave: s.wave, ac: s.ac });
    // only q2 has a load: q1, qb1 and qb2 are digital nets
    c.add(Element::Resistor { name: "Rl".into(), a: n[4], b: 0, r: 10e3 });
    let r = transient(&c, &TranSettings { tstop: 85e-6, tstep: 0.1e-6, uic: false }).unwrap();
    // clock edges at 1, 11, 21, ...: q1 toggles at every edge, q2 at every rising edge of q1 (a ripple counter that counts down)
    let q1 = |t: f64| at(&r, "V(q1)", t);
    let q2 = |t: f64| at(&r, "V(q2)", t);
    for (k, (a, b)) in [(1.0, 1.0), (0.0, 1.0), (1.0, 0.0), (0.0, 0.0), (1.0, 1.0), (0.0, 1.0), (1.0, 0.0), (0.0, 0.0)].iter().enumerate() {
        let t = (k as f64) * 10e-6 + 6e-6;
        assert!((q1(t) > 2.5) == (*a > 0.5), "q1 after edge {}: {}", k + 1, q1(t));
        assert!((q2(t) > 2.5) == (*b > 0.5), "q2 after edge {}: {}", k + 1, q2(t));
    }
}


/// A run that keeps only some signals has the same time points and the same numbers for them.
#[test]
fn a_filtered_run_keeps_only_the_wanted_signals_with_the_same_values() {
    let mut c = Circuit::new();
    let n = place(&mut c, "vcc vss clk q1 q2 qb1 qb2", "Y1 dff q1 qb1 vss vcc 60 0.5 0 0 0 qb1 clk 0 0\nY2 dff q2 qb2 vss vcc 60 0.5 0 0 0 qb2 q1 0 0");
    c.add(Element::vdc("Vcc", n[0], 0, 5.0));
    c.add(Element::Resistor { name: "Rg".into(), a: n[1], b: 0, r: 1e-3 });
    let s = parse_source_full("PULSE(0 5 1u 1n 1n 4u 10u)").unwrap();
    c.add(Element::VSource { name: "Vclk".into(), p: n[2], n: 0, wave: s.wave, ac: s.ac });
    c.add(Element::Resistor { name: "Rl".into(), a: n[4], b: 0, r: 10e3 });
    let set = TranSettings { tstop: 45e-6, tstep: 0.1e-6, uic: false };
    let full = transient(&c, &set).unwrap();
    let keep = |name: &str| name == "V(q2)" || name == "I(Rl)";
    let part = transient_with(&c, &set, Some(&keep)).unwrap();
    assert_eq!(full.time, part.time);
    let names: Vec<&str> = part.signals.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["V(q2)", "I(Rl)"]);
    for s in &part.signals {
        assert_eq!(s.values, full.signal(&s.name).unwrap().values, "{}", s.name);
    }
}
