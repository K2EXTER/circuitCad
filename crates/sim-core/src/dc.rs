//! DC operating point: modified nodal analysis + damped Newton-Raphson, with gmin and
//! source stepping as convergence aids.

use crate::circuit::{BjtModel, Circuit, Element, MosModel, SwitchModel};
use crate::waveform::Waveform;
use std::fmt;

const VT: f64 = 0.025852; // kT/q at 300.15 K
pub(crate) const GMIN: f64 = 1e-12;
const RELTOL: f64 = 1e-6;
const VNTOL: f64 = 1e-8;
const ABSTOL: f64 = 1e-12;
const MAX_ITER: usize = 150;

#[derive(Clone, Debug, PartialEq)]
pub enum SimError {
    NoGround,
    /// The matrix cannot be solved: usually voltage sources in a loop / in parallel.
    Singular,
    NoConvergence,
    /// Bad analysis settings.
    Invalid(String),
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimError::NoGround => write!(f, "GNDにつながる部品がありません"),
            SimError::Singular => write!(f, "回路方程式が解けません (電圧源のループや並列接続、コイルと電圧源のループなど)"),
            SimError::NoConvergence => write!(f, "収束しませんでした"),
            SimError::Invalid(m) => write!(f, "{m}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ElementResult {
    pub name: String,
    /// Main current, flowing through the element from its first to its second terminal
    /// (resistor a->b, source p->n, diode a->k, transistor: collector current into C).
    pub current: f64,
    /// Voltage across the element, first minus second terminal (transistor: Vce).
    pub voltage: f64,
    /// Power absorbed (negative = delivered).
    pub power: f64,
    /// Further currents, e.g. transistor `Ib`, `Ie`.
    pub extra: Vec<(String, f64)>,
}

#[derive(Clone, Debug)]
pub struct DcResult {
    /// Voltage of every circuit node (ground included, 0 V).
    pub voltages: Vec<(String, f64)>,
    pub elements: Vec<ElementResult>,
    pub iterations: usize,
    pub strategy: &'static str,
    pub warnings: Vec<String>,
}

impl DcResult {
    pub fn voltage(&self, node: &str) -> Option<f64> {
        self.voltages.iter().find(|(n, _)| n == node).map(|(_, v)| *v)
    }
    pub fn element(&self, name: &str) -> Option<&ElementResult> {
        self.elements.iter().find(|e| e.name == name)
    }
}

// ------------------------------------------------------------------ compiled form

pub(crate) enum Ce {
    R { name: String, a: usize, b: usize, g: f64 },
    C { name: String, a: usize, b: usize, c: f64, idx: usize },
    V { name: String, p: usize, n: usize, wave: Waveform, ac: Option<(f64, f64)>, br: usize },
    L { name: String, p: usize, n: usize, l: f64, br: usize, idx: usize },
    I { name: String, from: usize, to: usize, wave: Waveform, ac: Option<(f64, f64)> },
    E { name: String, p: usize, n: usize, cp: usize, cn: usize, gain: f64, br: usize },
    G { name: String, p: usize, n: usize, cp: usize, cn: usize, gm: f64 },
    D { name: String, a: usize, ai: usize, k: usize, rs: f64, is: f64, nvt: f64, vcrit: f64, bv: f64, ibv: f64, slot: usize },
    /// `bi` is the internal base node (equal to `b` when the model has no `rb`).
    Q { name: String, c: usize, b: usize, bi: usize, e: usize, m: BjtModel, slot: usize },
    M { name: String, d: usize, g: usize, s: usize, m: MosModel },
    S { name: String, a: usize, b: usize, cp: usize, cn: usize, m: SwitchModel },
}

pub(crate) struct Compiled {
    pub(crate) node_names: Vec<String>, // circuit nodes only
    pub(crate) n_caps: usize,
    pub(crate) n_inds: usize,
    n_nodes: usize,          // including ground and internal diode/base nodes
    pub(crate) n_unknowns: usize,
    n_slots: usize,
    pub(crate) ce: Vec<Ce>,
}

fn vcrit(nvt: f64, is: f64) -> f64 {
    nvt * (nvt / (std::f64::consts::SQRT_2 * is)).ln()
}

pub(crate) fn compile(c: &Circuit) -> Compiled {
    let mut n_nodes = c.names.len();
    let mut n_br = 0;
    let mut n_slots = 0;
    let (mut n_caps, mut n_inds) = (0, 0);
    let mut ce = vec![];
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
    Compiled { node_names: c.names.clone(), n_caps, n_inds, n_nodes, n_unknowns: br_base + n_br, n_slots, ce }
}

pub(crate) fn v(x: &[f64], node: usize) -> f64 {
    if node == 0 { 0.0 } else { x[node - 1] }
}

// ------------------------------------------------------------------ device equations

pub(crate) fn exp_safe(x: f64) -> f64 {
    x.min(80.0).exp()
}

/// SPICE3 junction voltage limiting. Returns the limited voltage.
fn pnjlim(vnew: f64, vold: f64, vt: f64, vcrit: f64) -> f64 {
    if vnew > vcrit && (vnew - vold).abs() > 2.0 * vt {
        if vold > 0.0 {
            let arg = (vnew - vold) / vt;
            if arg > 0.0 {
                vold + vt * (2.0 + (arg - 2.0).ln())
            } else {
                vold - vt * (2.0 + (2.0 - arg).ln())
            }
        } else {
            vt * (vnew / vt).ln()
        }
    } else {
        vnew
    }
}

/// Transistor in "NPN space": currents into C, B, E and the partial derivatives
/// with respect to (vbe, vbc).
pub(crate) struct BjtEval {
    pub(crate) ic: f64,
    pub(crate) ib: f64,
    ie: f64,
    dic: (f64, f64),
    dib: (f64, f64),
    die: (f64, f64),
}

pub(crate) fn bjt_eval(m: &BjtModel, vbe: f64, vbc: f64) -> BjtEval {
    let (nfvt, nrvt) = (m.nf * VT, m.nr * VT);
    let ef = exp_safe(vbe / nfvt);
    let er = exp_safe(vbc / nrvt);
    let (iff, ir) = (m.is * (ef - 1.0), m.is * (er - 1.0));
    let (gf, gr) = (m.is / nfvt * ef, m.is / nrvt * er);
    let ic = iff - ir * (1.0 + 1.0 / m.br);
    let ib = iff / m.bf + ir / m.br;
    BjtEval {
        ic,
        ib,
        ie: -(ic + ib),
        dic: (gf, -gr * (1.0 + 1.0 / m.br)),
        dib: (gf / m.bf, gr / m.br),
        die: (-(gf + gf / m.bf), gr * (1.0 + 1.0 / m.br) - gr / m.br),
    }
}

/// Level-1 MOSFET in NMOS orientation: `(id, gm, gds)` with `id` flowing drain to source.
/// The device is symmetric, so for `vds < 0` source and drain swap roles.
pub(crate) fn mos_eval(m: &MosModel, vgs: f64, vds: f64) -> (f64, f64, f64) {
    fn forward(m: &MosModel, vgs: f64, vds: f64) -> (f64, f64, f64) {
        let vov = vgs - m.vto.abs();
        if vov <= 0.0 {
            return (0.0, 0.0, 0.0);
        }
        let clm = 1.0 + m.lambda * vds;
        if vds < vov {
            let id0 = m.kp * (vov * vds - 0.5 * vds * vds);
            (id0 * clm, m.kp * vds * clm, m.kp * (vov - vds) * clm + id0 * m.lambda)
        } else {
            let id0 = 0.5 * m.kp * vov * vov;
            (id0 * clm, m.kp * vov * clm, id0 * m.lambda)
        }
    }
    if vds >= 0.0 {
        forward(m, vgs, vds)
    } else {
        let (id, gm, gds) = forward(m, vgs - vds, -vds);
        (-id, -gm, gm + gds)
    }
}

// ------------------------------------------------------------------ linear algebra

/// Solve A x = b in place (dense, partial pivoting).
fn lu_solve(n: usize, a: &mut [f64], b: &mut [f64]) -> Result<(), SimError> {
    for col in 0..n {
        let (mut piv, mut best) = (col, a[col * n + col].abs());
        for r in col + 1..n {
            if a[r * n + col].abs() > best {
                best = a[r * n + col].abs();
                piv = r;
            }
        }
        if best < 1e-20 || !best.is_finite() {
            return Err(SimError::Singular);
        }
        if piv != col {
            for c in 0..n {
                a.swap(col * n + c, piv * n + c);
            }
            b.swap(col, piv);
        }
        let d = a[col * n + col];
        for r in col + 1..n {
            let f = a[r * n + col] / d;
            if f != 0.0 {
                for c in col..n {
                    a[r * n + c] -= f * a[col * n + c];
                }
                b[r] -= f * b[col];
            }
        }
    }
    for r in (0..n).rev() {
        let mut s = b[r];
        for c in r + 1..n {
            s -= a[r * n + c] * b[c];
        }
        b[r] = s / a[r * n + r];
    }
    Ok(())
}

// ------------------------------------------------------------------ Newton

pub(crate) struct Mna {
    pub(crate) n: usize,
    pub(crate) a: Vec<f64>,
    pub(crate) b: Vec<f64>,
}

impl Mna {
    fn new(n: usize) -> Self {
        Self { n, a: vec![0.0; n * n], b: vec![0.0; n] }
    }
    /// A[row node][col node] += val (ground rows/cols are dropped).
    fn gn(&mut self, r: usize, c: usize, val: f64) {
        if r != 0 && c != 0 {
            self.a[(r - 1) * self.n + (c - 1)] += val;
        }
    }
    fn at(&mut self, r: usize, c: usize, val: f64) {
        self.a[r * self.n + c] += val;
    }
    fn g(&mut self, p: usize, q: usize, g: f64) {
        self.gn(p, p, g);
        self.gn(q, q, g);
        self.gn(p, q, -g);
        self.gn(q, p, -g);
    }
    /// A current `i` flows from node `p` through an element to node `q`.
    fn current(&mut self, p: usize, q: usize, i: f64) {
        if p != 0 {
            self.b[p - 1] -= i;
        }
        if q != 0 {
            self.b[q - 1] += i;
        }
    }
    /// Branch row for a voltage-defined element between p and n.
    fn branch(&mut self, p: usize, n: usize, br: usize) {
        if p != 0 {
            self.at(p - 1, br, 1.0);
            self.at(br, p - 1, 1.0);
        }
        if n != 0 {
            self.at(n - 1, br, -1.0);
            self.at(br, n - 1, -1.0);
        }
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
}

impl Ctx<'_> {
    fn source(&self, w: &Waveform) -> f64 {
        if self.dynamic.is_some() { w.eval(self.t) } else { w.dc_value() }
    }

