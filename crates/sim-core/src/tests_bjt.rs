//! Transistor model: Early effect (VAF / VAR) and the series resistances RB, RC, RE.

use crate::*;

fn bjt_circuit(model: BjtModel, ext_re: f64, ext_rc: f64, vb: f64, vc: f64) -> (Circuit, usize) {
    let mut c = Circuit::new();
    let (b, col, e) = (c.node("b"), c.node("c"), c.node("e"));
    let (cb, ce) = (if ext_rc > 0.0 { c.node("cx") } else { col }, if ext_re > 0.0 { e } else { 0 });
    c.add(Element::Bjt { name: "Q1".into(), c: cb, b, e: ce, model });
    c.add(Element::vdc("Vb", b, 0, vb));
    c.add(Element::vdc("Vc", col, 0, vc));
    if ext_rc > 0.0 {
        c.add(Element::Resistor { name: "Rc".into(), a: col, b: cb, r: ext_rc });
    }
    if ext_re > 0.0 {
        c.add(Element::Resistor { name: "Re".into(), a: e, b: 0, r: ext_re });
    }
    (c, col)
}

fn ic(c: &Circuit) -> f64 {
    // current out of the collector source = collector current
    -dc_op(c).unwrap().element("Vc").unwrap().current
}

#[test]
fn the_early_effect_makes_the_collector_current_grow_with_vce() {
    let m = |vaf: f64| BjtModel { vaf, ..BjtModel::default() };
    // same base voltage, collector at 2 V and at 12 V
    let (lo, hi) = (ic(&bjt_circuit(m(100.0), 0.0, 0.0, 0.65, 2.0).0), ic(&bjt_circuit(m(100.0), 0.0, 0.0, 0.65, 12.0).0));
    // 1 + vce/vaf with vce = vcb + vbe: the ratio is (1 + 11.35/100) / (1 + 1.35/100) ~ 1.099 (the base current does not move)
    assert!((hi / lo - (1.0 + 11.35 / 100.0) / (1.0 + 1.35 / 100.0)).abs() < 0.004, "{}", hi / lo);
    // without it the current barely moves
    let (lo0, hi0) = (ic(&bjt_circuit(m(f64::INFINITY), 0.0, 0.0, 0.65, 2.0).0), ic(&bjt_circuit(m(f64::INFINITY), 0.0, 0.0, 0.65, 12.0).0));
    assert!((hi0 / lo0 - 1.0).abs() < 1e-3, "{}", hi0 / lo0);
}

#[test]
fn internal_re_and_rc_match_external_resistors() {
    let base = BjtModel { bf: 150.0, ..BjtModel::default() };
    let inside = BjtModel { re: 10.0, rc: 5.0, ..base.clone() };
    let (a, _) = bjt_circuit(inside, 0.0, 0.0, 1.0, 5.0);
    let (b, _) = bjt_circuit(base, 10.0, 5.0, 1.0, 5.0);
    let (ia, ib) = (ic(&a), ic(&b));
    assert!(ia > 1e-3, "{ia}");
    assert!((ia / ib - 1.0).abs() < 1e-6, "{ia} {ib}");
}

#[test]
fn a_pnp_with_early_voltage_still_converges_and_conducts_the_other_way() {
    let mut c = Circuit::new();
    let (e, b, col) = (c.node("e"), c.node("b"), c.node("c"));
    c.add(Element::Bjt { name: "Q1".into(), c: col, b, e, model: BjtModel { pnp: true, vaf: 80.0, re: 1.0, rc: 1.0, ..BjtModel::default() } });
    c.add(Element::vdc("Ve", e, 0, 5.0));
    c.add(Element::vdc("Vb", b, 0, 4.2));
    c.add(Element::Resistor { name: "Rl".into(), a: col, b: 0, r: 100.0 });
    let r = dc_op(&c).unwrap();
    assert!(r.voltage("c").unwrap() > 0.1, "{}", r.voltage("c").unwrap());
}

#[test]
fn model_cards_read_the_early_voltage_and_series_resistances() {
    let m = parse_models(".model Q1 NPN(BF=100 VAF=80 VAR=20 RC=2 RE=0.5)\n.model Q2 NPN(BF=100 VA=60)").unwrap();
    let get = |n: &str| match &m[n] { spice_in::Model::Bjt(b) => b.clone(), _ => panic!() };
    let q1 = get("q1");
    assert_eq!((q1.vaf, q1.var, q1.rc, q1.re), (80.0, 20.0, 2.0, 0.5));
    assert_eq!(get("q2").vaf, 60.0);
    assert!(get("q2").var.is_infinite());
}

// ---- charge storage: junction capacitances and the transit time

#[test]
fn the_depletion_charge_is_continuous_and_its_derivative_is_the_capacitance() {
    use crate::dc::depletion;
    let (cj0, vj, m, fc) = (10e-12, 0.75, 0.33, 0.5);
    for v in [-5.0, -1.0, 0.0, 0.3, 0.37, 0.375, 0.38, 0.5, 0.9] {
        let (q, c) = depletion(cj0, vj, m, fc, v);
        let h = 1e-6;
        let dq = (depletion(cj0, vj, m, fc, v + h).0 - depletion(cj0, vj, m, fc, v - h).0) / (2.0 * h);
        assert!((dq / c - 1.0).abs() < 1e-4, "v={v}: dQ/dV {dq} vs C {c} (q={q})");
    }
    // C at zero bias is cj0, and falls as the junction is reverse biased
    assert!((depletion(cj0, vj, m, fc, 0.0).1 - cj0).abs() < 1e-18);
    assert!(depletion(cj0, vj, m, fc, -3.0).1 < cj0);
}

