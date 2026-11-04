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

fn edge_time(r: &TranResult, name: &str, level: f64, rising: bool, after: f64) -> Option<f64> {
    let s = r.signal(name)?;
    for k in 1..r.time.len() {
        if r.time[k - 1] < after {
            continue;
        }
        let (a, b) = (s.values[k - 1], s.values[k]);
        if rising && a < level && b >= level || !rising && a > level && b <= level {
            // linear interpolation between the two points
            return Some(r.time[k - 1] + (level - a) / (b - a) * (r.time[k] - r.time[k - 1]));
        }
    }
    None
}

/// An inverter with a delay: the output follows the input by td (rising and falling alike), seen through a resistive load.
#[test]
fn a_delayed_inverter_follows_its_input_after_the_delay() {
    let mut c = Circuit::new();
    let n = place(&mut c, "vcc vss a y", "Y1 inv y 0 vss vcc 60 0.5 0 0 0 td=20n a");
    c.add(Element::vdc("Vcc", n[0], 0, 5.0));
    c.add(Element::Resistor { name: "Rg".into(), a: n[1], b: 0, r: 1e-3 });
    let s = parse_source_full("PULSE(0 5 1u 1n 1n 2u 4u)").unwrap();
    c.add(Element::VSource { name: "Va".into(), p: n[2], n: 0, wave: s.wave, ac: s.ac });
    c.add(Element::Resistor { name: "Rl".into(), a: n[3], b: 0, r: 10e3 });
    let r = transient(&c, &TranSettings { tstop: 9e-6, tstep: 20e-9, uic: false }).unwrap();
    // the input crosses 2.5 V at about 1.0005 us (rising) and 3.0005 us (falling), and 5.0005 us again
    let a_rise = edge_time(&r, "V(a)", 2.5, true, 0.0).unwrap();
    let y_fall = edge_time(&r, "V(y)", 2.5, false, 0.0).unwrap();
    assert!((y_fall - a_rise - 20e-9).abs() < 8e-9, "falling output {:.1} ns after the input rose", (y_fall - a_rise) * 1e9);
    let a_fall = edge_time(&r, "V(a)", 2.5, false, 2e-6).unwrap();
    let y_rise = edge_time(&r, "V(y)", 2.5, true, 2e-6).unwrap();
    assert!((y_rise - a_fall - 20e-9).abs() < 8e-9, "rising output {:.1} ns after the input fell", (y_rise - a_fall) * 1e9);
}

/// A pulse shorter than the delay does not get through (inertial delay), a longer one does.
#[test]
fn a_pulse_shorter_than_the_delay_is_swallowed() {
    for (width, passes) in [(10e-9, false), (60e-9, true)] {
        let mut c = Circuit::new();
        let n = place(&mut c, "vcc vss a y", "Y1 buf y 0 vss vcc 60 0.5 0 0 0 td=30n a");
        c.add(Element::vdc("Vcc", n[0], 0, 5.0));
        c.add(Element::Resistor { name: "Rg".into(), a: n[1], b: 0, r: 1e-3 });
        let s = parse_source_full(&format!("PULSE(0 5 1u 1n 1n {width:e} 5u)")).unwrap();
        c.add(Element::VSource { name: "Va".into(), p: n[2], n: 0, wave: s.wave, ac: s.ac });
        c.add(Element::Resistor { name: "Rl".into(), a: n[3], b: 0, r: 10e3 });
        let r = transient(&c, &TranSettings { tstop: 3e-6, tstep: 5e-9, uic: false }).unwrap();
        let top = r.signal("V(y)").unwrap().values.iter().cloned().fold(0.0f64, f64::max);
        assert_eq!(top > 2.5, passes, "a {width:e} s pulse through a 30 ns delay: the output reached {top}");
    }
}

