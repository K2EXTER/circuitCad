//! The remaining model parameters: BJT leakage currents (ISE/ISC), high injection (IKF), diode recombination (ISR),
//! the temperature parameters EG / XTI / XTB, and the MOSFET overlap capacitances (CGSO/CGDO).

use crate::dc::bjt_eval;
use crate::*;

fn rich_bjt() -> BjtModel {
    BjtModel { bf: 150.0, br: 2.0, vaf: 80.0, var: 30.0, ise: 1e-14, ne: 1.5, isc: 2e-14, nc: 2.0, ikf: 0.05, ikr: 0.02, ..BjtModel::default() }
}

#[test]
fn the_bjt_derivatives_match_finite_differences_with_every_effect_on() {
    let m = rich_bjt();
    for (vbe, vbc) in [(0.2, -3.0), (0.55, -1.0), (0.7, -5.0), (0.75, 0.2), (0.4, 0.4), (-0.5, -0.5)] {
        let q = bjt_eval(&m, vbe, vbc);
        let h = 1e-7;
        let (e1, e2) = (bjt_eval(&m, vbe + h, vbc), bjt_eval(&m, vbe - h, vbc));
        let (f1, f2) = (bjt_eval(&m, vbe, vbc + h), bjt_eval(&m, vbe, vbc - h));
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-4 * a.abs().max(b.abs()) + 1e-12;
        assert!(close(q.dic.0, (e1.ic - e2.ic) / (2.0 * h)), "dic/dvbe at {vbe},{vbc}: {} vs {}", q.dic.0, (e1.ic - e2.ic) / (2.0 * h));
        assert!(close(q.dib.0, (e1.ib - e2.ib) / (2.0 * h)), "dib/dvbe at {vbe},{vbc}");
        assert!(close(q.dic.1, (f1.ic - f2.ic) / (2.0 * h)), "dic/dvbc at {vbe},{vbc}: {} vs {}", q.dic.1, (f1.ic - f2.ic) / (2.0 * h));
        assert!(close(q.dib.1, (f1.ib - f2.ib) / (2.0 * h)), "dib/dvbc at {vbe},{vbc}");
    }
}

#[test]
fn high_injection_divides_the_transport_current() {
    // vbc = 0, no Early effect: ic = (if) * 2 / (1 + sqrt(1 + 4 if/ikf)) - ir/br with ir = 0
    let m = BjtModel { ikf: 0.01, ..BjtModel::default() };
    let q = bjt_eval(&m, 0.8, 0.0);
    let iff = m.is * ((0.8 / (m.nf * 0.025865f64)).exp() - 1.0);
    let want = iff * 2.0 / (1.0 + (1.0 + 4.0 * iff / 0.01).sqrt());
    assert!((q.ic / want - 1.0).abs() < 1e-3, "{} vs {want}", q.ic);
    // far above the knee the collector current grows like sqrt(if), not like if
    let far = bjt_eval(&m, 0.9, 0.0);
    let iff_far = m.is * ((0.9 / (m.nf * 0.025865f64)).exp() - 1.0);
    assert!(far.ic < 0.3 * iff_far, "{} {iff_far}", far.ic);
    // and without IKF it is the full if
    assert!((bjt_eval(&BjtModel::default(), 0.8, 0.0).ic / iff - 1.0).abs() < 1e-3);
}

#[test]
fn leakage_currents_add_to_the_base_current_at_low_bias() {
    let (with, without) = (BjtModel { ise: 1e-13, ne: 1.5, ..BjtModel::default() }, BjtModel::default());
    let (a, b) = (bjt_eval(&with, 0.35, -2.0), bjt_eval(&without, 0.35, -2.0));
    let leak = 1e-13 * ((0.35 / (1.5 * 0.025865f64)).exp() - 1.0);
    assert!(((a.ib - b.ib) / leak - 1.0).abs() < 1e-3, "{} {leak}", a.ib - b.ib);
    // the collector current is untouched by the base-emitter leakage
    assert!((a.ic / b.ic - 1.0).abs() < 1e-12);
}

