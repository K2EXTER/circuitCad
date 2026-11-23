//! Saturating inductors: `L1 a b 10u ISAT=<A> LDROP=<0..1>` in a library model.

use crate::*;

/// 1 V across a saturating inductor (with a 1 mOhm series resistor so the DC point exists).
fn ramp(text: &str, tstop: f64) -> TranResult {
    let mut c = Circuit::new();
    let (a, o) = (c.node("a"), c.node("o"));
    add_subckt_instance(&mut c, "U1", "T", text, &[o, 0]).unwrap_or_else(|e| panic!("{e}"));
    c.add(Element::Resistor { name: "R1".into(), a, b: o, r: 1e-3 });
    c.add(Element::VSource { name: "V1".into(), p: a, n: 0, wave: parse_source("0").unwrap(), ac: None });
    // start from rest and switch the 1 V on: a pulse that rises at once
    if let Some(Element::VSource { wave, .. }) = c.elements.iter_mut().find(|e| matches!(e, Element::VSource { .. })) {
        *wave = parse_source("PULSE(0 1 0 1n 1n 1 2)").unwrap();
    }
    transient(&c, &TranSettings { tstop, tstep: tstop / 400.0, uic: true }).unwrap()
}

fn at(r: &TranResult, name: &str, t: f64) -> f64 {
    let s = r.signal(name).unwrap();
    let i = r.time.partition_point(|&x| x < t).clamp(1, r.time.len() - 1);
    let (t0, t1) = (r.time[i - 1], r.time[i]);
    s.values[i - 1] + (t - t0) / (t1 - t0).max(1e-30) * (s.values[i] - s.values[i - 1])
}

#[test]
fn a_saturating_inductor_follows_the_flux_curve() {
    // L = 10u, the inductance halves at 1 A: i0 = 1 A. flux = L*i0*atan(i/i0) = V*t, so i = i0*tan(V t / (L i0))
    let r = ramp(".subckt T p gnd\nL1 p gnd 10u ISAT=1 LDROP=0.5\n.ends T", 12e-6);
    for (t, want) in [(5e-6, (0.5f64).tan()), (10e-6, (1.0f64).tan())] {
        let got = at(&r, "I(U1.L1)", t);
        assert!((got / want - 1.0).abs() < 0.03, "t={t}: {got} vs {want}");
    }
}

#[test]
fn without_isat_the_inductor_is_linear() {
    let r = ramp(".subckt T p gnd\nL1 p gnd 10u\n.ends T", 12e-6);
    let got = at(&r, "I(U1.L1)", 10e-6);
    assert!((got / 1.0 - 1.0).abs() < 0.02, "{got}");
}

#[test]
fn the_drop_sets_where_the_inductance_falls() {
    // 30 % down at 2 A: the incremental inductance at 2 A is 0.7 L (i0 = 2*sqrt(0.7/0.3))
    let i0 = Circuit::saturation_scale(2.0, 0.3);
    assert!((1.0 / (1.0 + (2.0 / i0).powi(2)) - 0.7).abs() < 1e-12);
}

#[test]
fn ac_uses_the_inductance_at_the_operating_current() {
    // a 1 A DC bias through the coil halves its inductance (LDROP=0.5 at ISAT=1); 1 A of AC across it reads w*L
    let mut c = Circuit::new();
    let a = c.node("a");
    add_subckt_instance(&mut c, "U1", "T", ".subckt T p q
L1 p q 10u ISAT=1 LDROP=0.5
.ends T", &[a, 0]).unwrap();
    c.add(Element::ISource { name: "Ibias".into(), from: 0, to: a, wave: parse_source("1").unwrap(), ac: None });
    c.add(Element::ISource { name: "Iac".into(), from: 0, to: a, wave: parse_source("0").unwrap(), ac: Some((1.0, 0.0)) });
    let r = ac_analysis(&c, &AcSettings { fstart: 1e5, fstop: 1e5, sweep: AcSweep::Linear(2) }).unwrap();
    let v = r.signal("V(a)").unwrap().mag()[0];
    let w = 2.0 * std::f64::consts::PI * 1e5;
    assert!((v / (w * 5e-6) - 1.0).abs() < 1e-6, "{v} vs {}", w * 5e-6);
}

#[test]
fn an_inductor_follows_the_curve_of_its_data_sheet() {
    // a hard knee: full inductance to 1 A, then down to a fifth at 2 A and flat beyond. 1 V across 10 uH: the current rises at
    // 0.1 A/us to 1 A (10 us), then faster as the inductance falls; at 2 A and beyond 2 uH makes 0.5 A/us
    let k = SatCurve::parse("0:1,1:1,2:0.2").unwrap();
    assert_eq!((k.ratio(0.5), k.ratio(1.5), k.ratio(-3.0)), (1.0, 0.6, 0.2));
    // flux per henry: 1 up to 1 A, the trapezoid (1 + 0.2) / 2 from 1 to 2 A, 0.2 a A beyond
    assert!((k.flux(2.0) - 1.6).abs() < 1e-12 && (k.flux(-3.0) + 1.8).abs() < 1e-12);
    let r = ramp(".subckt T p gnd\nL1 p gnd 10u LCURVE=0:1,1:1,2:0.2\n.ends T", 20e-6);
    // flux = V t: 10 us at 1 A; 16 us at 2 A; 18 us at 3 A
    for (t, want) in [(5e-6, 0.5), (10e-6, 1.0), (16e-6, 2.0), (18e-6, 3.0)] {
        let got = at(&r, "I(U1.L1)", t);
        assert!((got - want).abs() < 0.03 * want, "t={t}: {got} vs {want}");
    }
    // the AC inductance at a 1.5 A bias: 0.6 of 10 uH
    let mut c = Circuit::new();
    let a = c.node("a");
    add_subckt_instance(&mut c, "U1", "T", ".subckt T p q\nL1 p q 10u LCURVE=0:1,1:1,2:0.2\n.ends T", &[a, 0]).unwrap();
    c.add(Element::ISource { name: "Ibias".into(), from: 0, to: a, wave: parse_source("1.5").unwrap(), ac: None });
    c.add(Element::ISource { name: "Iac".into(), from: 0, to: a, wave: parse_source("0").unwrap(), ac: Some((1.0, 0.0)) });
    let ac = ac_analysis(&c, &AcSettings { fstart: 1e5, fstop: 1e5, sweep: AcSweep::Linear(2) }).unwrap();
    let s = ac.signal("V(a)").unwrap();
    let l = s.re[0].hypot(s.im[0]) / (2.0 * std::f64::consts::PI * 1e5);
    assert!((l - 6e-6).abs() < 0.05e-6, "{l}");
    // a curve out of order says so
    assert!(SatCurve::parse("0:1,2:0.5,1:0.8").unwrap_err().contains("小さい順"));
}