/// Gates with delays make a ring oscillator that oscillates (no delay: it cannot): three inverters of 10 ns each, a half
/// period of 30 ns, and a buffer to an analog load to see it. The delay inside the ring is exact whatever the step is.
#[test]
fn a_ring_of_three_delayed_inverters_oscillates_with_the_period_of_six_delays() {
    let mut c = Circuit::new();
    let n = place(&mut c, "vcc vss y", "Y1 inv n2 0 vss vcc 60 0.5 0 0 0 td=10n n1\nY2 inv n3 0 vss vcc 60 0.5 0 0 0 td=10n n2\nY3 inv n1 0 vss vcc 60 0.5 0 0 0 td=10n n3\nY4 buf y 0 vss vcc 60 0.5 0 0 0 n1");
    c.add(Element::vdc("Vcc", n[0], 0, 5.0));
    c.add(Element::Resistor { name: "Rg".into(), a: n[1], b: 0, r: 1e-3 });
    c.add(Element::Resistor { name: "Rl".into(), a: n[2], b: 0, r: 10e3 });
    // an odd ring has no state to rest in: the run starts it going
    let r = transient(&c, &TranSettings { tstop: 2e-6, tstep: 5e-9, uic: false }).unwrap();
    let mut rises: Vec<f64> = vec![];
    let mut after = 0.0;
    while let Some(t) = edge_time(&r, "V(y)", 2.5, true, after) {
        rises.push(t);
        after = t + 1e-12;
    }
    assert!(rises.len() > 10, "the ring oscillates: {} rising edges", rises.len());
    // 2 us / 60 ns = 33 periods; the output is seen at the steps of the run (5 ns), 12 to a period
    assert!((30..=36).contains(&rises.len()), "{} rising edges in 2 us for a 60 ns period", rises.len());
}

/// The time inside the digital part does not depend on the step: a chain of 100 buffers of 10 ns each delays an edge by 1 us even
/// when the steps are 500 ns, and an output on an analog node that has a delay is a corner the run lands a step on.
#[test]
fn a_long_chain_of_delays_is_exact_whatever_the_step() {
    let mut body = String::new();
    for k in 0..100 {
        let (a, y) = (if k == 0 { "a".to_string() } else { format!("c{k}") }, format!("c{}", k + 1));
        body += &format!("Y{k} buf {y} 0 vss vcc 60 0.5 0 0 0 td=10n {a}\n");
    }
    body += "YO buf y 0 vss vcc 60 0.5 0 0 0 td=1n c100";
    let mut c = Circuit::new();
    let n = place(&mut c, "vcc vss a y", &body);
    c.add(Element::vdc("Vcc", n[0], 0, 5.0));
    c.add(Element::Resistor { name: "Rg".into(), a: n[1], b: 0, r: 1e-3 });
    let s = parse_source_full("PULSE(0 5 2u 1n 1n 5u 20u)").unwrap();
    c.add(Element::VSource { name: "Va".into(), p: n[2], n: 0, wave: s.wave, ac: s.ac });
    c.add(Element::Resistor { name: "Rl".into(), a: n[3], b: 0, r: 10e3 });
    let r = transient(&c, &TranSettings { tstop: 8e-6, tstep: 500e-9, uic: false }).unwrap();
    let a_rise = edge_time(&r, "V(a)", 2.5, true, 0.0).unwrap();
    let y_rise = edge_time(&r, "V(y)", 2.5, true, 0.0).unwrap();
    // 100 x 10 ns + 1 ns; the output is seen at the steps (the run lands one on the arrival), so it rises a step's worth later at most
    let d = y_rise - a_rise;
    assert!((1.0e-6..1.0e-6 + 520e-9).contains(&d), "the chain took {:.1} ns", d * 1e9);
}

/// The ring oscillator count does not depend on the step either when its output has a delay of its own (the run lands on it).
#[test]
fn a_ring_seen_through_a_delayed_buffer_has_the_same_edges_whatever_the_step() {
    let ring = |tstep: f64| {
        let mut c = Circuit::new();
        let n = place(&mut c, "vcc vss y", "Y1 inv n2 0 vss vcc 60 0.5 0 0 0 td=10n n1\nY2 inv n3 0 vss vcc 60 0.5 0 0 0 td=10n n2\nY3 inv n1 0 vss vcc 60 0.5 0 0 0 td=10n n3\nY4 buf y 0 vss vcc 60 0.5 0 0 0 td=1n n1");
        c.add(Element::vdc("Vcc", n[0], 0, 5.0));
        c.add(Element::Resistor { name: "Rg".into(), a: n[1], b: 0, r: 1e-3 });
        c.add(Element::Resistor { name: "Rl".into(), a: n[2], b: 0, r: 10e3 });
        let r = transient(&c, &TranSettings { tstop: 2e-6, tstep, uic: false }).unwrap();
        let (mut count, mut after) = (0, 0.0);
        while let Some(t) = edge_time(&r, "V(y)", 2.5, true, after) {
            count += 1;
            after = t + 1e-12;
        }
        count
    };
    let (fine, coarse) = (ring(5e-9), ring(200e-9));
    assert!((30..=36).contains(&fine), "{fine} rising edges at 5 ns");
    assert!((30..=36).contains(&coarse), "{coarse} rising edges at 200 ns steps");
}
