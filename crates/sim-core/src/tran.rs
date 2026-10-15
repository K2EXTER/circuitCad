//! Transient analysis: trapezoidal integration (backward Euler for the step after every source
//! corner, to avoid numerical ringing), Newton-Raphson per step, step halving on failure.

use crate::circuit::Circuit;
use crate::dc::{compile, floating_warnings, has_ground, solve_op, v, Attempt, Ctx, Dyn, SimError, Ce};

const MAX_POINTS: f64 = 1_000_000.0;
/// Absolute floors of the step-error check: a capacitor voltage (V) and an inductor current (A).
const LTE_ABS_V: f64 = 1e-6;
const LTE_ABS_I: f64 = 1e-9;

#[derive(Clone, Debug)]
pub struct TranSettings {
    pub tstop: f64,
    /// Largest time step (and the output spacing away from source corners).
    pub tstep: f64,
    /// Start with every capacitor/inductor at zero instead of from the DC operating point.
    pub uic: bool,
}

#[derive(Clone, Debug)]
pub struct Signal {
    /// `V(node)` or `I(element)`.
    pub name: String,
    pub unit: &'static str,
    pub values: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct TranResult {
    pub time: Vec<f64>,
    pub signals: Vec<Signal>,
    pub warnings: Vec<String>,
    /// Newton solves that were thrown away and retried with a smaller step.
    pub retries: usize,
    /// Steps thrown away because the error estimate was too large, and redone smaller.
    pub rejected: usize,
}

impl TranResult {
    pub fn signal(&self, name: &str) -> Option<&Signal> {
        self.signals.iter().find(|s| s.name == name)
    }
}

/// The operating point the run starts from. Capacitors with an initial voltage (`IC=`) are held at
/// it by temporary voltage sources while the rest of the circuit settles around them.
fn start_point(circuit: &Circuit, cm: &crate::dc::Compiled) -> Result<(Vec<f64>, Vec<f64>), SimError> {
    let order = crate::dc::DC_ORDER;
    let has_ic = circuit.initial.iter().any(|(n, _)| circuit.elements.iter().any(|e| matches!(e, crate::Element::Capacitor { name, .. } if name == n)));
    if !has_ic {
        return solve_op(cm, &order).map(|(att, _)| (att.x, att.vj));
    }
    // Capacitors have no branch unknown, and the held sources go last, so every index of the
    // original circuit is unchanged and the extra unknowns sit at the end.
    let mut held = circuit.clone();
    let mut sources = vec![];
    held.elements.retain(|e| match e {
        crate::Element::Capacitor { name, a, b, .. } => match circuit.initial.iter().find(|(n, _)| n == name) {
            Some((_, v)) => {
                sources.push(crate::Element::vdc(&format!("ic.{name}"), *a, *b, *v));
                false
            }
            None => true,
        },
        _ => true,
    });
    held.elements.extend(sources);
    let cm_held = compile(&held);
    let (att, _) = solve_op(&cm_held, &order)?;
    let n = cm.n_unknowns;
    Ok((att.x[..n].to_vec(), att.vj))
}

/// The capacitor voltages and inductor currents of a solution (the integrator's state variables).
fn reactive_state(cm: &crate::dc::Compiled, x: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let (mut cv, mut il) = (vec![0.0; cm.n_caps], vec![0.0; cm.n_inds]);
    for e in &cm.ce {
        match e {
            Ce::C { a, b, idx, .. } => cv[*idx] = v(x, *a) - v(x, *b),
            Ce::L { br, idx, .. } => il[*idx] = x[*br],
            Ce::Q { bi, ci, ei, m, cap: Some(idx), .. } => {
                let pol = if m.pnp { -1.0 } else { 1.0 };
                cv[*idx] = pol * (v(x, *bi) - v(x, *ei));
                cv[*idx + 1] = pol * (v(x, *bi) - v(x, *ci));
            }
            Ce::D { ai, k, cap: Some(idx), .. } => cv[*idx] = v(x, *ai) - v(x, *k),
            _ => {}
        }
    }
    (cv, il)
}

/// Divided difference of order `pts.len() - 1` through `(t, y)` points.
fn divided_difference(pts: &[(f64, f64)]) -> f64 {
    let mut d: Vec<f64> = pts.iter().map(|p| p.1).collect();
    for k in 1..pts.len() {
        for i in 0..pts.len() - k {
            d[i] = (d[i + 1] - d[i]) / (pts[i + k].0 - pts[i].0);
        }
    }
    d[0]
}

/// One earlier accepted point: time, capacitor voltages, inductor currents.
type Sample = (f64, Vec<f64>, Vec<f64>);

/// Local truncation error of the step to `t_new` relative to what is allowed (> 1: too big).
/// Trapezoidal: h^3/12 times the third derivative (= 6 DD3); backward Euler: h^2/2 times the second (= 2 DD2).
/// `None` while there are not enough smooth earlier points.
fn step_error_ratio(hist: &[Sample], t_new: f64, cv: &[f64], il: &[f64], trap: bool, rel: f64) -> Option<f64> {
    let need = if trap { 3 } else { 2 };
    if hist.len() < need {
        return None;
    }
    let past = &hist[hist.len() - need..];
    let h = t_new - past[need - 1].0;
    let mut worst: f64 = 0.0;
    let mut check = |get: &dyn Fn(&Sample) -> f64, new: f64, abs: f64| {
        let mut pts: Vec<(f64, f64)> = past.iter().map(|p| (p.0, get(p))).collect();
        pts.push((t_new, new));
        let dd = divided_difference(&pts).abs();
        let err = if trap { h.powi(3) * dd / 2.0 } else { h * h * dd };
        let old = pts[need - 1].1;
        worst = worst.max(err / (rel * new.abs().max(old.abs()) + abs));
    };
    for (k, &new) in cv.iter().enumerate() {
        check(&|p| p.1[k], new, LTE_ABS_V);
    }
    for (k, &new) in il.iter().enumerate() {
        check(&|p| p.2[k], new, LTE_ABS_I);
    }
    Some(worst)
}

pub fn transient(circuit: &Circuit, s: &TranSettings) -> Result<TranResult, SimError> {
    if !(s.tstop.is_finite() && s.tstop > 0.0) {
        return Err(SimError::Invalid("終了時間は正の値にしてください".into()));
    }
    if !(s.tstep.is_finite() && s.tstep > 0.0) {
        return Err(SimError::Invalid("時間刻みは正の値にしてください".into()));
    }
    if s.tstop / s.tstep > MAX_POINTS {
        return Err(SimError::Invalid(format!("終了時間 / 時間刻み が大きすぎます (最大 {} 点)", MAX_POINTS as u64)));
    }
    if !has_ground(circuit) {
        return Err(SimError::NoGround);
    }
    let mut warnings = floating_warnings(circuit);
    for e in &circuit.elements {
        if let crate::Element::VSource { name, wave, .. } | crate::Element::ISource { name, wave, .. } = e
            && let Some(ts) = wave.noise_period()
                && s.tstep > ts / 3.0 {
                    warnings.push(format!(
                        "{name}: ノイズの最高周波数に対して時間刻みが粗すぎます (刻みは {:.3e} 秒以下を推奨)",
                        ts / 3.0
                    ));
                }
    }
    let cm = compile(circuit);

    // initial point
    let (mut x, mut vj) = if s.uic {
        (vec![0.0; cm.n_unknowns], cm.zero_junction_state())
    } else {
        match start_point(circuit, &cm) {
            Ok((x, vj)) => (x, vj),
            // an oscillator has no stable operating point (with the capacitors open it sits on
            // the unstable balance point): start from zero charge instead, like SPICE's UIC
            Err(SimError::NoConvergence) => {
                warnings.push("DC動作点が求まらなかったため、すべてゼロの状態から始めました (発振回路では普通のことです)".into());
                (vec![0.0; cm.n_unknowns], cm.zero_junction_state())
            }
            Err(e) => return Err(e),
        }
    };

    let mut cap_v = vec![0.0; cm.n_caps];
    let mut cap_i = vec![0.0; cm.n_caps];
    let mut ind_i = vec![0.0; cm.n_inds];
    let mut ind_v = vec![0.0; cm.n_inds];
    for e in &cm.ce {
        match e {
            Ce::C { a, b, idx, .. } => cap_v[*idx] = v(&x, *a) - v(&x, *b),
            Ce::L { br, idx, .. } => ind_i[*idx] = x[*br],
            Ce::Q { bi, ci, ei, m, cap: Some(idx), .. } => {
                let pol = if m.pnp { -1.0 } else { 1.0 };
                cap_v[*idx] = pol * (v(&x, *bi) - v(&x, *ei));
                cap_v[*idx + 1] = pol * (v(&x, *bi) - v(&x, *ci));
            }
            Ce::D { ai, k, cap: Some(idx), .. } => cap_v[*idx] = v(&x, *ai) - v(&x, *k),
            _ => {}
        }
    }

    // times the integrator must land on
    let mut bps: Vec<f64> = vec![];
    for e in &circuit.elements {
        match e {
            crate::Element::VSource { wave, .. } | crate::Element::ISource { wave, .. } => bps.extend(wave.breakpoints(s.tstop)),
            _ => {}
        }
    }
    bps.sort_by(|a, b| a.total_cmp(b));
    bps.dedup_by(|a, b| (*a - *b).abs() <= 1e-15 * s.tstop.max(1e-12));
    let mut bp_idx = 0;

    let mut result = TranResult { time: vec![], signals: vec![], warnings: vec![], retries: 0, rejected: 0 };
    let node_ids: Vec<usize> = (1..cm.node_names.len()).collect();
    for &i in &node_ids {
        result.signals.push(Signal { name: format!("V({})", cm.node_names[i]), unit: "V", values: vec![] });
    }
    let first = cm.element_results(&x, Some(0.0), &cap_i);
    for e in &first {
        result.signals.push(Signal { name: format!("I({})", e.name), unit: "A", values: vec![] });
    }
    let record = |res: &mut TranResult, t: f64, x: &[f64], cap_i: &[f64], cm: &crate::dc::Compiled| {
        res.time.push(t);
        for (k, &i) in node_ids.iter().enumerate() {
            res.signals[k].values.push(v(x, i));
        }
        let base = node_ids.len();
        let mut k = 0;
        cm.visit_elements(x, Some(t), cap_i, |_, current, _, _, _| {
            res.signals[base + k].values.push(current);
            k += 1;
        });
    };
    record(&mut result, 0.0, &x, &cap_i, &cm);

    let eps = 1e-12 * s.tstop;
    let mut t = 0.0;
    let mut force_be = true; // first step: no derivative history yet
    let lte_rel = crate::options::options().lte_reltol;
    // `tstep` is the largest step; `h_ctrl` is the step the error control currently trusts
    let h_min = s.tstep * 1e-4;
    let mut h_ctrl = s.tstep / 10.0;
    // the last accepted points (time, capacitor voltages, inductor currents) for the error estimate;
    // a source corner breaks the smoothness, so the history restarts there
    let mut hist: Vec<(f64, Vec<f64>, Vec<f64>)> = vec![(0.0, cap_v.clone(), ind_i.clone())];
    while t < s.tstop - eps {
        while bp_idx < bps.len() && bps[bp_idx] <= t + eps {
            bp_idx += 1;
        }
        let mut h = h_ctrl.min(s.tstep).min(s.tstop - t);
        let wanted = h;
        let mut lands_on_bp = false;
        if bp_idx < bps.len() && t + h >= bps[bp_idx] - eps {
            h = bps[bp_idx] - t;
            lands_on_bp = true;
        }
        if t + h >= s.tstop - eps {
            h = s.tstop - t;
        }

        let mut retried = false;
        let mut rejects = 0;
        // one pass per tried step size: Newton (shrinking on failure), then the error check
        let (att, ratio, order) = 'step: loop {
            let mut tries = 0;
            let mut h_back = 0.0;
            let att: Attempt = loop {
                let trap = !(force_be || retried);
                let ctx = Ctx {
                    gmin: crate::options::options().gmin,
                    scale: 1.0,
                    t: t + h,
                    dynamic: Some(Dyn { h, trap, cap_v: &cap_v, cap_i: &cap_i, ind_i: &ind_i, ind_v: &ind_v }),
                    sw_widen: 1.0,
                };
                match cm.newton(x.clone(), vj.clone(), &ctx) {
                    Ok(a) => break a,
                    Err(crate::dc::Fail::Singular) => return Err(SimError::Singular),
                    Err(crate::dc::Fail::NoConv) => {
                        tries += 1;
                        result.retries += 1;
                        lands_on_bp = false;
                        // the trapezoidal rule keeps the old capacitor current, which for a fast
                        // switching event no step size can fix: retry with backward Euler
                        retried = true;
                        if tries == 4 && h * 16.0 < s.tstep.min(s.tstop - t) {
                            // An instant switching event (a latch, a logic gate) may have no solution
                            // close to the old state at all, yet one far away that a big step lands on:
                            // shrinking never helps there, so try the largest step once, then go on shrinking.
                            h_back = h / 4.0;
                            h = s.tstep.min(s.tstop - t);
                            if bp_idx < bps.len() && t + h >= bps[bp_idx] - eps {
                                h = bps[bp_idx] - t;
                                lands_on_bp = true;
                            }
                        } else if h_back > 0.0 {
                            h = h_back;
                            h_back = 0.0;
                        } else {
                            h /= 4.0;
                        }
                        if tries > 12 || h < 1e-18 {
                            return Err(SimError::NoConvergence);
                        }
                    }
                }
            };
            let trap = !(force_be || retried);
            let order = if trap { 3.0 } else { 2.0 };
            let (cv, il) = reactive_state(&cm, &att.x);
            let ratio = step_error_ratio(&hist, t + h, &cv, &il, trap, lte_rel);
            match ratio {
                // too inaccurate: redo this step with a smaller one (unless it is already tiny)
                Some(r) if r > 1.0 && h > h_min && rejects < 12 => {
                    rejects += 1;
                    result.rejected += 1;
                    lands_on_bp = false;
                    h *= (0.9 * r.powf(-1.0 / order)).clamp(0.125, 0.9);
                }
                _ => break 'step (att, ratio, order),
            }
        };

        // accept the step: update the integration state from the new solution
        let trap = !(force_be || retried);
        for e in &cm.ce {
            match e {
                Ce::C { a, b, c, idx, .. } => {
                    let vn = v(&att.x, *a) - v(&att.x, *b);
                    let geq = if trap { 2.0 * c / h } else { c / h };
                    cap_i[*idx] = geq * (vn - cap_v[*idx]) - if trap { cap_i[*idx] } else { 0.0 };
                    cap_v[*idx] = vn;
                }
                Ce::L { p, n, br, idx, .. } => {
                    ind_i[*idx] = att.x[*br];
                    ind_v[*idx] = v(&att.x, *p) - v(&att.x, *n);
                }
                Ce::D { ai, k, nvt, dm, cap: Some(idx), .. } => {
                    let kk = if trap { 2.0 } else { 1.0 };
                    let vnew = v(&att.x, *ai) - v(&att.x, *k);
                    let (q_new, q_old) = (crate::dc::diode_q(dm, *nvt, vnew).0, crate::dc::diode_q(dm, *nvt, cap_v[*idx]).0);
                    cap_i[*idx] = kk / h * (q_new - q_old) - if trap { cap_i[*idx] } else { 0.0 };
                    cap_v[*idx] = vnew;
                }
                Ce::Q { bi, ci, ei, m, cap: Some(idx), .. } => {
                    let pol = if m.pnp { -1.0 } else { 1.0 };
                    let k = if trap { 2.0 } else { 1.0 };
                    let vn = [pol * (v(&att.x, *bi) - v(&att.x, *ei)), pol * (v(&att.x, *bi) - v(&att.x, *ci))];
                    for (j, vnew) in vn.into_iter().enumerate() {
                        let (q_new, q_old) = if j == 0 { (crate::dc::bjt_qbe(m, vnew).0, crate::dc::bjt_qbe(m, cap_v[*idx]).0) } else { (crate::dc::bjt_qbc(m, vnew).0, crate::dc::bjt_qbc(m, cap_v[*idx + 1]).0) };
                        cap_i[*idx + j] = k / h * (q_new - q_old) - if trap { cap_i[*idx + j] } else { 0.0 };
                        cap_v[*idx + j] = vnew;
                    }
                }
                _ => {}
            }
        }
        t = if lands_on_bp { bps[bp_idx] } else { t + h };
        x = att.x;
        vj = att.vj;
        // the step after a retry has no reliable current history either
        force_be = lands_on_bp || retried;
        record(&mut result, t, &x, &cap_i, &cm);

        // the next step: grow while the error estimate leaves room, never beyond `tstep`
        if lands_on_bp {
            hist.clear();
            // restart from about the length of the step that reached the corner
            h_ctrl = h.max(s.tstep * 1e-3).min(s.tstep);
        } else if retried {
            h_ctrl = h.max(h_min);
        } else if h >= wanted * 0.999 || ratio.is_some_and(|r| r > 1.0) {
            let growth = ratio.map_or(2.0, |r| (0.9 * r.max(1e-12).powf(-1.0 / order)).clamp(0.5, 2.0));
            h_ctrl = (h * growth).clamp(h_min, s.tstep);
        }
        // else: the step was cut short by the end of the run; keep the controller's value
        hist.push((t, cap_v.clone(), ind_i.clone()));
        if hist.len() > 3 {
            hist.remove(0);
        }
    }

    warnings.dedup();
    result.warnings = warnings;
    Ok(result)
}