    pub fn dc(gmin: f64, scale: f64) -> Ctx<'static> {
        Ctx { gmin, scale, t: 0.0, dynamic: None }
    }
}

impl Compiled {
    /// Build the linearised system around `x` (with junction limiting through `vj`).
    /// Returns the matrix and whether any junction voltage had to be limited.
    pub(crate) fn assemble(&self, vj: &mut [f64], x: &[f64], ctx: &Ctx) -> (Mna, bool) {
        let (gmin, scale) = (ctx.gmin, ctx.scale);
        let mut m = Mna::new(self.n_unknowns);
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
                Ce::L { p, n, l, br, idx, .. } => {
                    // short circuit at DC; v = req*(i - i_old) [- v_old] in transient
                    m.branch(*p, *n, *br);
                    if let Some(d) = &ctx.dynamic {
                        let req = if d.trap { 2.0 * l / d.h } else { l / d.h };
                        m.at(*br, *br, -req);
                        m.b[*br] += -req * d.ind_i[*idx] - if d.trap { d.ind_v[*idx] } else { 0.0 };
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
                Ce::D { a, ai, k, rs, is, nvt, vcrit, bv, ibv, slot, .. } => {
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
                    if bv.is_finite() {
                        let eb = exp_safe(-(lim + bv) / nvt);
                        id -= ibv * eb;
                        gd += ibv / nvt * eb;
                    }
                    m.g(*ai, *k, gd);
                    m.current(*ai, *k, id - gd * lim);
                }
                Ce::S { a, b, cp, cn, m: sw, .. } => {
                    // i = g(vc) * vab; linearised with the control derivative as a transconductance
                    let (vab, vc) = (v(x, *a) - v(x, *b), v(x, *cp) - v(x, *cn));
                    let (g, dg) = sw.eval(vc);
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
                Ce::Q { c, b, bi, e, m: bm, slot, .. } => {
                    if bm.rb > 0.0 {
                        m.g(*b, *bi, 1.0 / bm.rb);
                    }
                    let b = bi; // the junctions sit on the internal base
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
                    // gmin across both junctions keeps the matrix well conditioned
                    m.g(*b, *e, gmin);
                    m.g(*b, *c, gmin);
                }
            }
        }
        (m, limited)
    }
}

fn converged(x: &[f64], xn: &[f64], n_node_unknowns: usize) -> bool {
    x.iter().zip(xn).enumerate().all(|(i, (a, b))| {
        let tol = if i < n_node_unknowns { VNTOL } else { ABSTOL };
        (a - b).abs() <= RELTOL * a.abs().max(b.abs()) + tol
    })
}

pub(crate) enum Fail {
    Singular,
    NoConv,
}

pub(crate) struct Attempt {
    pub x: Vec<f64>,
    pub vj: Vec<f64>,
    pub iterations: usize,
}

impl Compiled {
    /// Newton-Raphson. Plain steps first; if that does not converge, again with backtracking.
    pub(crate) fn newton(&self, x0: Vec<f64>, vj0: Vec<f64>, ctx: &Ctx) -> Result<Attempt, Fail> {
        match self.newton_with(x0.clone(), vj0.clone(), ctx, false) {
            Err(Fail::NoConv) => self.newton_with(x0, vj0, ctx, true),
            other => other,
        }
    }

