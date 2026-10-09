use crate::*;

fn ladder(n: usize) -> Circuit {
    let mut c = Circuit::new();
    let (p, g) = (c.node("n0"), c.node("0"));
    c.add(Element::VSource { name: "V1".into(), p, n: g, wave: parse_source("PULSE(0 1 0 1n 1n 1m 2m)").unwrap(), ac: None });
    for i in 0..n {
        let (a, b) = (c.node(&format!("n{i}")), c.node(&format!("n{}", i + 1)));
        c.add(Element::Resistor { name: format!("R{i}"), a, b, r: 1000.0 });
        c.add(Element::Capacitor { name: format!("C{i}"), a: b, b: g, c: 1e-9 });
    }
    c
}

/// A long resistor chain with a load at the end is a divider with a known answer; this
/// exercises the sparse solver (including the voltage-source branch row) on a large system.
#[test]
fn a_long_resistor_chain_solves_exactly() {
    let n = 500;
    let mut c = Circuit::new();
    let (p, g) = (c.node("n0"), c.node("0"));
    c.add(Element::VSource { name: "V1".into(), p, n: g, wave: parse_source("DC 10").unwrap(), ac: None });
    for i in 0..n {
        let (a, b) = (c.node(&format!("n{i}")), c.node(&format!("n{}", i + 1)));
        c.add(Element::Resistor { name: format!("R{i}"), a, b, r: 100.0 });
    }
    let last = c.node(&format!("n{n}"));
    c.add(Element::Resistor { name: "RL".into(), a: last, b: g, r: 100.0 });
    let r = dc_op(&c).unwrap();
    for k in [1, 100, 250, 499, 500] {
        let want = 10.0 * (n + 1 - k) as f64 / (n + 1) as f64;
        let got = r.voltage(&format!("n{k}")).unwrap();
        // GMIN to ground at every node leaves ~1e-6 relative error
        assert!((got - want).abs() < 1e-4, "n{k}: {got} vs {want}");
    }
}

/// `cargo test -p sim-core --release -- --ignored --nocapture` prints transient timings.
#[test]
#[ignore]
fn bench_rc_ladder_transient() {
    for n in [50, 100, 200, 400, 800] {
        let t = std::time::Instant::now();
        let r = transient(&ladder(n), &TranSettings { tstop: 2e-3, tstep: 1e-6, uic: false }).unwrap();
        eprintln!("PERF n={n} points={} time={:?}", r.time.len(), t.elapsed());
    }
}

/// Every stage also has a clamp diode to ground, so each step needs Newton iterations.
#[test]
#[ignore]
fn bench_diode_ladder_transient() {
    for n in [25, 50, 100, 200] {
        let mut c = ladder(n);
        let g = c.node("0");
        for i in 1..=n {
            let a = c.node(&format!("n{i}"));
            c.add(Element::Diode { name: format!("D{i}"), a, k: g, model: DiodeModel::default() });
        }
        let t = std::time::Instant::now();
        let r = transient(&c, &TranSettings { tstop: 2e-3, tstep: 1e-6, uic: false }).unwrap();
        eprintln!("PERF diode n={n} points={} time={:?}", r.time.len(), t.elapsed());
    }
}
