//! DC operating point: modified nodal analysis + damped Newton-Raphson, with gmin and
//! source stepping as convergence aids.

use crate::circuit::{BjtModel, Circuit, DiodeModel, Element, MosModel, SwitchModel};
use crate::waveform::Waveform;
use std::fmt;

/// kT/q at the nominal 27 C (300.15 K): k/q = 8.617333e-5 V/K. (0.025852 was kT/q at 300.00 K, 0.05 %
/// low: a forward junction voltage is about 25 Vt, so that is 0.3 mV and a 1 % change of current.)
pub(crate) const VT: f64 = 8.617333262e-5 * 300.15;

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
        self.voltages.iter().find(|(n, _)| n == node).or_else(|| self.voltages.iter().find(|(n, _)| n.eq_ignore_ascii_case(node))).map(|(_, v)| *v)
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
    L { name: String, p: usize, n: usize, l: f64, br: usize, idx: usize, i0: f64 },
    /// Mutual inductance `m` (H) between the inductors with branches `br1` / `br2` and history slots `idx1` / `idx2`.
    K { br1: usize, br2: usize, idx1: usize, idx2: usize, m: f64 },
    I { name: String, from: usize, to: usize, wave: Waveform, ac: Option<(f64, f64)> },
    E { name: String, p: usize, n: usize, cp: usize, cn: usize, gain: f64, br: usize },
    G { name: String, p: usize, n: usize, cp: usize, cn: usize, gm: f64 },
    D { name: String, a: usize, ai: usize, k: usize, rs: f64, is: f64, nvt: f64, vcrit: f64, bv: f64, ibv: f64, slot: usize, dm: DiodeModel, cap: Option<usize> },
    /// `bi` is the internal base node (equal to `b` when the model has no `rb`).
    Q { name: String, c: usize, b: usize, bi: usize, e: usize, ci: usize, ei: usize, m: BjtModel, slot: usize, cap: Option<usize> },
    M { name: String, d: usize, g: usize, s: usize, m: MosModel },
    S { name: String, a: usize, b: usize, cp: usize, cn: usize, m: SwitchModel },
    /// A flip-flop / latch / Schmitt trigger: two voltage-source outputs (`br_q`, `br_qb`; node 0 = unused) at the level of state `idx`.
    Seq { kind: crate::SeqKind, ins: Vec<usize>, q: usize, qb: usize, br_q: usize, br_qb: usize, idx: usize, lv: crate::SeqLevels },
    /// `br` is only meaningful for a voltage-type source.
    B { name: String, p: usize, n: usize, voltage: bool, br: usize, c: crate::expr::CExpr },
    /// Charge-type capacitor: `c` is Q(x); slots `idx` (voltage) and `idx + 1` (charge) of the capacitor state.
    Cq { name: String, a: usize, b: usize, c: crate::expr::CExpr, idx: usize },
}

/// (gmin, (step, trapezoidal) when dynamic)
type FactorKey = (u64, Option<(u64, bool)>);

pub(crate) struct Compiled {
    pub(crate) node_names: Vec<String>, // circuit nodes only
    pub(crate) n_caps: usize,
    pub(crate) n_inds: usize,
    n_nodes: usize,          // including ground and internal diode/base nodes
    pub(crate) n_unknowns: usize,
    n_slots: usize,
    /// No diode, transistor or switch: the equations are linear, so one solve is exact.
    linear: bool,
    /// Behavioural sources can make Newton cycle between two points (a clipped feedback loop),
    /// so the backtracking line search also rejects steps that do not reduce the residual.
    has_b: bool,
    /// Factorisations of the linear system keyed by (gmin, dynamic step); a few are kept because
    /// the step shrinks to land on source corners and then returns to the nominal value.
    cache: std::cell::RefCell<Vec<(FactorKey, std::rc::Rc<Factor>)>>,
    /// The pivot order and fill pattern of the last full factorisation (see `linalg::refactor`).
    plan: std::cell::RefCell<Option<std::rc::Rc<linalg::Symbolic>>>,
    pub(crate) ce: Vec<Ce>,
    /// The stored state (0 or 1) of every `Ce::Seq`, changed only between solves.
    pub(crate) seq_state: std::cell::RefCell<Vec<f64>>,
}


mod build;
mod devices;
mod linalg;
mod mna;
mod newton;
mod seq;

