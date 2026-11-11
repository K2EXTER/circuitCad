//! The matrix the Newton iteration solves with must be the derivative of the circuit equations. Every element is stamped as
//! `A(x) x = b(x)`, so the residual is `F(x) = A(x) x - b(x)` and its Jacobian is `A(x)` itself; a wrong derivative in a device
//! model does not change the answer, only how fast the iteration gets there (quadratic turns linear), which no answer test sees.
//! Each column of `A` is compared here with a central difference of `F`, for every kind of nonlinear element, in DC and in both
//! integration rules of the transient.

use crate::dc::{Ctx, Dyn, compile, small_signal_op};
use crate::expr::Expr;
use crate::*;

/// The residual `A(x) x - b(x)` and the matrix (dense, row-major) at `x`, with the junction state `vj` (copied, so that every
/// evaluation sees the same one).
fn residual(cm: &crate::dc::Compiled, x: &[f64], vj: &[f64], ctx: &Ctx) -> (Vec<f64>, Vec<f64>, bool) {
    let mut vj = vj.to_vec();
    let (m, limited) = cm.assemble(&mut vj, x, ctx);
    let a = m.dense();
    let n = m.n;
    let f = (0..n).map(|r| (0..n).map(|c| a[r * n + c] * x[c]).sum::<f64>() - m.b[r]).collect();
    (f, a, limited)
}

/// Compare the stamped matrix with the difference quotient of the residual, column by column; returns the mismatches.
fn mismatches(c: &Circuit, x: &[f64], ctx: &Ctx) -> Vec<String> {
    let cm = compile(c);
    // let junction limiting walk up to `x`, so that the evaluations below are the unlimited equations
    let mut vj = cm.zero_junction_state();
    for _ in 0..1000 {
        if !cm.assemble(&mut vj, x, ctx).1 {
            break;
        }
    }
    let (_, a, limited) = residual(&cm, x, &vj, ctx);
    assert!(!limited, "the junction state did not settle at the test point");
    let n = cm.n_unknowns;
    let mut bad = vec![];
    for col in 0..n {
        let h = 1e-6 * x[col].abs().max(1.0);
        let (mut xp, mut xm) = (x.to_vec(), x.to_vec());
        xp[col] += h;
        xm[col] -= h;
        let (fp, _, lp) = residual(&cm, &xp, &vj, ctx);
        let (fm, _, lm) = residual(&cm, &xm, &vj, ctx);
        assert!(!lp && !lm, "a step of {h} was limited");
        for row in 0..n {
            let fd = (fp[row] - fm[row]) / (2.0 * h);
            let st = a[row * n + col];
            let scale = (0..n).map(|k| a[row * n + k].abs()).fold(0.0, f64::max);
            if (fd - st).abs() > 1e-4 * fd.abs().max(st.abs()) + 1e-7 * scale + 1e-12 {
                // circuit nodes by name; internal nodes and branch currents by their index
                let name = |i: usize| cm.node_names.get(i + 1).cloned().unwrap_or_else(|| format!("#{i}"));
                bad.push(format!("d(row {})/d({}): stamped {st:.6e}, difference {fd:.6e}", name(row), name(col)));
            }
        }
    }
    bad
}

/// The DC solution moved a little (so that no device sits on a symmetric point), as the point to check.
fn test_point(c: &Circuit) -> Vec<f64> {
    let cm = compile(c);
    let (att, _) = small_signal_op(&cm).expect("the test circuit should solve");
    att.x.iter().enumerate().map(|(i, v)| v + 0.003 * ((i * 7) % 5) as f64 - 0.006).collect()
}

fn check_dc(c: &Circuit) {
    let x = test_point(c);
    let bad = mismatches(c, &x, &Ctx::dc(1e-12, 1.0));
    assert!(bad.is_empty(), "DC:\n{}", bad.join("\n"));
}

fn check_tran(c: &Circuit) {
    let x = test_point(c);
    let cm = compile(c);
    // the previous point at rest: the history terms are constants and do not enter the derivative, and a large one (a stored
    // charge divided by a 10 ns step) would only drown the difference quotient in rounding
    let cap_v = vec![0.0; cm.n_caps];
    let cap_i = vec![0.0; cm.n_caps];
    let ind_i = vec![0.0; cm.n_inds];
    let ind_v = vec![0.0; cm.n_inds];
    for trap in [false, true] {
        let ctx = Ctx { gmin: 1e-12, scale: 1.0, t: 1e-6, dynamic: Some(Dyn { h: 1e-8, trap, cap_v: &cap_v, cap_i: &cap_i, ind_i: &ind_i, ind_v: &ind_v, capped: false }), sw_widen: 1.0, seed_g: None };
        let bad = mismatches(c, &x, &ctx);
        assert!(bad.is_empty(), "transient (trap = {trap}):\n{}", bad.join("\n"));
    }
}

