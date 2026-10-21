//! Newton-Raphson iteration on the assembled system.

use super::*;

pub(super) fn converged(x: &[f64], xn: &[f64], n_node_unknowns: usize) -> bool {
    let o = crate::options::options();
    x.iter().zip(xn).enumerate().all(|(i, (a, b))| {
        let tol = if i < n_node_unknowns { o.vntol } else { o.abstol };
        (a - b).abs() <= o.reltol * a.abs().max(b.abs()) + tol
    })
}

thread_local! {
    /// When set, `newton_with` gives up (no convergence) once this moment has passed.
    static DEADLINE: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

thread_local! {
    pub(crate) static REFAC: std::cell::Cell<[u64; 2]> = const { std::cell::Cell::new([0; 2]) };
}

/// The plain Newton iteration of a transient step is cut after this many iterations (large systems, first tries only).
const PLAIN_CAP: usize = 40;

/// Systems below this many unknowns are always factored from scratch (and their plain iteration is never cut short).
const REFACTOR_MIN_SIZE: usize = 60;

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
        match self.newton_with(x0.clone(), vj0.clone(), ctx, false, 1.0, f64::INFINITY) {
            Err(Fail::NoConv) => {
                let r = self.newton_with(x0.clone(), vj0.clone(), ctx, true, 1.0, f64::INFINITY);
                if !self.has_b || !matches!(r, Err(Fail::NoConv)) {
                    return r;
                }
                // A clipped feedback loop in a behavioural source can make full Newton steps
                // jump between two points; taking only a part of every step breaks that cycle.
                for damp in [0.5, 0.2] {
                    let r = self.newton_with(x0.clone(), vj0.clone(), ctx, false, damp, f64::INFINITY);
                    if !matches!(r, Err(Fail::NoConv)) {
                        return r;
                    }
                }
                Err(Fail::NoConv)
            }
            other => other,
        }
    }

    /// `newton` with forty times the iteration limit (for a solution the iteration only crawls towards).
    pub(crate) fn newton_long(&self, x0: Vec<f64>, vj0: Vec<f64>, ctx: &Ctx) -> Result<Attempt, Fail> {
        let saved = crate::options::options();
        crate::options::set_options(crate::options::SimOptions { max_iter: saved.max_iter.saturating_mul(40), ..saved });
        // a circuit that really has no solution must not keep the caller waiting: give up after a few seconds
        DEADLINE.with(|d| d.set(Some(std::time::Instant::now() + std::time::Duration::from_secs(4))));
        let r = self.newton(x0, vj0, ctx);
        DEADLINE.with(|d| d.set(None));
        crate::options::set_options(saved);
        r
    }

    /// `newton`, then shorter steps if it fails: close to the point where a latch splits into two
    /// stable states the full step has no fixed point to land on, but a damped iteration slides
    /// down into one of them.
    pub(crate) fn newton_damped(&self, x0: Vec<f64>, vj0: Vec<f64>, ctx: &Ctx) -> Result<Attempt, Fail> {
        let r = self.newton(x0.clone(), vj0.clone(), ctx);
        if !matches!(r, Err(Fail::NoConv)) {
            return r;
        }
        for (damp, max_dx) in [(0.3, f64::INFINITY), (0.1, f64::INFINITY), (1.0, 2.0), (1.0, 0.5), (0.5, 0.1)] {
            let r = self.newton_with(x0.clone(), vj0.clone(), ctx, false, damp, max_dx);
            if !matches!(r, Err(Fail::NoConv)) {
                return r;
            }
        }
        Err(Fail::NoConv)
    }

    /// Solve `rows * x = b`: with the pivot order of the last factorisation when the matrix has the same pattern and the answer
    /// is accurate (`linalg::residual_ok`); otherwise by a full factorisation, which also makes the next plan.
    fn solve_reusing(&self, n: usize, rows: Vec<Vec<(usize, f64)>>, b: Vec<f64>) -> Result<Vec<f64>, SimError> {
        // Only for the large systems, where the factorisation is most of the time: in a small circuit a bistable flip (a multivibrator)
        // has an almost singular matrix, and what Newton's iteration does there depends on the pivots chosen at that moment.
        let plan = if n >= REFACTOR_MIN_SIZE { self.plan.borrow().clone() } else { None };
        let rf = plan.as_ref().and_then(|sym| linalg::refactor(sym, &rows));
        if let Some(f) = rf {
            let mut x = b.clone();
            f.solve(&mut x);
            if x.iter().all(|v| v.is_finite()) && linalg::residual_ok(&rows, &x, &b) {
                REFAC.with(|c| c.set([c.get()[0] + 1, c.get()[1]]));
                return Ok(x);
            }
        }
        REFAC.with(|c| c.set([c.get()[0], c.get()[1] + 1]));
        let (f, sym) = linalg::factor_sym(n, rows)?;
        if n >= REFACTOR_MIN_SIZE {
            *self.plan.borrow_mut() = Some(std::rc::Rc::new(sym));
        }
        let mut x = b;
        f.solve(&mut x);
        Ok(x)
    }

    /// One solve is exact for a linear circuit. The matrix depends only on gmin and the
    /// integration step, so its factorisation is reused while those stay the same.
    pub(super) fn solve_linear(&self, x0: Vec<f64>, mut vj: Vec<f64>, ctx: &Ctx) -> Result<Attempt, Fail> {
        let key = (ctx.gmin.to_bits(), ctx.dynamic.as_ref().map(|d| (d.h.to_bits(), d.trap)));
        let hit = self.cache.borrow().iter().find(|e| e.0 == key).map(|e| e.1.clone());
        let (mut xn, fac) = match hit {
            Some(f) => (self.assemble_with(&mut vj, &x0, ctx, true).0.b, f),
            None => {
                let m = self.assemble_with(&mut vj, &x0, ctx, false).0;
                let f = std::rc::Rc::new(factor(m.n, m.rows).map_err(|_| Fail::Singular)?);
                let mut cache = self.cache.borrow_mut();
                if cache.len() >= 4 {
                    cache.remove(0);
                }
                cache.push((key, f.clone()));
                // the right-hand side is cheap to rebuild and keeps the two paths identical
                (self.assemble_with(&mut vj, &x0, ctx, true).0.b, f)
            }
        };
        fac.solve(&mut xn);
        if xn.iter().any(|v| !v.is_finite()) {
            return Err(Fail::NoConv);
        }
        Ok(Attempt { x: xn, vj, iterations: 1 })
    }

    /// With `backtrack`, a full step that makes the residual of the circuit equations larger is
    /// halved until it helps. A regenerative loop (flip-flop, multivibrator) close to its
    /// switching point has a nearly singular Jacobian, and the full step then overshoots wildly.
    pub(super) fn newton_with(&self, x0: Vec<f64>, vj0: Vec<f64>, ctx: &Ctx, backtrack: bool, damp: f64, max_dx: f64) -> Result<Attempt, Fail> {
        if self.linear {
            return self.solve_linear(x0, vj0, ctx);
        }
        let (mut x, mut vj) = (x0, vj0);
        let n_node_unknowns = self.n_nodes - 1;
        // the previous point, its Newton step, its residual, its junction state and the fraction taken
        type Step = (Vec<f64>, Vec<f64>, f64, Vec<f64>, f64);
        let mut prev: Option<Step> = None;
        // The plain iteration of a transient step that has not converged after a few dozen iterations is not going to: the
        // caller shrinks the step (or tries the line search below, which keeps the full limit). Running it on to the DC limit of
        // 150 was most of the iterations of a transient with many failing steps.
        let plain = !backtrack && damp == 1.0 && max_dx.is_infinite();
        // Only for large systems, like the reuse of the pivot order: in a small circuit a bistable flip (a latch, a logic gate, a
        // multivibrator) needs a few dozen iterations that depend on the last bits of the maths, and the logic blinker example
        // reached 39 of the 40 on Windows and did not converge on Linux and macOS with a cap of 40.
        let capped = ctx.dynamic.as_ref().is_some_and(|d| d.capped) && plain;
        let limit = crate::options::options().max_iter;
        for it in 1..=limit {
            if DEADLINE.with(|d| d.get()).is_some_and(|d| std::time::Instant::now() > d) {
                return Err(Fail::NoConv);
            }
            let (m, limited) = self.assemble(&mut vj, &x, ctx);
            let n = m.n;
            if capped && it > PLAIN_CAP && n >= REFACTOR_MIN_SIZE {
                return Err(Fail::NoConv);
            }
            let mut r2 = f64::NAN;
            // every equation balances to rounding level (relative to its largest term)
            let mut balanced = false;
            if backtrack {
                let mut sum2 = 0.0;
                balanced = true;
                for i in 0..n {
                    let (mut s, mut mag) = (0.0, m.b[i].abs());
                    for &(k, a) in &m.rows[i] {
                        let t = a * x[k];
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
                    && r2 > *rp * (if self.has_b { 0.9999 } else { 1.0001 }) && r2 > 1e-20 && *lam > 1e-3 {
                        *lam *= 0.5;
                        x = xp.iter().zip(dx.iter()).map(|(a, d)| a + *lam * d).collect();
                        vj = vjp.clone();
                        continue;
                    }
            }
            let (x_old, vj_old) = if backtrack { (x.clone(), vj.clone()) } else { (vec![], vec![]) };
            let mut xn = m.b;
            // after the first iteration a singular matrix is the iterate's fault, not the circuit's: an overshoot into
            // a junction at several volts of forward bias puts 1e20 S on the diagonal and the pivots vanish. Report it
            // as "no convergence" so that the damped retries and the other strategies get their turn.
            match self.solve_reusing(m.n, m.rows, xn.clone()) {
                Ok(x) => xn = x,
                Err(_) if it > 1 => return Err(Fail::NoConv),
                Err(_) => return Err(Fail::Singular),
            }
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
            // a part of the step, and never more than `max_dx` volts/amps for any unknown
            let big = x.iter().zip(&xn).map(|(a, b)| (b - a).abs()).fold(0.0, f64::max);
            let part = if big * damp > max_dx { max_dx / big } else { damp };
            x = if part < 1.0 { x.iter().zip(&xn).map(|(a, b)| a + part * (b - a)).collect() } else { xn };
        }
        Err(Fail::NoConv)
    }
}
