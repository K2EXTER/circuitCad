//! Turning a `Circuit` into the compiled form the solver works on.

use super::*;

pub(super) fn vcrit(nvt: f64, is: f64) -> f64 {
    nvt * (nvt / (std::f64::consts::SQRT_2 * is)).ln()
}

pub(crate) fn compile(c: &Circuit) -> Compiled {
    let mut n_nodes = c.names.len();
    let mut n_br = 0;
    let mut n_slots = 0;
    let (mut n_caps, mut n_inds, mut n_seq) = (0, 0, 0);
    let mut seq_names: Vec<String> = vec![];
    let mut ce = vec![];
    let mut b_exprs: Vec<(usize, &crate::expr::Expr)> = vec![];
    // branch unknowns are numbered after all node unknowns; node count is fixed after the loop,
    // so keep branch ordinals and offset them afterwards
    let mut internal = 0;
    for e in &c.elements {
        match e {
            Element::Diode { model, .. } if model.rs > 0.0 => internal += 1,
            Element::Bjt { model, .. } => internal += [model.rb, model.rc, model.re].iter().filter(|r| **r > 0.0).count(),
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
            Element::ChargeCap { name, a, b, q } => {
                // two slots: the voltage across it (for the step-size control) and its charge (the state)
                ce.push(Ce::Cq { name: name.clone(), a: *a, b: *b, c: crate::expr::CExpr::empty(), idx: n_caps });
                n_caps += 2;
                b_exprs.push((ce.len() - 1, q));
            }
            Element::Inductor { name, a, b, l } => {
                let i0 = c.saturations.iter().find(|(n, _)| n == name).map_or(0.0, |(_, i)| *i);
                ce.push(Ce::L { name: name.clone(), p: *a, n: *b, l: *l, br: br_base + n_br, idx: n_inds, i0 });
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
            // coupled inductors are compiled after all the inductors are known (below)
            Element::Mutual { .. } => {}
            Element::Seq { name, kind, ins, q, qb, levels } => {
                seq_names.push(name.clone());
                // one branch per used output; `usize::MAX` marks an output left unconnected
                let mut take = |node: usize| {
                    if node == 0 {
                        usize::MAX
                    } else {
                        n_br += 1;
                        br_base + n_br - 1
                    }
                };
                let (br_q, br_qb) = (take(*q), take(*qb));
                ce.push(Ce::Seq { kind: *kind, ins: ins.clone(), q: *q, qb: *qb, br_q, br_qb, idx: n_seq, lv: *levels });
                n_seq += 1;
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
                let cap = model.has_charge().then(|| {
                    n_caps += 1;
                    n_caps - 1
                });
                ce.push(Ce::D { name: name.clone(), a: *a, ai, k: *k, rs: model.rs, is: model.is, nvt, vcrit: vcrit(nvt, model.is), bv: model.bv, ibv: model.ibv, slot: n_slots, dm: model.clone(), cap });
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
                let mut internal_node = |r: f64, outer: usize| {
                    if r > 0.0 {
                        n_nodes += 1;
                        n_nodes - 1
                    } else {
                        outer
                    }
                };
                let bi = internal_node(model.rb, *b);
                let ci = internal_node(model.rc, *c);
                let ei = internal_node(model.re, *e);
                // a transistor with charge storage gets two integrator slots: base-emitter, then base-collector
                let cap = model.has_charge().then(|| {
                    n_caps += 2;
                    n_caps - 2
                });
                ce.push(Ce::Q { name: name.clone(), c: *c, b: *b, bi, e: *e, ci, ei, m: model.clone(), slot: n_slots, cap });
                n_slots += 2;
            }
        }
    }
    // coupled inductors: the mutual inductance is k * sqrt(L1 L2) between the two branches
    for e in &c.elements {
        if let Element::Mutual { l1, l2, k, .. } = e {
            let find = |nm: &str| ce.iter().find_map(|x| match x {
                Ce::L { name, br, idx, l, .. } if name.eq_ignore_ascii_case(nm) => Some((*br, *idx, *l)),
                _ => None,
            });
            if let (Some((br1, idx1, la)), Some((br2, idx2, lb))) = (find(l1), find(l2)) {
                ce.push(Ce::K { br1, br2, idx1, idx2, m: k * (la * lb).sqrt() });
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
        let compiled = expr.compile(&|nm| branch_of.iter().find(|(n, _)| crate::expr::canon_branch(n) == crate::expr::canon_branch(nm)).map(|(_, b)| *b));
        if let Ce::B { c, .. } | Ce::Cq { c, .. } = &mut ce[at] {
            *c = compiled;
        }
    }
    let linear = !ce.iter().any(|e| matches!(e, Ce::D { .. } | Ce::Q { .. } | Ce::M { .. } | Ce::S { .. } | Ce::B { .. } | Ce::Cq { .. }) || matches!(e, Ce::L { i0, .. } if *i0 > 0.0));
    let has_b = ce.iter().any(|e| matches!(e, Ce::B { .. }));
    // the stored states start at 0, unless the circuit says otherwise (`Circuit::initial` holds 1 / 0 under the element's name)
    let seq_state = std::cell::RefCell::new(seq_names.iter().map(|n| c.initial.iter().find(|(m, _)| m == n).map_or(0.0, |(_, v)| if *v > 0.5 { 1.0 } else { 0.0 })).collect::<Vec<f64>>());
    let seed = if c.nodesets.is_empty() {
        vec![]
    } else {
        let mut s = vec![0.0; n_nodes.saturating_sub(1)];
        for (name, volts) in &c.nodesets {
            if let Some(i) = c.names.iter().position(|n| n.eq_ignore_ascii_case(name)).filter(|i| *i > 0) {
                s[i - 1] = *volts;
            }
        }
        s
    };
    Compiled { seed, seq_state, seq_names, node_names: c.names.clone(), n_caps, n_inds, n_nodes, n_unknowns: br_base + n_br, n_slots, linear, has_b, cache: Default::default(), plan: Default::default(), ce }
}

pub(crate) fn v(x: &[f64], node: usize) -> f64 {
    if node == 0 { 0.0 } else { x[node - 1] }
}
