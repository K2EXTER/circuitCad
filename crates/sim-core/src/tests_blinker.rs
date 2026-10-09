//! LED blinker (astable multivibrator): a regenerative loop that used to stop the transient
//! solver with "did not converge" at the moment of every flip.

use crate::*;

fn blinker(model: BjtModel, r_base2: f64, cap: f64) -> Circuit {
    let mut c = Circuit::new();
    let (vcc, c1, c2, b1, b2, a1, a2) = (c.node("vcc"), c.node("c1"), c.node("c2"), c.node("b1"), c.node("b2"), c.node("a1"), c.node("a2"));
    c.add(Element::vdc("Vcc", vcc, 0, 5.0));
    let r = |c: &mut Circuit, name: &str, a: usize, b: usize, ohms: f64| c.add(Element::Resistor { name: name.into(), a, b, r: ohms });
    r(&mut c, "R1", vcc, a1, 330.0);
    r(&mut c, "R4", vcc, a2, 330.0);
    r(&mut c, "R2", vcc, b1, 47e3);
    r(&mut c, "R3", vcc, b2, r_base2);
    let led = DiodeModel { is: 1e-18, n: 2.0, rs: 5.0, ..DiodeModel::default() };
    c.add(Element::Diode { name: "D1".into(), a: a1, k: c1, model: led.clone() });
    c.add(Element::Diode { name: "D2".into(), a: a2, k: c2, model: led });
    c.add(Element::Capacitor { name: "C1".into(), a: c1, b: b2, c: cap });
    c.add(Element::Capacitor { name: "C2".into(), a: c2, b: b1, c: cap });
    c.add(Element::Bjt { name: "Q1".into(), c: c1, b: b1, e: 0, model: model.clone() });
    c.add(Element::Bjt { name: "Q2".into(), c: c2, b: b2, e: 0, model });
    c
}

fn toggles(r: &TranResult, node: &str) -> (usize, f64, f64) {
    let s = r.signal(node).unwrap();
    let n = s.values.windows(2).filter(|w| (w[0] > 2.5) != (w[1] > 2.5)).count();
    let (lo, hi) = s.values.iter().fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
    (n, lo, hi)
}

fn run(c: &Circuit, tstop: f64, uic: bool) -> Result<TranResult, SimError> {
    transient(c, &TranSettings { tstop, tstep: 2e-3, uic })
}

#[test]
fn astable_multivibrator_oscillates_with_the_expected_period() {
    let c = blinker(BjtModel { bf: 200.0, ..BjtModel::default() }, 56e3, 10e-6);
    let r = run(&c, 6.0, false).expect("no convergence failure at the flips");
    let (n, lo, hi) = toggles(&r, "V(c1)");
    // each collector swings between saturation (~0 V) and the LED drop below the supply
    assert!(lo < 0.2 && hi > 3.0, "{lo} {hi}");
    // the collector only swings about 3.5 V (the LED drop), so each half period is
    // R*C*ln((Vcc + swing - Vbe) / (Vcc - Vbe)) = 0.6 R C: 0.28 s and 0.34 s,
    // a full period of about 0.62 s, i.e. about 19 edges in six seconds
    assert!((17..=24).contains(&n), "{n} edges");
}

#[test]
fn symmetric_circuit_and_zero_state_start_also_run() {
    let c = blinker(BjtModel { bf: 200.0, ..BjtModel::default() }, 47e3, 10e-6);
    for uic in [false, true] {
        let r = run(&c, 4.0, uic).unwrap_or_else(|e| panic!("uic={uic}: {e}"));
        assert!(toggles(&r, "V(c1)").0 >= 8, "uic={uic}");
    }
}

#[test]
fn slower_blinker_with_big_capacitors() {
    let c = blinker(BjtModel { bf: 200.0, ..BjtModel::default() }, 56e3, 100e-6);
    let r = run(&c, 10.0, false).unwrap();
    // half periods of 3.2 s and 3.9 s
    assert!((2..=4).contains(&toggles(&r, "V(c1)").0));
}

#[test]
fn led_blinker_example_runs() {
    let sch: sch_core::Schematic = serde_json::from_str(include_str!("../../../docs/examples/led_blinker.schjson")).unwrap();
    let c = from_schematic(&sch).unwrap();
    let r = run(&c, 5.0, false).unwrap();
    let (n1, ..) = toggles(&r, "V(COL1)");
    let (n2, ..) = toggles(&r, "V(COL2)");
    assert!(n1 >= 8 && n2 >= 8, "{n1} {n2}");
    // the two collectors are in opposite states most of the time
    let (a, b) = (r.signal("V(COL1)").unwrap(), r.signal("V(COL2)").unwrap());
    let opposite = a.values.iter().zip(&b.values).filter(|(x, y)| (**x > 2.5) != (**y > 2.5)).count();
    assert!(opposite as f64 > 0.9 * a.values.len() as f64, "{opposite} of {}", a.values.len());
}

#[test]
fn damping_does_not_change_ordinary_results() {
    // a plain RC step still lands on the analytic curve to the usual accuracy
    let mut c = Circuit::new();
    let (a, o) = (c.node("a"), c.node("o"));
    c.add(Element::vdc("V1", a, 0, 5.0));
    c.add(Element::Resistor { name: "R1".into(), a, b: o, r: 1e3 });
    c.add(Element::Capacitor { name: "C1".into(), a: o, b: 0, c: 1e-6 });
    let r = transient(&c, &TranSettings { tstop: 5e-3, tstep: 1e-5, uic: true }).unwrap();
    let v = r.signal("V(o)").unwrap();
    let k = r.time.iter().position(|t| *t >= 1e-3).unwrap();
    assert!((v.values[k] - 5.0 * (1.0 - (-r.time[k] / 1e-3).exp())).abs() < 5e-3);
}
