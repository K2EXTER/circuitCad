//! Diode charge storage: junction capacitance (CJO / VJ / M / FC) and the transit time TT.

use crate::*;

fn model(cj0: f64, tt: f64) -> DiodeModel {
    DiodeModel { cj0, vj: 0.7, m: 0.4, tt, ..DiodeModel::default() }
}

/// A diode reverse biased at 2 V through 1k from a source with an AC part.
fn reverse(m: DiodeModel) -> Circuit {
    let mut c = Circuit::new();
    let (src, a) = (c.node("src"), c.node("a"));
    c.add(Element::Diode { name: "D1".into(), a: 0, k: a, model: m });
    c.add(Element::Resistor { name: "R1".into(), a: src, b: a, r: 1e3 });
    c.add(Element::VSource { name: "Vs".into(), p: src, n: 0, wave: parse_source("2").unwrap(), ac: Some((1.0, 0.0)) });
    c
}

#[test]
fn a_reverse_biased_diode_capacitance_sets_the_ac_corner() {
    let cap = 100e-12 / (1.0 + 2.0 / 0.7f64).powf(0.4);
    let f = 1.0 / (2.0 * std::f64::consts::PI * 1e3 * cap);
    let r = ac_analysis(&reverse(model(100e-12, 0.0)), &AcSettings { fstart: f, fstop: f, sweep: AcSweep::Linear(2) }).unwrap();
    let mag = r.signal("V(a)").unwrap().mag()[0];
    assert!((mag - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.01, "{mag}");
}

#[test]
fn the_transient_charges_the_junction_capacitance() {
    let mut c = reverse(model(100e-12, 0.0));
    if let Some(Element::VSource { wave, .. }) = c.elements.iter_mut().find(|e| matches!(e, Element::VSource { .. })) {
        *wave = parse_source("PULSE(2 2.01 10n 1n 1n 1 2)").unwrap();
    }
    let cap = 100e-12 / (1.0 + 2.0 / 0.7f64).powf(0.4);
    let tau = 1e3 * cap;
    let r = transient(&c, &TranSettings { tstop: 10e-9 + 5.0 * tau, tstep: tau / 50.0, uic: false }).unwrap();
    let s = r.signal("V(a)").unwrap();
    let t = 11e-9 + 3.0 * tau;
    let i = r.time.partition_point(|&x| x < t).clamp(1, r.time.len() - 1);
    let (t0, t1) = (r.time[i - 1], r.time[i]);
    let rise = s.values[i - 1] + (t - t0) / (t1 - t0) * (s.values[i] - s.values[i - 1]) - 2.0;
    assert!((rise / 0.01 - (1.0 - (-3.0f64).exp())).abs() < 0.03, "{rise}");
}

#[test]
fn the_transit_time_adds_a_diffusion_capacitance_that_follows_the_current() {
    // 1 mA forward: rd = VT / I, the diffusion capacitance is tt / rd, so rd * C = tt: the impedance falls to rd / sqrt(2) at 1 / (2 pi tt)
    let tt = 10e-9;
    let mut c = Circuit::new();
    let a = c.node("a");
    c.add(Element::Diode { name: "D1".into(), a, k: 0, model: DiodeModel { tt, ..DiodeModel::default() } });
    c.add(Element::ISource { name: "I1".into(), from: 0, to: a, wave: parse_source("1m").unwrap(), ac: Some((1.0, 0.0)) });
    let f = 1.0 / (2.0 * std::f64::consts::PI * tt);
    let r = ac_analysis(&c, &AcSettings { fstart: f, fstop: f, sweep: AcSweep::Linear(2) }).unwrap();
    let z = r.signal("V(a)").unwrap().mag()[0];
    let rd = 0.025865 / 1e-3;
    assert!((z / (rd / std::f64::consts::SQRT_2) - 1.0).abs() < 0.02, "{z} vs {}", rd / std::f64::consts::SQRT_2);
}

#[test]
fn model_cards_read_the_diode_capacitance_parameters_and_area_scales_the_capacitance() {
    let m = parse_models(".model D1 D(IS=1n CJO=2p VJ=0.6 M=0.33 FC=0.4 TT=5n)").unwrap();
    let d = match &m["d1"] { spice_in::Model::Diode(d) => d.clone(), _ => panic!() };
    assert_eq!((d.cj0, d.vj, d.m, d.fc, d.tt), (2e-12, 0.6, 0.33, 0.4, 5e-9));
    assert!(d.has_charge() && !DiodeModel::default().has_charge());
    assert!(ignored_model_params(".model D1 D(IS=1n CJO=2p TT=1n ISR=1p)").iter().all(|(_, p)| p == &vec!["ISR".to_string()]));
    let mut c = Circuit::new();
    let a = c.node("a");
    add_subckt_instance(&mut c, "U1", "T", ".subckt T a gnd\nD1 a gnd DM 3\n.model DM D(CJO=2p)\n.ends T", &[a, 0]).unwrap();
    let cj = c.elements.iter().find_map(|e| match e { Element::Diode { model, .. } => Some(model.cj0), _ => None }).unwrap();
    assert!((cj / 6e-12 - 1.0).abs() < 1e-12, "{cj}");
}