    /// With `backtrack`, a full step that makes the residual of the circuit equations larger is
    /// halved until it helps. A regenerative loop (flip-flop, multivibrator) close to its
    /// switching point has a nearly singular Jacobian, and the full step then overshoots wildly.
    fn newton_with(&self, x0: Vec<f64>, vj0: Vec<f64>, ctx: &Ctx, backtrack: bool) -> Result<Attempt, Fail> {
        let (mut x, mut vj) = (x0, vj0);
        let n_node_unknowns = self.n_nodes - 1;
        // the previous point, its Newton step, its residual, its junction state and the fraction taken
        let mut prev: Option<(Vec<f64>, Vec<f64>, f64, Vec<f64>, f64)> = None;
        for it in 1..=MAX_ITER {
            let (mut m, limited) = self.assemble(&mut vj, &x, ctx);
            let n = m.n;
            let mut r2 = f64::NAN;
            // every equation balances to rounding level (relative to its largest term)
            let mut balanced = false;
            if backtrack {
                let mut sum2 = 0.0;
                balanced = true;
                for i in 0..n {
                    let (mut s, mut mag) = (0.0, m.b[i].abs());
                    for k in 0..n {
                        let t = m.a[i * n + k] * x[k];
                        s += t;
                        mag += t.abs();
                    }
                    let f = s - m.b[i];
                    sum2 += f * f;
                    if f.abs() > 1e-9 * mag + 1e-12 {
                        balanced = false;
                    }
                }
                r2 = sum2;
                // junction limiting changes the equations, so compare only unlimited iterations
                if let (false, Some((xp, dx, rp, vjp, lam))) = (limited, prev.as_mut())
                    && r2 > *rp * 1.0001 && r2 > 1e-20 && *lam > 1e-3 {
                        *lam *= 0.5;
                        x = xp.iter().zip(dx.iter()).map(|(a, d)| a + *lam * d).collect();
                        vj = vjp.clone();
                        continue;
                    }
            }
            let (x_old, vj_old) = if backtrack { (x.clone(), vj.clone()) } else { (vec![], vec![]) };
            lu_solve(m.n, &mut m.a, &mut m.b).map_err(|_| Fail::Singular)?;
            let xn = m.b;
            if xn.iter().any(|v| !v.is_finite()) {
                return Err(Fail::NoConv);
            }
            let done = !limited && it > 1 && converged(&x, &xn, n_node_unknowns);
            // Close to a regenerative switching point the Jacobian is nearly singular: the
            // residual is already at rounding level while a poorly determined direction of x
            // still moves a little. That is as good as the arithmetic can do, so accept it.
            let stalled = backtrack && balanced && !limited && it > 1 && x.iter().zip(&xn).all(|(a, b)| (a - b).abs() < 1e-2);
            if done || stalled {
                return Ok(Attempt { x: xn, vj, iterations: it });
            }
            if backtrack {
                let dx: Vec<f64> = xn.iter().zip(&x_old).map(|(a, b)| a - b).collect();
                prev = Some((x_old, dx, if limited { f64::INFINITY } else { r2 }, vj_old, 1.0));
            }
            x = xn;
        }
        Err(Fail::NoConv)
    }
}

// ------------------------------------------------------------------ public entry

/// Nodes without a DC path to ground (capacitors are open; ideal current sources and
/// controlled-source inputs do not provide one).
fn floating_nodes(c: &Circuit) -> Vec<usize> {
    let n = c.names.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut Vec<usize>, i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        let mut c = i;
        while p[c] != r {
            let nx = p[c];
            p[c] = r;
            c = nx;
        }
        r
    }
    let mut union = |a: usize, b: usize| {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        parent[ra] = rb;
    };
    for e in &c.elements {
        match e {
            Element::Resistor { a, b, .. } | Element::Inductor { a, b, .. } => union(*a, *b),
            Element::VSource { p, n, .. } | Element::Vcvs { p, n, .. } => union(*p, *n),
            Element::Diode { a, k, .. } => union(*a, *k),
            Element::Mosfet { d, s, .. } => union(*d, *s),
            Element::Switch { a, b, .. } => union(*a, *b),
            Element::Bjt { c, b, e, .. } => {
                union(*c, *e);
                union(*b, *e);
            }
            Element::Capacitor { .. } | Element::ISource { .. } | Element::Vccs { .. } => {}
        }
    }
    let g = find(&mut parent, 0);
    (1..n).filter(|&i| find(&mut parent, i) != g).collect()
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Strategy {
    Direct,
    Gmin,
    Source,
}

impl Compiled {
    fn run(&self, which: Strategy) -> Result<Attempt, Fail> {
        let zero_x = vec![0.0; self.n_unknowns];
        let zero_vj = vec![0.0; self.n_slots];
        match which {
            Strategy::Direct => self.newton(zero_x, zero_vj, &Ctx::dc(GMIN, 1.0)),
            Strategy::Gmin => {
                // gmin from 1e-2 down to 1e-12, each step starting from the previous solution
                let (mut x, mut vj, mut iters) = (zero_x, zero_vj, 0);
                for g in (2..=12).map(|e| 10f64.powi(-e)) {
                    let a = self.newton(x, vj, &Ctx::dc(g, 1.0))?;
                    iters += a.iterations;
                    x = a.x;
                    vj = a.vj;
                }
                Ok(Attempt { x, vj, iterations: iters })
            }
            Strategy::Source => {
                // ramp every independent source from 0 to full value
                let (mut x, mut vj, mut iters) = (zero_x, zero_vj, 0);
                for step in 1..=25 {
                    let a = self.newton(x, vj, &Ctx::dc(GMIN, step as f64 / 25.0))?;
                    iters += a.iterations;
                    x = a.x;
                    vj = a.vj;
                }
                Ok(Attempt { x, vj, iterations: iters })
            }
        }
    }
}

pub fn dc_op(circuit: &Circuit) -> Result<DcResult, SimError> {
    dc_op_with(circuit, &[Strategy::Direct, Strategy::Gmin, Strategy::Source])
}

/// Floating-node warnings are produced by the caller; this only needs a ground reference.
pub(crate) fn has_ground(circuit: &Circuit) -> bool {
    circuit.elements.iter().any(|e| match e {
        Element::Resistor { a, b, .. } | Element::Capacitor { a, b, .. } | Element::Inductor { a, b, .. } => *a == 0 || *b == 0,
        Element::VSource { p, n, .. } => *p == 0 || *n == 0,
        Element::ISource { from, to, .. } => *from == 0 || *to == 0,
        Element::Diode { a, k, .. } => *a == 0 || *k == 0,
        Element::Mosfet { d, g, s, .. } => *d == 0 || *g == 0 || *s == 0,
        Element::Switch { a, b, cp, cn, .. } => *a == 0 || *b == 0 || *cp == 0 || *cn == 0,
        Element::Bjt { c, b, e, .. } => *c == 0 || *b == 0 || *e == 0,
        Element::Vcvs { p, n, .. } | Element::Vccs { p, n, .. } => *p == 0 || *n == 0,
    })
}

pub(crate) fn floating_warnings(circuit: &Circuit) -> Vec<String> {
    let mut w = circuit.warnings.clone();
    for i in floating_nodes(circuit) {
        w.push(format!("ノード {} にGNDへのDC経路がありません (0Vとして扱います)", circuit.names[i]));
    }
    w
}

/// Try the strategies in order; the first that converges wins.
pub(crate) fn solve_op(cm: &Compiled, order: &[Strategy]) -> Result<(Attempt, &'static str), SimError> {
    let mut last = SimError::NoConvergence;
    for &which in order {
        match cm.run(which) {
            Ok(att) => {
                let name = match which {
                    Strategy::Direct => "direct",
                    Strategy::Gmin => "gmin stepping",
                    Strategy::Source => "source stepping",
                };
                return Ok((att, name));
            }
            // a structurally singular matrix will not be cured by stepping
            Err(Fail::Singular) => return Err(SimError::Singular),
            Err(Fail::NoConv) => last = SimError::NoConvergence,
        }
    }
    Err(last)
}

pub(crate) fn dc_op_with(circuit: &Circuit, order: &[Strategy]) -> Result<DcResult, SimError> {
    if !has_ground(circuit) {
        return Err(SimError::NoGround);
    }
    let warnings = floating_warnings(circuit);
    let cm = compile(circuit);
    let (att, name) = solve_op(&cm, order)?;
    Ok(report(&cm, &att, name, warnings))
}

fn report(cm: &Compiled, att: &Attempt, strategy: &'static str, warnings: Vec<String>) -> DcResult {
    let x = &att.x;
    let els = cm.element_results(x, None, &vec![0.0; cm.n_caps]);
    let voltages = cm.node_names.iter().enumerate().map(|(i, n)| (n.clone(), v(x, i))).collect();
    DcResult { voltages, elements: els, iterations: att.iterations, strategy, warnings }
}

impl Compiled {
    pub(crate) fn zero_junction_state(&self) -> Vec<f64> {
        vec![0.0; self.n_slots]
    }

