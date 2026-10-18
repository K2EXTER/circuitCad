//! Modified nodal analysis: the matrix, the integration context and the stamping of every element.

use super::*;

pub(crate) struct Mna {
    pub(crate) n: usize,
    /// Sparse rows of (column, value). Stamps are appended unsorted; `compress` sorts each row
    /// and sums duplicate entries.
    pub(crate) rows: Vec<Vec<(usize, f64)>>,
    pub(crate) b: Vec<f64>,
    /// Only the right-hand side is wanted (the factorisation of the matrix is cached).
    rhs_only: bool,
}

impl Mna {
    pub(super) fn new(n: usize, rhs_only: bool) -> Self {
        let rows = if rhs_only { Vec::new() } else { vec![Vec::new(); n] };
        Self { n, rows, b: vec![0.0; n], rhs_only }
    }
    /// A[row node][col node] += val (ground rows/cols are dropped).
    pub(super) fn gn(&mut self, r: usize, c: usize, val: f64) {
        if r != 0 && c != 0 && !self.rhs_only {
            self.rows[r - 1].push((c - 1, val));
        }
    }
    pub(super) fn at(&mut self, r: usize, c: usize, val: f64) {
        if !self.rhs_only {
            self.rows[r].push((c, val));
        }
    }
    pub(super) fn g(&mut self, p: usize, q: usize, g: f64) {
        self.gn(p, p, g);
        self.gn(q, q, g);
        self.gn(p, q, -g);
        self.gn(q, p, -g);
    }
    /// A current `i` flows from node `p` through an element to node `q`.
    pub(super) fn current(&mut self, p: usize, q: usize, i: f64) {
        if p != 0 {
            self.b[p - 1] -= i;
        }
        if q != 0 {
            self.b[q - 1] += i;
        }
    }
    /// Branch row for a voltage-defined element between p and n.
    pub(super) fn branch(&mut self, p: usize, n: usize, br: usize) {
        if p != 0 {
            self.at(p - 1, br, 1.0);
            self.at(br, p - 1, 1.0);
        }
        if n != 0 {
            self.at(n - 1, br, -1.0);
            self.at(br, n - 1, -1.0);
        }
    }
    /// Sort every row by column and merge duplicate entries.
    pub(super) fn compress(&mut self) {
        for row in &mut self.rows {
            row.sort_unstable_by_key(|e| e.0);
            let mut w = 0;
            for r in 0..row.len() {
                if w > 0 && row[w - 1].0 == row[r].0 {
                    row[w - 1].1 += row[r].1;
                } else {
                    row[w] = row[r];
                    w += 1;
                }
            }
            row.truncate(w);
        }
    }
    /// Row-major dense copy.
    pub(crate) fn dense(&self) -> Vec<f64> {
        let mut a = vec![0.0; self.n * self.n];
        for (r, row) in self.rows.iter().enumerate() {
            for &(c, v) in row {
                a[r * self.n + c] = v;
            }
        }
        a
    }
}

/// Integration state for one transient step: values at the previous time point.
pub(crate) struct Dyn<'a> {
    pub h: f64,
    /// Trapezoidal rule when true, backward Euler otherwise.
    pub trap: bool,
    pub cap_v: &'a [f64],
    pub cap_i: &'a [f64],
    pub ind_i: &'a [f64],
    pub ind_v: &'a [f64],
}

/// Everything that varies between Newton solves.
pub(crate) struct Ctx<'a> {
    pub gmin: f64,
    /// Scale for independent sources (source stepping).
    pub scale: f64,
    /// Time of the solution being computed (transient only).
    pub t: f64,
    pub dynamic: Option<Dyn<'a>>,
    /// Factor by which switch transitions are widened (1 = as modelled).
    pub sw_widen: f64,
}

impl Ctx<'_> {
    pub(super) fn source(&self, w: &Waveform) -> f64 {
        if self.dynamic.is_some() { w.eval(self.t) } else { w.dc_value() }
    }

    pub fn dc(gmin: f64, scale: f64) -> Ctx<'static> {
        Ctx { gmin, scale, t: 0.0, dynamic: None, sw_widen: 1.0 }
    }
}

impl Compiled {
    /// Build the linearised system around `x` (with junction limiting through `vj`).
    /// Returns the matrix and whether any junction voltage had to be limited.
    pub(crate) fn assemble(&self, vj: &mut [f64], x: &[f64], ctx: &Ctx) -> (Mna, bool) {
        self.assemble_with(vj, x, ctx, false)
    }

