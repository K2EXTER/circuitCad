//! Newton-Raphson iteration on the assembled system.

use super::*;

pub(super) fn converged(x: &[f64], xn: &[f64], n_node_unknowns: usize) -> bool {
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
    pub(super) fn newton_with(&self, x0: Vec<f64>, vj0: Vec<f64>, ctx: &Ctx, backtrack: bool) -> Result<Attempt, Fail> {
        if self.linear {
            return self.solve_linear(x0, vj0, ctx);
        }
        let (mut x, mut vj) = (x0, vj0);
        let n_node_unknowns = self.n_nodes - 1;
        // the previous point, its Newton step, its residual, its junction state and the fraction taken
        let mut prev: Option<(Vec<f64>, Vec<f64>, f64, Vec<f64>, f64)> = None;
        for it in 1..=MAX_ITER {
            let (m, limited) = self.assemble(&mut vj, &x, ctx);
            let n = m.n;
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
            factor(m.n, m.rows).map_err(|_| Fail::Singular)?.solve(&mut xn);
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
