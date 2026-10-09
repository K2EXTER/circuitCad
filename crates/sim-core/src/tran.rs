//! Transient analysis: trapezoidal integration (backward Euler for the step after every source
//! corner, to avoid numerical ringing), Newton-Raphson per step, step halving on failure.

use crate::circuit::Circuit;
use crate::dc::{compile, floating_warnings, has_ground, solve_op, v, Attempt, Ctx, Dyn, SimError, Strategy, Ce, GMIN};

const MAX_POINTS: f64 = 1_000_000.0;

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
}

impl TranResult {
    pub fn signal(&self, name: &str) -> Option<&Signal> {
        self.signals.iter().find(|s| s.name == name)
    }
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
        match solve_op(&cm, &[Strategy::Direct, Strategy::Gmin, Strategy::Source]) {
            Ok((att, _)) => (att.x, att.vj),
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

    let mut result = TranResult { time: vec![], signals: vec![], warnings: vec![], retries: 0 };
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
        for (k, e) in cm.element_results(x, Some(t), cap_i).iter().enumerate() {
            res.signals[node_ids.len() + k].values.push(e.current);
        }
    };
    record(&mut result, 0.0, &x, &cap_i, &cm);

    let eps = 1e-12 * s.tstop;
    let mut t = 0.0;
    let mut force_be = true; // first step: no derivative history yet
    while t < s.tstop - eps {
        while bp_idx < bps.len() && bps[bp_idx] <= t + eps {
            bp_idx += 1;
        }
        let mut h = s.tstep.min(s.tstop - t);
        let mut lands_on_bp = false;
        if bp_idx < bps.len() && t + h >= bps[bp_idx] - eps {
            h = bps[bp_idx] - t;
            lands_on_bp = true;
        }
        if t + h >= s.tstop - eps {
            h = s.tstop - t;
        }

        let mut tries = 0;
        let mut retried = false;
        let att: Attempt = loop {
            let trap = !(force_be || retried);
            let ctx = Ctx {
                gmin: GMIN,
                scale: 1.0,
                t: t + h,
                dynamic: Some(Dyn { h, trap, cap_v: &cap_v, cap_i: &cap_i, ind_i: &ind_i, ind_v: &ind_v }),
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
                    h /= 4.0;
                    if tries > 12 || h < 1e-18 {
                        return Err(SimError::NoConvergence);
                    }
                }
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
                _ => {}
            }
        }
        t = if lands_on_bp { bps[bp_idx] } else { t + h };
        x = att.x;
        vj = att.vj;
        // the step after a retry has no reliable current history either
        force_be = lands_on_bp || retried;
        record(&mut result, t, &x, &cap_i, &cm);
    }

    warnings.dedup();
    result.warnings = warnings;
    Ok(result)
}