fn check(c: &Circuit) {
    check_dc(c);
    check_tran(c);
}

fn expr(c: &mut Circuit, text: &str) -> Expr {
    Expr::parse(text, &mut |n| c.node(n), &|n| n.to_string()).unwrap()
}

#[test]
fn diodes_forward_reverse_and_in_breakdown() {
    let mut c = Circuit::new();
    let (s, a, b, k, z) = (c.node("s"), c.node("a"), c.node("b"), c.node("k"), c.node("z"));
    c.add(Element::vdc("V1", s, 0, 5.0));
    // plain, then with series resistance, recombination current and both kinds of stored charge
    c.add(Element::Resistor { name: "R1".into(), a: s, b: a, r: 2e3 });
    c.add(Element::Diode { name: "D1".into(), a, k: 0, model: DiodeModel::default() });
    c.add(Element::Resistor { name: "R2".into(), a: s, b, r: 500.0 });
    c.add(Element::Diode { name: "D2".into(), a: b, k: 0, model: DiodeModel { rs: 5.0, isr: 1e-11, cj0: 4e-12, tt: 5e-9, ..DiodeModel::default() } });
    // reverse biased with junction capacitance
    c.add(Element::Resistor { name: "R3".into(), a: s, b: k, r: 1e3 });
    c.add(Element::Diode { name: "D3".into(), a: 0, k, model: DiodeModel { cj0: 10e-12, m: 0.4, ..DiodeModel::default() } });
    // a 3.3 V zener carrying a few milliamps
    c.add(Element::Resistor { name: "R4".into(), a: s, b: z, r: 470.0 });
    c.add(Element::Diode { name: "D4".into(), a: 0, k: z, model: DiodeModel { bv: 3.3, ibv: 1e-3, cj0: 50e-12, ..DiodeModel::default() } });
    check(&c);
}

fn rich(pnp: bool) -> BjtModel {
    BjtModel {
        is: 1e-15,
        bf: 180.0,
        br: 3.0,
        rb: 20.0,
        rc: 1.5,
        re: 0.4,
        vaf: 90.0,
        var: 25.0,
        cje: 20e-12,
        cjc: 8e-12,
        tf: 0.4e-9,
        tr: 20e-9,
        ise: 1e-14,
        isc: 1e-13,
        ikf: 0.08,
        ikr: 0.01,
        pnp,
        ..BjtModel::default()
    }
}

#[test]
fn transistors_in_every_region_with_every_effect_on() {
    let mut c = Circuit::new();
    let vcc = c.node("vcc");
    c.add(Element::vdc("V1", vcc, 0, 9.0));
    // NPN in the active region
    let (b1, c1, e1) = (c.node("b1"), c.node("c1"), c.node("e1"));
    c.add(Element::Resistor { name: "RB1".into(), a: vcc, b: b1, r: 220e3 });
    c.add(Element::Resistor { name: "RC1".into(), a: vcc, b: c1, r: 2.2e3 });
    c.add(Element::Resistor { name: "RE1".into(), a: e1, b: 0, r: 100.0 });
    c.add(Element::Bjt { name: "Q1".into(), c: c1, b: b1, e: e1, model: rich(false) });
    // PNP driven hard into saturation
    let (b2, c2) = (c.node("b2"), c.node("c2"));
    c.add(Element::Resistor { name: "RB2".into(), a: b2, b: 0, r: 4.7e3 });
    c.add(Element::Resistor { name: "RC2".into(), a: c2, b: 0, r: 1e3 });
    c.add(Element::Bjt { name: "Q2".into(), c: c2, b: b2, e: vcc, model: rich(true) });
    // NPN used backwards (collector and emitter swapped)
    let (b3, c3) = (c.node("b3"), c.node("c3"));
    c.add(Element::Resistor { name: "RB3".into(), a: vcc, b: b3, r: 100e3 });
    c.add(Element::Resistor { name: "RC3".into(), a: vcc, b: c3, r: 1e3 });
    c.add(Element::Bjt { name: "Q3".into(), c: 0, b: b3, e: c3, model: rich(false) });
    check(&c);
}