    /// Currents, voltages and powers at solution `x`. `t` selects the source values
    /// (`None` = DC); `cap_i` are the capacitor currents (zero at DC).
    pub(crate) fn element_results(&self, x: &[f64], t: Option<f64>, cap_i: &[f64]) -> Vec<ElementResult> {
        let mut els = vec![];
        let mut push = |name: &str, current: f64, voltage: f64, extra: Vec<(String, f64)>, power: f64| {
            els.push(ElementResult { name: name.to_string(), current, voltage, power, extra });
        };
        let src = |w: &Waveform| match t {
            Some(t) => w.eval(t),
            None => w.dc_value(),
        };
        for e in &self.ce {
            match e {
                Ce::R { name, a, b, g } => {
                    let vv = v(x, *a) - v(x, *b);
                    push(name, vv * g, vv, vec![], vv * vv * g);
                }
                Ce::C { name, a, b, idx, .. } => {
                    let (vv, i) = (v(x, *a) - v(x, *b), cap_i[*idx]);
                    push(name, i, vv, vec![], vv * i);
                }
                Ce::V { name, p, n, br, .. } | Ce::L { name, p, n, br, .. } | Ce::E { name, p, n, br, .. } => {
                    let (i, vv) = (x[*br], v(x, *p) - v(x, *n));
                    push(name, i, vv, vec![], vv * i);
                }
                Ce::I { name, from, to, wave, .. } => {
                    let (i, vv) = (src(wave), v(x, *from) - v(x, *to));
                    push(name, i, vv, vec![], vv * i);
                }
                Ce::G { name, p, n, cp, cn, gm } => {
                    let (i, vv) = (gm * (v(x, *cp) - v(x, *cn)), v(x, *p) - v(x, *n));
                    push(name, i, vv, vec![], vv * i);
                }
                Ce::D { name, a, ai, k, rs, is, nvt, bv, ibv, .. } => {
                    let vd = v(x, *ai) - v(x, *k);
                    let mut id = is * (exp_safe(vd / nvt) - 1.0);
                    if bv.is_finite() {
                        id -= ibv * exp_safe(-(vd + bv) / nvt);
                    }
                    let i = if *rs > 0.0 { (v(x, *a) - v(x, *ai)) / rs } else { id };
                    let vv = v(x, *a) - v(x, *k);
                    push(name, i, vv, vec![], vv * i);
                }
                Ce::M { name, d, g, s, m } => {
                    let pol = if m.pmos { -1.0 } else { 1.0 };
                    let (vgs, vds) = (v(x, *g) - v(x, *s), v(x, *d) - v(x, *s));
                    let id = pol * mos_eval(m, pol * vgs, pol * vds).0;
                    push(name, id, vds, vec![("Vgs".into(), vgs)], vds * id);
                }
                Ce::S { name, a, b, cp, cn, m, .. } => {
                    let (vab, vc) = (v(x, *a) - v(x, *b), v(x, *cp) - v(x, *cn));
                    let i = m.eval(vc).0 * vab;
                    push(name, i, vab, vec![("Vc".into(), vc)], vab * i);
                }
                Ce::Q { name, c, b, bi, e, m, .. } => {
                    let pol = if m.pnp { -1.0 } else { 1.0 };
                    let q = bjt_eval(m, pol * (v(x, *bi) - v(x, *e)), pol * (v(x, *bi) - v(x, *c)));
                    let (ic, ib, ie) = (pol * q.ic, pol * q.ib, pol * q.ie);
                    let p = v(x, *c) * ic + v(x, *b) * ib + v(x, *e) * ie;
                    push(name, ic, v(x, *c) - v(x, *e), vec![("Ib".into(), ib), ("Ie".into(), ie)], p);
                }
            }
        }
        els
    }
}