/// Base driven through 1k, collector at 0 V, emitter at 0 V, base held at -2 V: both junctions are reverse biased.
fn reverse_biased(model: BjtModel) -> Circuit {
    let mut c = Circuit::new();
    let (src, b) = (c.node("src"), c.node("b"));
    c.add(Element::Bjt { name: "Q1".into(), c: 0, b, e: 0, model });
    c.add(Element::Resistor { name: "R1".into(), a: src, b, r: 1e3 });
    c.add(Element::VSource { name: "Vs".into(), p: src, n: 0, wave: parse_source("-2").unwrap(), ac: Some((1.0, 0.0)) });
    c
}

#[test]
fn a_reverse_biased_junction_capacitance_sets_the_ac_corner() {
    let m = BjtModel { cjc: 100e-12, cje: 50e-12, vjc: 0.75, mjc: 0.33, vje: 0.75, mje: 0.33, ..BjtModel::default() };
    let c = reverse_biased(m);
    // both junctions see 2 V reverse: C = cj0 / (1 + 2/0.75)^0.33 each
    let cap = (100e-12 + 50e-12) / (1.0 + 2.0 / 0.75f64).powf(0.33);
    let f = 1.0 / (2.0 * std::f64::consts::PI * 1e3 * cap);
    let r = ac_analysis(&c, &AcSettings { fstart: f, fstop: f, sweep: AcSweep::Linear(2) }).unwrap();
    let mag = r.signal("V(b)").unwrap().mag()[0];
    // at the corner the divider reads 1/sqrt(2)
    assert!((mag - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.01, "{mag}");
}

#[test]
fn the_transient_charges_the_same_capacitance() {
    let m = BjtModel { cjc: 100e-12, cje: 50e-12, ..BjtModel::default() };
    let mut c = reverse_biased(m);
    // a 10 mV step on top of the -2 V: tau = R * C(-2 V)
    if let Some(Element::VSource { wave, .. }) = c.elements.iter_mut().find(|e| matches!(e, Element::VSource { .. })) {
        *wave = parse_source("PULSE(-2 -1.99 10n 1n 1n 1 2)").unwrap();
    }
    let cap = 150e-12 / (1.0 + 2.0 / 0.75f64).powf(0.33);
    let tau = 1e3 * cap;
    let r = transient(&c, &TranSettings { tstop: 10e-9 + 5.0 * tau, tstep: tau / 50.0, uic: false }).unwrap();
    let s = r.signal("V(b)").unwrap();
    let at = |t: f64| {
        let i = r.time.partition_point(|&x| x < t).clamp(1, r.time.len() - 1);
        let (t0, t1) = (r.time[i - 1], r.time[i]);
        s.values[i - 1] + (t - t0) / (t1 - t0) * (s.values[i] - s.values[i - 1])
    };
    let rise = at(10e-9 + 1e-9 + 3.0 * tau) - (-2.0);
    // after 3 tau of a 10 mV step: 10 mV * (1 - e^-3) = 9.5 mV
    assert!((rise / 0.01 - (1.0 - (-3.0f64).exp())).abs() < 0.03, "{rise}");
}

#[test]
fn the_transit_time_makes_the_current_gain_roll_off_at_f_beta() {
    // 10 uA of base current -> about 1 mA of collector current (bf 100); tf = 0.1 ns: |h21| = bf / sqrt(1 + (w bf tf)^2)
    let mut c = Circuit::new();
    let (b, col) = (c.node("b"), c.node("c"));
    c.add(Element::Bjt { name: "Q1".into(), c: col, b, e: 0, model: BjtModel { bf: 100.0, tf: 0.1e-9, ..BjtModel::default() } });
    c.add(Element::ISource { name: "Ib".into(), from: 0, to: b, wave: parse_source("10u").unwrap(), ac: Some((1.0, 0.0)) });
    c.add(Element::vdc("Vc", col, 0, 5.0));
    let f = 1.0 / (2.0 * std::f64::consts::PI * 100.0 * 0.1e-9);
    let r = ac_analysis(&c, &AcSettings { fstart: f, fstop: f, sweep: AcSweep::Linear(2) }).unwrap();
    let h21 = r.signal("I(Vc)").unwrap().mag()[0];
    assert!((h21 / (100.0 * std::f64::consts::FRAC_1_SQRT_2) - 1.0).abs() < 0.02, "{h21}");
}

#[test]
fn model_cards_read_the_capacitance_parameters() {
    let m = parse_models(".model Q1 NPN(CJE=2p VJE=0.8 MJE=0.4 CJC=1p PC=0.6 MC=0.3 FC=0.6 TF=0.3n TR=5n)").unwrap();
    let q = match &m["q1"] { spice_in::Model::Bjt(b) => b.clone(), _ => panic!() };
    assert_eq!((q.cje, q.vje, q.mje, q.cjc, q.vjc, q.mjc, q.fc, q.tf, q.tr), (2e-12, 0.8, 0.4, 1e-12, 0.6, 0.3, 0.6, 0.3e-9, 5e-9));
    assert!(q.has_charge());
    assert!(!BjtModel::default().has_charge());
    assert!(ignored_model_params(".model Q1 NPN(CJE=2p TF=1n IKF=0.1)").iter().all(|(_, p)| p == &vec!["IKF".to_string()]));
}
