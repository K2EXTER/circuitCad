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