// the pieces see each other through the parent; the rest of the crate gets what it needs from here
use build::*;
use devices::*;
use linalg::*;

pub(crate) use build::{compile, v};
pub(crate) use devices::{bjt_eval, bjt_qbc, bjt_qbe, diode_q, diode_recomb, exp_safe, mos_eval};
#[cfg(test)]
pub(crate) use devices::depletion;
pub(crate) use mna::{Ctx, Dyn};
pub(crate) use newton::{Attempt, Fail};
#[cfg(test)]
pub(crate) use newton::REFAC;

// ------------------------------------------------------------------ public entry

/// Nodes without a DC path to ground (capacitors are open; ideal current sources and
/// controlled-source inputs do not provide one).
fn floating_nodes(c: &Circuit) -> Vec<usize> {
    let n = c.names.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
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
            Element::Mutual { .. } => {}
            // the outputs are voltage sources against ground
            Element::Seq { q, qb, .. } => {
                union(*q, 0);
                union(*qb, 0);
            }
            Element::Bjt { c, b, e, .. } => {
                union(*c, *e);
                union(*b, *e);
            }
            Element::BSource { p, n, voltage: true, .. } => union(*p, *n),
            Element::Capacitor { .. } | Element::ChargeCap { .. } | Element::ISource { .. } | Element::Vccs { .. } | Element::BSource { .. } => {}
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
    /// Switches start with wide transitions that narrow step by step to the modelled width
    /// (latches and comparators built from switches have no usable gradient otherwise).
    Widen,
}

/// The order in which the operating point is attempted before a transient, AC or noise run. Those
/// rely on a bistable or oscillating circuit reporting "no operating point" and starting from zero.
pub(crate) const DC_ORDER: [Strategy; 3] = [Strategy::Direct, Strategy::Gmin, Strategy::Source];

/// `dc_op` (the DC panel) also tries `Widen`, so that a latch settles into one of its stable states.
pub(crate) const DC_ORDER_WIDEN: [Strategy; 4] = [Strategy::Direct, Strategy::Gmin, Strategy::Source, Strategy::Widen];

