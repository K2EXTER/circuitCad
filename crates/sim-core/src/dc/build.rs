//! Turning a `Circuit` into the compiled form the solver works on.

use super::*;

pub(super) fn vcrit(nvt: f64, is: f64) -> f64 {
    nvt * (nvt / (std::f64::consts::SQRT_2 * is)).ln()
}

pub(crate) fn compile(c: &Circuit) -> Compiled {
    let mut n_nodes = c.names.len();
    let mut n_br = 0;
    let mut n_slots = 0;
    let (mut n_caps, mut n_inds) = (0, 0);
    let mut ce = vec![];
    let mut b_exprs: Vec<(usize, &crate::expr::Expr)> = vec![];
    // branch unknowns are numbered after all node unknowns; node count is fixed after the loop,
    // so keep branch ordinals and offset them afterwards
    let mut internal = 0;
    for e in &c.elements {
        match e {
            Element::Diode { model, .. } if model.rs > 0.0 => internal += 1,
            Element::Bjt { model, .. } if model.rb > 0.0 => internal += 1,
            _ => {}
        }
    }
    let total_nodes = n_nodes + internal;
    let br_base = total_nodes - 1;
    for e in &c.elements {
        match e {
            Element::Resistor { name, a, b, r } => ce.push(Ce::R { name: name.clone(), a: *a, b: *b, g: 1.0 / r }),
            Element::Capacitor { name, a, b, c } => {
                ce.push(Ce::C { name: name.clone(), a: *a, b: *b, c: *c, idx: n_caps });
                n_caps += 1;
            }
            Element::Inductor { name, a, b, l } => {
                ce.push(Ce::L { name: name.clone(), p: *a, n: *b, l: *l, br: br_base + n_br, idx: n_inds });
                n_br += 1;
                n_inds += 1;
            }
            Element::VSource { name, p, n, wave, ac } => {
                ce.push(Ce::V { name: name.clone(), p: *p, n: *n, wave: wave.clone(), ac: *ac, br: br_base + n_br });
                n_br += 1;
            }
            Element::ISource { name, from, to, wave, ac } => ce.push(Ce::I { name: name.clone(), from: *from, to: *to, wave: wave.clone(), ac: *ac }),
            Element::Vcvs { name, p, n, cp, cn, gain } => {
                ce.push(Ce::E { name: name.clone(), p: *p, n: *n, cp: *cp, cn: *cn, gain: *gain, br: br_base + n_br });
                n_br += 1;
            }
            Element::Vccs { name, p, n, cp, cn, gm } => {
                ce.push(Ce::G { name: name.clone(), p: *p, n: *n, cp: *cp, cn: *cn, gm: *gm })
            }
            Element::Diode { name, a, k, model } => {
                let ai = if model.rs > 0.0 {
                    n_nodes += 1;
                    n_nodes - 1
                } else {
                    *a
                };
                let nvt = model.n * VT;
                ce.push(Ce::D { name: name.clone(), a: *a, ai, k: *k, rs: model.rs, is: model.is, nvt, vcrit: vcrit(nvt, model.is), bv: model.bv, ibv: model.ibv, slot: n_slots });
                n_slots += 1;
            }
            Element::Mosfet { name, d, g, s, model } => ce.push(Ce::M { name: name.clone(), d: *d, g: *g, s: *s, m: model.clone() }),
            Element::Switch { name, a, b, cp, cn, model } => ce.push(Ce::S { name: name.clone(), a: *a, b: *b, cp: *cp, cn: *cn, m: model.clone() }),
            Element::BSource { name, p, n, voltage, expr } => {
                let br = if *voltage {
                    n_br += 1;
                    br_base + n_br - 1
                } else {
                    0
                };
                ce.push(Ce::B { name: name.clone(), p: *p, n: *n, voltage: *voltage, br, c: crate::expr::CExpr::empty() });
                b_exprs.push((ce.len() - 1, expr));
            }
            Element::Bjt { name, c, b, e, model } => {
                let bi = if model.rb > 0.0 {
                    n_nodes += 1;
                    n_nodes - 1
                } else {
                    *b
                };
                ce.push(Ce::Q { name: name.clone(), c: *c, b: *b, bi, e: *e, m: model.clone(), slot: n_slots });
                n_slots += 2;
            }
        }
    }
    // currents of V sources, inductors, E sources and voltage-type B sources can be read by an expression
    let branch_of: Vec<(String, usize)> = ce
        .iter()
        .filter_map(|e| match e {
            Ce::V { name, br, .. } | Ce::L { name, br, .. } | Ce::E { name, br, .. } => Some((name.clone(), *br)),
            Ce::B { name, voltage: true, br, .. } => Some((name.clone(), *br)),
            _ => None,
        })
        .collect();
    for (at, expr) in b_exprs {
        let compiled = expr.compile(&|nm| branch_of.iter().find(|(n, _)| n.eq_ignore_ascii_case(nm)).map(|(_, b)| *b));
        if let Ce::B { c, .. } = &mut ce[at] {
            *c = compiled;
        }
    }
    let linear = !ce.iter().any(|e| matches!(e, Ce::D { .. } | Ce::Q { .. } | Ce::M { .. } | Ce::S { .. } | Ce::B { .. }));
    let has_b = ce.iter().any(|e| matches!(e, Ce::B { .. }));
    Compiled { node_names: c.names.clone(), n_caps, n_inds, n_nodes, n_unknowns: br_base + n_br, n_slots, linear, has_b, cache: Default::default(), ce }
}

pub(crate) fn v(x: &[f64], node: usize) -> f64 {
    if node == 0 { 0.0 } else { x[node - 1] }
}