#[test]
fn diode_recombination_adds_current_below_the_knee() {
    let m = DiodeModel { is: 1e-14, isr: 1e-12, nr: 2.0, ..DiodeModel::default() };
    let mut c = Circuit::new();
    let a = c.node("a");
    c.add(Element::Diode { name: "D1".into(), a, k: 0, model: m });
    c.add(Element::vdc("V1", a, 0, 0.15));
    let i = -dc_op(&c).unwrap().element("V1").unwrap().current;
    let want = 1e-14 * ((0.15 / 0.025865f64).exp() - 1.0) + 1e-12 * ((0.15 / (2.0 * 0.025865f64)).exp() - 1.0);
    assert!((i / want - 1.0).abs() < 2e-2, "{i} vs {want}");
    assert!(i > 3.0 * 1e-14 * ((0.15 / 0.025865f64).exp() - 1.0), "recombination should dominate here");
}

#[test]
fn the_temperature_parameters_come_from_the_model() {
    let at = |eg: f64, xti: f64, t: f64| {
        let mut c = Circuit::new();
        let a = c.node("a");
        c.add(Element::Diode { name: "D1".into(), a, k: 0, model: DiodeModel { eg, xti, ..DiodeModel::default() } });
        match &c.at_temperature(t).elements[0] {
            Element::Diode { model, .. } => model.is,
            _ => unreachable!(),
        }
    };
    // silicon vs Schottky: the saturation current rises much faster with temperature for the larger band gap
    let (si, schottky) = (at(1.11, 3.0, 100.0), at(0.69, 2.0, 100.0));
    assert!(si > 5.0 * schottky, "{si} {schottky}");
    // the formula itself
    let ratio: f64 = (100.0 + 273.15) / (27.0 + 273.15);
    let want = 1e-14 * ratio.powf(3.0) * ((1.11 / (8.617333e-5 * 373.15)) * (ratio - 1.0)).exp();
    assert!((si / want - 1.0).abs() < 1e-9, "{si} {want}");
    // XTB sets how the gain moves
    let gain = |xtb: f64| {
        let mut c = Circuit::new();
        let (b, col) = (c.node("b"), c.node("c"));
        c.add(Element::Bjt { name: "Q1".into(), c: col, b, e: 0, model: BjtModel { bf: 100.0, xtb, ..BjtModel::default() } });
        match &c.at_temperature(100.0).elements[0] {
            Element::Bjt { model, .. } => model.bf,
            _ => unreachable!(),
        }
    };
    assert!((gain(0.0) - 100.0).abs() < 1e-9 && gain(1.5) > 130.0);
}

#[test]
fn mosfet_overlap_capacitances_scale_with_the_width() {
    let text = ".subckt T d g s\nM1 d g s s NM W=10u L=1u M=2\n.model NM NMOS(KP=100u VTO=1 CGSO=1n CGDO=0.5n)\n.ends T";
    let mut c = Circuit::new();
    let (d, g) = (c.node("d"), c.node("g"));
    add_subckt_instance(&mut c, "U1", "T", text, &[d, g, 0]).unwrap();
    let cap = |suffix: &str| c.elements.iter().find_map(|e| match e { Element::Capacitor { name, c, .. } if name.ends_with(suffix) => Some(*c), _ => None });
    // 1 nF/m * 10 um * 2 parallel devices
    assert!((cap(".CGS").unwrap() / 2e-14 - 1.0).abs() < 1e-9, "{:?}", cap(".CGS"));
    assert!((cap(".CGD").unwrap() / 1e-14 - 1.0).abs() < 1e-9, "{:?}", cap(".CGD"));
}

#[test]
fn model_cards_read_the_new_parameters() {
    let m = parse_models(".model Q1 NPN(ISE=1p NE=1.4 ISC=2p NC=1.9 IKF=0.1 IKR=0.05 EG=1.12 XTI=3.2 XTB=1.2)\n.model D1 D(ISR=3p NR=1.8 EG=0.69 XTI=2)").unwrap();
    let q = match &m["q1"] { spice_in::Model::Bjt(b) => b.clone(), _ => panic!() };
    assert_eq!((q.ise, q.ne, q.isc, q.nc, q.ikf, q.ikr, q.eg, q.xti, q.xtb), (1e-12, 1.4, 2e-12, 1.9, 0.1, 0.05, 1.12, 3.2, 1.2));
    let d = match &m["d1"] { spice_in::Model::Diode(d) => d.clone(), _ => panic!() };
    assert_eq!((d.isr, d.nr, d.eg, d.xti), (3e-12, 1.8, 0.69, 2.0));
    assert!(ignored_model_params(".model Q1 NPN(ISE=1p IKF=0.1 NK=0.5 KF=1f)").iter().all(|(_, p)| p == &vec!["NK".to_string(), "KF".to_string()]));
}