impl Compiled {
    fn run(&self, which: Strategy) -> Result<Attempt, Fail> {
        let zero_x = vec![0.0; self.n_unknowns];
        let zero_vj = vec![0.0; self.n_slots];
        match which {
            Strategy::Direct => self.newton(zero_x, zero_vj, &Ctx::dc(crate::options::options().gmin, 1.0)),
            Strategy::Gmin => {
                // gmin from 1e-2 down to 1e-12 (a decade at a time), each step starting from the previous solution. A step
                // that does not converge is split into smaller ones: a latch or a current-limited output stage can need
                // hundreds of iterations for a decade, but only a few for a third of it.
                let first = self.newton_damped(zero_x, zero_vj, &Ctx::dc(1e-2, 1.0))?;
                let (mut x, mut vj, mut iters) = (first.x, first.vj, first.iterations);
                let (mut e, mut de) = (2.0f64, 1.0f64);
                while e < 12.0 - 1e-9 {
                    let target = (e + de).min(12.0);
                    let ctx = Ctx::dc(10f64.powf(-target), 1.0);
                    match self.newton_damped(x.clone(), vj.clone(), &ctx) {
                        Ok(a) => {
                            iters += a.iterations;
                            x = a.x;
                            vj = a.vj;
                            de = (de * 2.0).min(1.0);
                            e = target;
                        }
                        Err(_) if de >= 1.0 / 8.0 => de /= 2.0,
                        // already a small step: a circuit with saturating gates (current-limited output stages, latches built
                        // from them) can crawl towards the solution for hundreds of iterations, so give it forty times as many
                        Err(f) => {
                            let a = self.newton_long(x.clone(), vj.clone(), &ctx).map_err(|_| f)?;
                            // a "solution" with a node at a million volts is a runaway that only looks converged (relative tolerances)
                            if a.x.iter().take(self.n_nodes - 1).any(|v| v.abs() > 1e6) {
                                return Err(Fail::NoConv);
                            }
                            iters += a.iterations;
                            x = a.x;
                            vj = a.vj;
                            e = target;
                        }
                    }
                }
                Ok(Attempt { x, vj, iterations: iters })
            }
            Strategy::Source => {
                // ramp every independent source from 0 to full value
                let (mut x, mut vj, mut iters) = (zero_x, zero_vj, 0);
                for step in 1..=25 {
                    let a = self.newton(x, vj, &Ctx::dc(crate::options::options().gmin, step as f64 / 25.0))?;
                    iters += a.iterations;
                    x = a.x;
                    vj = a.vj;
                }
                Ok(Attempt { x, vj, iterations: iters })
            }
            Strategy::Widen => {
                let (mut x, mut vj, mut iters) = (zero_x, zero_vj, 0);
                for widen in [1000.0, 300.0, 100.0, 30.0, 10.0, 5.0, 3.0, 2.0, 1.5, 1.0] {
                    let ctx = Ctx { sw_widen: widen, ..Ctx::dc(crate::options::options().gmin, 1.0) };
                    let a = self.newton_damped(x, vj, &ctx)?;
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
    dc_op_with(circuit, &DC_ORDER_WIDEN)
}

/// Floating-node warnings are produced by the caller; this only needs a ground reference.
pub(crate) fn has_ground(circuit: &Circuit) -> bool {
    circuit.elements.iter().any(|e| match e {
        Element::Resistor { a, b, .. } | Element::Capacitor { a, b, .. } | Element::ChargeCap { a, b, .. } | Element::Inductor { a, b, .. } => *a == 0 || *b == 0,
        Element::VSource { p, n, .. } => *p == 0 || *n == 0,
        Element::ISource { from, to, .. } => *from == 0 || *to == 0,
        Element::Diode { a, k, .. } => *a == 0 || *k == 0,
        Element::Mosfet { d, g, s, .. } => *d == 0 || *g == 0 || *s == 0,
        Element::Switch { a, b, cp, cn, .. } => *a == 0 || *b == 0 || *cp == 0 || *cn == 0,
        Element::Mutual { .. } => false,
        Element::Seq { .. } => true,
        Element::Bjt { c, b, e, .. } => *c == 0 || *b == 0 || *e == 0,
        Element::Vcvs { p, n, .. } | Element::Vccs { p, n, .. } | Element::BSource { p, n, .. } => *p == 0 || *n == 0,
    })
}

pub(crate) fn floating_warnings(circuit: &Circuit) -> Vec<String> {
    let mut w = circuit.warnings.clone();
    for i in floating_nodes(circuit) {
        // a node inside a part ("U1.comp", "C1.esr") is the model's business: the learner cannot wire it
        if circuit.names[i].contains('.') {
            continue;
        }
        w.push(format!("ノード {} にGNDへのDC経路がありません (0Vとして扱います)", circuit.names[i]));
    }
    w
}

/// Try the strategies in order; the first that converges wins.
pub(crate) fn solve_op(cm: &Compiled, order: &[Strategy]) -> Result<(Attempt, &'static str), SimError> {
    TROUBLE.with(|t| t.borrow_mut().take());
    newton::STUCK.with(|s| s.set(None));
    let mut last = SimError::NoConvergence;
    for &which in order {
        match cm.run(which) {
            Ok(mut att) => {
                // the stored states of flip-flops and latches follow the operating point: update them and solve again until
                // they agree with the voltages (a few rounds at most; a ring of them can chase its own tail)
                if cm.has_seq() {
                    for _ in 0..16 {
                        if !cm.seq_update(None, &att.x) {
                            break;
                        }
                        match cm.run(which) {
                            Ok(a) => att = a,
                            Err(_) => break,
                        }
                    }
                }
                let name = match which {
                    Strategy::Direct => "direct",
                    Strategy::Gmin => "gmin stepping",
                    Strategy::Source => "source stepping",
                    Strategy::Widen => "switch widening",
                };
                return Ok((att, name));
            }
            // a circuit that is singular for good fails every strategy at its first iteration, which costs nothing
            Err(Fail::Singular) => last = SimError::Singular,
            Err(Fail::NoConv) => last = SimError::NoConvergence,
        }
    }
    if last == SimError::NoConvergence
        && let Some((i, before, after)) = newton::STUCK.with(|s| s.get())
        && let Some(name) = cm.node_names.get(i + 1)
    {
        TROUBLE.with(|t| *t.borrow_mut() = Some(trouble_text(name, before, after)));
    }
    Err(last)
}

thread_local! {
    static TROUBLE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

fn trouble_text(node: &str, before: f64, after: f64) -> String {
    let place = match node.split_once('.') {
        Some((part, _)) => format!("部品 {part} の中のノード {node}"),
        None => format!("ノード {node}"),
    };
    if after.abs() > 1e3 {
        // a node at kilovolts is the iteration running away, not a place that is merely hard to settle: it need not be the cause
        format!(
            "計算が発散しました。最後の繰り返しで一番大きく動いたのは {place} です ({before:.4} V から {after:.4} V)。ここは暴走の結果で、原因とは限りません。非線形な部品 (比較器・基準電圧・オペアンプのマクロモデルなど) が複数つながっていると、ゼロからの計算が飛び出すことがあります。部品を1つずつ外して、収束するようになるものを探すと、組み合わせの原因が分かります"
        )
    } else {
        format!(
            "計算の最後まで落ち着かなかったのは {place} です (最後の繰り返しで {before:.4} V から {after:.4} V へ動きました)。この近くの部品の向き・値と、帰還のかかり方を見直してください。2つの状態を持つ部品 (ヒステリシスつきの比較器など) や、電圧に応じて電流が決まる部品が近くにあると、DCでは答えが1点に決まらないことがあります"
        )
    }
}

/// Where the last DC solve that gave up was still unsettled (None if the last solve succeeded or has no such place).
pub fn last_convergence_trouble() -> Option<String> {
    TROUBLE.with(|t| t.borrow().clone())
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
        self.visit_elements(x, t, cap_i, |name, current, voltage, extra, power| {
            els.push(ElementResult { name: name.to_string(), current, voltage, power, extra });
        });
        els
    }

    /// Calls `push(name, current, voltage, extra, power)` for every element, in circuit order.
    pub(crate) fn visit_elements(&self, x: &[f64], t: Option<f64>, cap_i: &[f64], mut push: impl FnMut(&str, f64, f64, Vec<(String, f64)>, f64)) {
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
                Ce::C { name, a, b, idx, .. } | Ce::Cq { name, a, b, idx, .. } => {
                    let (vv, i) = (v(x, *a) - v(x, *b), cap_i[*idx]);
                    push(name, i, vv, vec![], vv * i);
                }
                Ce::V { name, p, n, br, .. } | Ce::L { name, p, n, br, .. } | Ce::E { name, p, n, br, .. } => {
                    let (i, vv) = (x[*br], v(x, *p) - v(x, *n));
                    push(name, i, vv, vec![], vv * i);
                }
                Ce::K { .. } | Ce::Seq { .. } => {}
                Ce::I { name, from, to, wave, .. } => {
                    let (i, vv) = (src(wave), v(x, *from) - v(x, *to));
                    push(name, i, vv, vec![], vv * i);
                }
                Ce::G { name, p, n, cp, cn, gm } => {
                    let (i, vv) = (gm * (v(x, *cp) - v(x, *cn)), v(x, *p) - v(x, *n));
                    push(name, i, vv, vec![], vv * i);
                }
                Ce::D { name, a, ai, k, rs, is, nvt, bv, ibv, dm, .. } => {
                    let vd = v(x, *ai) - v(x, *k);
                    let mut id = is * (exp_safe(vd / nvt) - 1.0) + diode_recomb(dm, vd).0;
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
                Ce::B { name, p, n, voltage, br, c } => {
                    let vv = v(x, *p) - v(x, *n);
                    let i = if *voltage { x[*br] } else { c.eval(x, t.unwrap_or(0.0)).0 };
                    push(name, i, vv, vec![], vv * i);
                }
                Ce::Q { name, c, b, bi, e, ci, ei, m, .. } => {
                    let pol = if m.pnp { -1.0 } else { 1.0 };
                    let q = bjt_eval(m, pol * (v(x, *bi) - v(x, *ei)), pol * (v(x, *bi) - v(x, *ci)));
                    let (ic, ib, ie) = (pol * q.ic, pol * q.ib, pol * q.ie);
                    let p = v(x, *c) * ic + v(x, *b) * ib + v(x, *e) * ie;
                    push(name, ic, v(x, *c) - v(x, *e), vec![("Ib".into(), ib), ("Ie".into(), ie)], p);
                }
            }
        }
    }
}
