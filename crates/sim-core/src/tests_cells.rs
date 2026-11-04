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

fn dff_with_data(td_data: &str, tsu: &str, th: &str) -> TranResult {
    // a flip-flop sampling a data signal: the data goes through a chain of buffers (td_data each, 3 of them), the clock is direct
    let body = format!(
        "Y1 buf d1 0 vss vcc 60 0.5 0 0 0 td={td_data} din\nY2 buf d2 0 vss vcc 60 0.5 0 0 0 td={td_data} d1\nY3 buf d 0 vss vcc 60 0.5 0 0 0 td={td_data} d2\nYF dff q 0 vss vcc 60 0.5 0 0 0 tsu={tsu} th={th} d clk 0 0"
    );
    let mut c = Circuit::new();
    let n = place(&mut c, "vcc vss din clk q", &body);
    c.add(Element::vdc("Vcc", n[0], 0, 5.0));
    c.add(Element::Resistor { name: "Rg".into(), a: n[1], b: 0, r: 1e-3 });
    // data rises at 1.0 us, the clock rises at 1.0 us + 150 ns and 2 us: with 3 x 40 ns the data is at the flip-flop at 1.12 us
    let dpulse = parse_source_full("PULSE(0 5 1u 1n 1n 0.6u 4u)").unwrap();
    c.add(Element::VSource { name: "Vd".into(), p: n[2], n: 0, wave: dpulse.wave, ac: dpulse.ac });
    let ck = parse_source_full("PULSE(0 5 1.15u 1n 1n 0.4u 1u)").unwrap();
    c.add(Element::VSource { name: "Vck".into(), p: n[3], n: 0, wave: ck.wave, ac: ck.ac });
    c.add(Element::Resistor { name: "Rl".into(), a: n[4], b: 0, r: 10e3 });
    transient(&c, &TranSettings { tstop: 3e-6, tstep: 20e-9, uic: false }).unwrap()
}

/// The data reaches the flip-flop 30 ns before the clock edge: a setup time of 20 ns is met, of 50 ns is not, and the report says so.
#[test]
fn the_setup_time_is_checked_at_the_clock_edge() {
    let ok = dff_with_data("40n", "20n", "0");
    assert!(ok.timing.violations.is_empty(), "{:?}", ok.timing);
    assert!(!ok.warnings.iter().any(|w| w.contains("セットアップ")), "{:?}", ok.warnings);
    let (cell, margin, _interval) = ok.timing.tightest.clone().expect("a margin is found");
    assert!(cell.ends_with("YF"), "{cell}");
    // 30 ns between the data and the edge, minus 20 ns of setup: about 10 ns of room
    assert!((margin - 10e-9).abs() < 6e-9, "the margin {margin:e}");
    let bad = dff_with_data("40n", "50n", "0");
    assert_eq!(bad.timing.violations.len(), 1, "{:?}", bad.timing);
    let v = &bad.timing.violations[0];
    assert_eq!(v.kind, crate::TimingKind::Setup);
    assert!((v.worst_margin - 30e-9).abs() < 6e-9 && (v.required - 50e-9).abs() < 1e-12, "{v:?}");
    assert!(bad.warnings.iter().any(|w| w.contains("セットアップ時間が足りません")), "{:?}", bad.warnings);
}

/// The data changes 30 ns after the clock edge: a hold time of 10 ns is met, of 60 ns is not.
#[test]
fn the_hold_time_is_checked_after_the_clock_edge() {
    // data falls at 1.0 + 0.6 us = 1.6 us, + 120 ns of chain = 1.72 us; the clock edge at 1.15 us is far: use the second edge of the clock (2.15 us)
    // instead: put the fall of the data 30 ns after an edge by a data pulse that is 0.6 us long and a chain of 3 x 40 ns
    let run = |th: &str| {
        let body = format!("Y1 buf d1 0 vss vcc 60 0.5 0 0 0 td=40n din\nY2 buf d2 0 vss vcc 60 0.5 0 0 0 td=40n d1\nY3 buf d 0 vss vcc 60 0.5 0 0 0 td=40n d2\nYF dff q 0 vss vcc 60 0.5 0 0 0 tsu=5n th={th} d clk 0 0");
        let mut c = Circuit::new();
        let n = place(&mut c, "vcc vss din clk q", &body);
        c.add(Element::vdc("Vcc", n[0], 0, 5.0));
        c.add(Element::Resistor { name: "Rg".into(), a: n[1], b: 0, r: 1e-3 });
        // data: high from 0.5 us to 1.5 us (at the flip-flop: 0.62 us to 1.62 us); the clock edge at 1.59 us: the data falls 30 ns after it
        let dpulse = parse_source_full("PULSE(0 5 0.5u 1n 1n 1u 4u)").unwrap();
        c.add(Element::VSource { name: "Vd".into(), p: n[2], n: 0, wave: dpulse.wave, ac: dpulse.ac });
        let ck = parse_source_full("PULSE(0 5 1.59u 1n 1n 0.3u 4u)").unwrap();
        c.add(Element::VSource { name: "Vck".into(), p: n[3], n: 0, wave: ck.wave, ac: ck.ac });
        c.add(Element::Resistor { name: "Rl".into(), a: n[4], b: 0, r: 10e3 });
        transient(&c, &TranSettings { tstop: 3e-6, tstep: 20e-9, uic: false }).unwrap()
    };
    assert!(run("10n").timing.violations.is_empty());
    let bad = run("60n");
    assert_eq!(bad.timing.violations.len(), 1, "{:?}", bad.timing);
    assert_eq!(bad.timing.violations[0].kind, crate::TimingKind::Hold);
    assert!(bad.warnings.iter().any(|w| w.contains("ホールド時間が足りません")), "{:?}", bad.warnings);
}