    pub(super) fn assemble_with(&self, vj: &mut [f64], x: &[f64], ctx: &Ctx, rhs_only: bool) -> (Mna, bool) {
        let (gmin, scale) = (ctx.gmin, ctx.scale);
        let mut m = Mna::new(self.n_unknowns, rhs_only);
        let mut limited = false;
        for node in 1..self.n_nodes {
            m.at(node - 1, node - 1, gmin);
        }
        for e in &self.ce {
            match e {
                Ce::R { a, b, g, .. } => m.g(*a, *b, *g),
                Ce::C { a, b, c, idx, .. } => {
                    // open circuit at DC; companion model (conductance + history current) in transient
                    if let Some(d) = &ctx.dynamic {
                        let geq = if d.trap { 2.0 * c / d.h } else { c / d.h };
                        let hist = geq * d.cap_v[*idx] + if d.trap { d.cap_i[*idx] } else { 0.0 };
                        m.g(*a, *b, geq);
                        m.current(*a, *b, -hist);
                    }
                }
                Ce::V { p, n, wave, br, .. } => {
                    m.branch(*p, *n, *br);
                    m.b[*br] += ctx.source(wave) * scale;
                }
                Ce::L { p, n, l, br, idx, i0, .. } => {
                    // short circuit at DC; v = req*(i - i_old) [- v_old] in transient
                    m.branch(*p, *n, *br);
                    if let Some(d) = &ctx.dynamic {
                        if *i0 > 0.0 {
                            // saturating: v = d(flux)/dt with flux = l*i0*atan(i/i0), linearised around the iterate i_k
                            let ik = x[*br];
                            let ld = l / (1.0 + (ik / i0) * (ik / i0));
                            let flux = |i: f64| l * i0 * (i / i0).atan();
                            let k = if d.trap { 2.0 } else { 1.0 };
                            m.at(*br, *br, -k * ld / d.h);
                            m.b[*br] += k * (flux(ik) - ld * ik - flux(d.ind_i[*idx])) / d.h - if d.trap { d.ind_v[*idx] } else { 0.0 };
                        } else {
                            let req = if d.trap { 2.0 * l / d.h } else { l / d.h };
                            m.at(*br, *br, -req);
                            m.b[*br] += -req * d.ind_i[*idx] - if d.trap { d.ind_v[*idx] } else { 0.0 };
                        }
                    }
                }
                Ce::Seq { q, qb, br_q, br_qb, idx, lv, .. } => {
                    // the outputs sit at the level of the stored state: the state itself is not an unknown, so nothing loops
                    let s = self.seq_state.borrow().get(*idx).copied().unwrap_or(0.0);
                    if *br_q != usize::MAX {
                        m.branch(*q, 0, *br_q);
                        m.b[*br_q] = lv.vlow + (lv.vhigh - lv.vlow) * s;
                    }
                    if *br_qb != usize::MAX {
                        m.branch(*qb, 0, *br_qb);
                        m.b[*br_qb] = lv.vlow + (lv.vhigh - lv.vlow) * (1.0 - s);
                    }
                }
                Ce::K { br1, br2, idx1, idx2, m: mutual, .. } => {
                    // v1 = L1 di1/dt + M di2/dt (and the other way round): the companion terms of the other inductor's current
                    if let Some(d) = &ctx.dynamic {
                        let km = if d.trap { 2.0 * mutual / d.h } else { mutual / d.h };
                        m.at(*br1, *br2, -km);
                        m.b[*br1] += -km * d.ind_i[*idx2];
                        m.at(*br2, *br1, -km);
                        m.b[*br2] += -km * d.ind_i[*idx1];
                    }
                }
                Ce::I { from, to, wave, .. } => m.current(*from, *to, ctx.source(wave) * scale),
                Ce::E { p, n, cp, cn, gain, br, .. } => {
                    m.branch(*p, *n, *br);
                    if *cp != 0 {
                        m.at(*br, cp - 1, -gain);
                    }
                    if *cn != 0 {
                        m.at(*br, cn - 1, *gain);
                    }
                }
                Ce::G { p, n, cp, cn, gm, .. } => {
                    m.gn(*p, *cp, *gm);
                    m.gn(*p, *cn, -gm);
                    m.gn(*n, *cp, -gm);
                    m.gn(*n, *cn, *gm);
                }
                Ce::D { a, ai, k, rs, is, nvt, vcrit, bv, ibv, slot, dm, cap, .. } => {
                    if *rs > 0.0 {
                        m.g(*a, *ai, 1.0 / rs);
                    }
                    let raw = v(x, *ai) - v(x, *k);
                    // in breakdown the junction is limited in the mirrored (-v - bv) variable, like SPICE
                    let lim = if bv.is_finite() && raw < (-bv + 10.0 * nvt).min(0.0) {
                        let (rn, ro) = (-(raw + bv), -(vj[*slot] + bv));
                        -(pnjlim(rn, ro, *nvt, *vcrit) + bv)
                    } else {
                        pnjlim(raw, vj[*slot], *nvt, *vcrit)
                    };
                    if (lim - raw).abs() > 1e-12 {
                        limited = true;
                    }
                    vj[*slot] = lim;
                    let ex = exp_safe(lim / nvt);
                    let (mut id, mut gd) = (is * (ex - 1.0), is / nvt * ex + gmin);
                    let (ir_, gr_) = diode_recomb(dm, lim);
                    id += ir_;
                    gd += gr_;
                    if bv.is_finite() {
                        let eb = exp_safe(-(lim + bv) / nvt);
                        id -= ibv * eb;
                        gd += ibv / nvt * eb;
                    }
                    m.g(*ai, *k, gd);
                    m.current(*ai, *k, id - gd * lim);
                    // charge storage in transient: the junction as a capacitor with charge Q(v), linearised around the iterate
                    if let (Some(d), Some(idx)) = (&ctx.dynamic, cap) {
                        let kk = if d.trap { 2.0 } else { 1.0 };
                        let (q, cc) = diode_q(dm, *nvt, lim);
                        let q_old = diode_q(dm, *nvt, d.cap_v[*idx]).0;
                        m.g(*ai, *k, kk * cc / d.h);
                        m.current(*ai, *k, kk / d.h * (q - cc * lim - q_old) - if d.trap { d.cap_i[*idx] } else { 0.0 });
                    }
                }
                Ce::S { a, b, cp, cn, m: sw, .. } => {
                    // i = g(vc) * vab; linearised with the control derivative as a transconductance
                    let (vab, vc) = (v(x, *a) - v(x, *b), v(x, *cp) - v(x, *cn));
                    let (g, dg) = sw.eval_widened(vc, ctx.sw_widen);
                    let k = dg * vab;
                    m.g(*a, *b, g);
                    m.gn(*a, *cp, k);
                    m.gn(*a, *cn, -k);
                    m.gn(*b, *cp, -k);
                    m.gn(*b, *cn, k);
                    // the constant part of the linearisation is -k*vc at both ends
                    if *a != 0 {
                        m.b[*a - 1] += k * vc;
                    }
                    if *b != 0 {
                        m.b[*b - 1] -= k * vc;
                    }
                }
                Ce::M { d, g, s, m: mm, .. } => {
                    let pol = if mm.pmos { -1.0 } else { 1.0 };
                    let (vgs, vds) = (v(x, *g) - v(x, *s), v(x, *d) - v(x, *s));
                    let (idn, gm, gds) = mos_eval(mm, pol * vgs, pol * vds);
                    // derivatives with respect to the real terminal voltages equal the normalised ones
                    let id0 = pol * idn;
                    m.gn(*d, *g, gm);
                    m.gn(*d, *d, gds);
                    m.gn(*d, *s, -(gm + gds));
                    m.gn(*s, *g, -gm);
                    m.gn(*s, *d, -gds);
                    m.gn(*s, *s, gm + gds);
                    let ieq = id0 - gm * vgs - gds * vds;
                    if *d != 0 {
                        m.b[*d - 1] -= ieq;
                    }
                    if *s != 0 {
                        m.b[*s - 1] += ieq;
                    }
                    m.g(*d, *s, gmin);
                    m.g(*g, *s, gmin);
                }
                Ce::B { p, n, voltage, br, c, .. } => {
                    // linearise f(x) around the present solution: f0 + sum g_k (x_k - x0_k)
                    let (f0, grads) = c.eval(x, if ctx.dynamic.is_some() { ctx.t } else { 0.0 });
                    let col = |var: &crate::expr::Var| match var {
                        crate::expr::Var::Node(k) => *k - 1,
                        crate::expr::Var::Unknown(i) => *i,
                    };
                    let value = |var: &crate::expr::Var| match var {
                        crate::expr::Var::Node(k) => v(x, *k),
                        crate::expr::Var::Unknown(i) => x[*i],
                    };
                    let rest: f64 = f0 - c.vars.iter().zip(&grads).map(|(var, g)| g * value(var)).sum::<f64>();
                    if *voltage {
                        m.branch(*p, *n, *br);
                        for (var, g) in c.vars.iter().zip(&grads) {
                            m.at(*br, col(var), -g);
                        }
                        m.b[*br] += rest;
                    } else {
                        for (var, g) in c.vars.iter().zip(&grads) {
                            if *p != 0 {
                                m.at(*p - 1, col(var), *g);
                            }
                            if *n != 0 {
                                m.at(*n - 1, col(var), -g);
                            }
                        }
                        m.current(*p, *n, rest);
                    }
                }
                Ce::Q { c: c_out, b, bi, e: e_out, ci, ei, m: bm, slot, cap, .. } => {
                    if bm.rb > 0.0 {
                        m.g(*b, *bi, 1.0 / bm.rb);
                    }
                    if bm.rc > 0.0 {
                        m.g(*c_out, *ci, 1.0 / bm.rc);
                    }
                    if bm.re > 0.0 {
                        m.g(*e_out, *ei, 1.0 / bm.re);
                    }
                    // the junctions sit on the internal base, collector and emitter
                    let (b, c, e) = (bi, ci, ei);
                    let pol = if bm.pnp { -1.0 } else { 1.0 };
                    let crit_be = vcrit(bm.nf * VT, bm.is);
                    let crit_bc = vcrit(bm.nr * VT, bm.is);
                    let raw_be = pol * (v(x, *b) - v(x, *e));
                    let raw_bc = pol * (v(x, *b) - v(x, *c));
                    let lim_be = pnjlim(raw_be, vj[*slot], bm.nf * VT, crit_be);
                    let lim_bc = pnjlim(raw_bc, vj[*slot + 1], bm.nr * VT, crit_bc);
                    if (lim_be - raw_be).abs() > 1e-12 || (lim_bc - raw_bc).abs() > 1e-12 {
                        limited = true;
                    }
                    vj[*slot] = lim_be;
                    vj[*slot + 1] = lim_bc;
                    let q = bjt_eval(bm, lim_be, lim_bc);
                    for (node, i0, (dbe, dbc)) in [(*c, q.ic, q.dic), (*b, q.ib, q.dib), (*e, q.ie, q.die)] {
                        // currents into the terminal; d/dV_b = dbe+dbc, d/dV_e = -dbe, d/dV_c = -dbc
                        m.gn(node, *b, dbe + dbc);
                        m.gn(node, *e, -dbe);
                        m.gn(node, *c, -dbc);
                        if node != 0 {
                            m.b[node - 1] -= pol * i0 - dbe * pol * lim_be - dbc * pol * lim_bc;
                        }
                    }
                    // charge storage in transient: each junction is a capacitor with charge Q(v), linearised around the iterate
                    if let (Some(d), Some(idx)) = (&ctx.dynamic, cap) {
                        let k = if d.trap { 2.0 } else { 1.0 };
                        for (slot_i, other, vk, (q, cc), q_old) in [
                            (*idx, *e, lim_be, bjt_qbe(bm, lim_be), bjt_qbe(bm, d.cap_v[*idx]).0),
                            (*idx + 1, *c, lim_bc, bjt_qbc(bm, lim_bc), bjt_qbc(bm, d.cap_v[*idx + 1]).0),
                        ] {
                            let geq = k * cc / d.h;
                            let rest = k / d.h * (q - cc * vk - q_old) - if d.trap { d.cap_i[slot_i] } else { 0.0 };
                            m.g(*b, other, geq);
                            m.current(*b, other, pol * rest);
                        }
                    }
                    // gmin across both junctions keeps the matrix well conditioned
                    m.g(*b, *e, gmin);
                    m.g(*b, *c, gmin);
                }
            }
        }
        if !rhs_only {
            m.compress();
        }
        (m, limited)
    }
}