#[test]
fn mosfets_and_jfets_in_every_region() {
    let mut c = Circuit::new();
    let (vdd, g) = (c.node("vdd"), c.node("g"));
    c.add(Element::vdc("V1", vdd, 0, 5.0));
    c.add(Element::vdc("VG", g, 0, 2.5));
    let m = |pmos: bool| MosModel { kp: 0.02, vto: if pmos { -1.2 } else { 1.2 }, lambda: 0.02, pmos, cgs: 300e-12, cgd: 80e-12, rd: 0.3, rs: 0.1, ..MosModel::default() };
    // saturation (a large drain resistor would put it in the linear region: this one keeps vds above vov)
    let d1 = c.node("d1");
    c.add(Element::Resistor { name: "RD1".into(), a: vdd, b: d1, r: 100.0 });
    c.add_mosfet("M1", d1, g, 0, m(false));
    // linear region
    let d2 = c.node("d2");
    c.add(Element::Resistor { name: "RD2".into(), a: vdd, b: d2, r: 1e3 });
    c.add_mosfet("M2", d2, g, 0, m(false));
    // drain below source (the device runs reversed)
    let (d3, s3) = (c.node("d3"), c.node("s3"));
    c.add(Element::vdc("VS3", s3, 0, 0.3));
    c.add(Element::Resistor { name: "RD3".into(), a: d3, b: 0, r: 1e3 });
    c.add_mosfet("M3", d3, vdd, s3, m(false));
    // PMOS
    let d4 = c.node("d4");
    c.add(Element::Resistor { name: "RD4".into(), a: d4, b: 0, r: 200.0 });
    c.add_mosfet("M4", d4, g, vdd, m(true));
    // n-channel JFET with a source resistor (self-biased)
    let (d5, s5) = (c.node("d5"), c.node("s5"));
    c.add(Element::Resistor { name: "RD5".into(), a: vdd, b: d5, r: 1e3 });
    c.add(Element::Resistor { name: "RS5".into(), a: s5, b: 0, r: 200.0 });
    c.add_mosfet("J1", d5, 0, s5, MosModel { kp: 2e-3, vto: -2.0, lambda: 0.01, jfet: true, ..MosModel::default() });
    check(&c);
}

#[test]
fn switches_behavioural_sources_and_nonlinear_storage() {
    let mut c = Circuit::new();
    let (s, a, cc, o, q, w) = (c.node("s"), c.node("a"), c.node("cc"), c.node("o"), c.node("q"), c.node("w"));
    c.add(Element::vdc("V1", s, 0, 3.0));
    // a switch half way through its transition, both orientations
    c.add(Element::vdc("VC", cc, 0, 1.02));
    c.add(Element::Resistor { name: "R1".into(), a: s, b: a, r: 1e3 });
    c.add(Element::Switch { name: "S1".into(), a, b: 0, cp: cc, cn: 0, model: SwitchModel { ron: 10.0, roff: 1e6, vt: 1.0, vw: 0.1 } });
    c.add(Element::Switch { name: "S2".into(), a, b: s, cp: cc, cn: 0, model: SwitchModel { ron: 50.0, roff: 1e6, vt: 1.0, vw: -0.1 } });
    // behavioural sources: a current that depends on two nodes and a branch current, a voltage from a smooth function
    let i = expr(&mut c, "1m*tanh(V(a)-0.5)*V(s)+0.01*I(V1)");
    c.add(Element::BSource { name: "B1".into(), p: o, n: 0, voltage: false, expr: i });
    c.add(Element::Resistor { name: "R2".into(), a: o, b: 0, r: 1e3 });
    let v = expr(&mut c, "2*exp(-V(o))+V(a)*V(a)");
    c.add(Element::BSource { name: "B2".into(), p: w, n: 0, voltage: true, expr: v });
    c.add(Element::Resistor { name: "R3".into(), a: w, b: q, r: 1e3 });
    // a charge that is not proportional to the voltage, a saturating inductor coupled to a plain one
    let charge = expr(&mut c, "1n*(V(q)+0.2*V(q)*V(q))");
    c.add(Element::ChargeCap { name: "C1".into(), a: q, b: 0, q: charge });
    let (l1, l2) = (c.node("l1"), c.node("l2"));
    c.add(Element::Resistor { name: "R4".into(), a: s, b: l1, r: 100.0 });
    c.add(Element::Inductor { name: "L1".into(), a: l1, b: 0, l: 1e-3 });
    c.set_saturation("L1", 0.01);
    c.add(Element::Resistor { name: "R5".into(), a: l2, b: 0, r: 50.0 });
    c.add(Element::Inductor { name: "L2".into(), a: l2, b: 0, l: 2e-3 });
    c.add(Element::Mutual { name: "K1".into(), l1: "L1".into(), l2: "L2".into(), k: 0.9 });
    c.add(Element::Resistor { name: "R6".into(), a: q, b: 0, r: 10e3 });
    check(&c);
}
