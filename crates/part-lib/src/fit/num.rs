//! Small numeric tools for the model fits: Python's `{:.Ng}` number format (the bundled models were written with it),
//! Brent's root finder and the Nelder-Mead minimiser (both as SciPy has them, so a fit lands where the old Python
//! generator's did), and a Levenberg-Marquardt least squares for a few parameters.

/// Python's `format(x, f".{prec}g")`: `prec` significant digits, trailing zeros removed, scientific outside 1e-4 .. 10^prec.
pub fn g(x: f64, prec: usize) -> String {
    if x == 0.0 {
        return "0".into();
    }
    if !x.is_finite() {
        return format!("{x}");
    }
    let p = prec.max(1);
    let sci = format!("{:.*e}", p - 1, x);
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    if (-4..p as i32).contains(&exp) {
        let fixed = format!("{:.*}", (p as i32 - 1 - exp).max(0) as usize, x);
        strip_zeros(&fixed)
    } else {
        format!("{}e{}{:02}", strip_zeros(mant), if exp < 0 { '-' } else { '+' }, exp.abs())
    }
}

fn strip_zeros(s: &str) -> String {
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s.to_string() }
}

/// A root of `f` between `a` and `b` (the signs of `f(a)` and `f(b)` must differ): SciPy's `brentq` with its default
/// tolerances (`xtol` 2e-12, `rtol` 4 eps, 100 iterations).
pub fn brentq(mut f: impl FnMut(f64) -> f64, a: f64, b: f64) -> Option<f64> {
    const XTOL: f64 = 2e-12;
    const RTOL: f64 = 4.0 * f64::EPSILON;
    let (mut xpre, mut xcur) = (a, b);
    let (mut fpre, mut fcur) = (f(xpre), f(xcur));
    let (mut xblk, mut fblk, mut spre, mut scur) = (0.0, 0.0, 0.0, 0.0);
    if fpre * fcur > 0.0 || !(fpre.is_finite() && fcur.is_finite()) {
        return None;
    }
    if fpre == 0.0 {
        return Some(xpre);
    }
    if fcur == 0.0 {
        return Some(xcur);
    }
    for _ in 0..100 {
        if fpre != 0.0 && fcur != 0.0 && fpre.is_sign_negative() != fcur.is_sign_negative() {
            xblk = xpre;
            fblk = fpre;
            spre = xcur - xpre;
            scur = spre;
        }
        if fblk.abs() < fcur.abs() {
            xpre = xcur;
            xcur = xblk;
            xblk = xpre;
            fpre = fcur;
            fcur = fblk;
            fblk = fpre;
        }
        let delta = (XTOL + RTOL * xcur.abs()) / 2.0;
        let sbis = (xblk - xcur) / 2.0;
        if fcur == 0.0 || sbis.abs() < delta {
            return Some(xcur);
        }
        if spre.abs() > delta && fcur.abs() < fpre.abs() {
            let stry = if xpre == xblk {
                -fcur * (xcur - xpre) / (fcur - fpre)
            } else {
                let dpre = (fpre - fcur) / (xpre - xcur);
                let dblk = (fblk - fcur) / (xblk - xcur);
                -fcur * (fblk * dblk - fpre * dpre) / (dblk * dpre * (fblk - fpre))
            };
            if 2.0 * stry.abs() < spre.abs().min(3.0 * sbis.abs() - delta) {
                spre = scur;
                scur = stry;
            } else {
                spre = sbis;
                scur = sbis;
            }
        } else {
            spre = sbis;
            scur = sbis;
        }
        xpre = xcur;
        fpre = fcur;
        if scur.abs() > delta {
            xcur += scur;
        } else {
            xcur += if sbis > 0.0 { delta } else { -delta };
        }
        fcur = f(xcur);
    }
    Some(xcur)
}

/// The Nelder-Mead simplex minimiser as SciPy's `minimize(method="Nelder-Mead")` runs it (the same first simplex, steps and
/// stopping rule), so that a fit started from the same point takes the same path. Returns the best point and its cost.
pub fn nelder_mead(f: &mut impl FnMut(&[f64]) -> f64, x0: &[f64], max_iter: usize, max_fev: usize, xatol: f64, fatol: f64) -> (Vec<f64>, f64) {
    let (rho, chi, psi, sigma) = (1.0, 2.0, 0.5, 0.5);
    let n = x0.len();
    let mut sim: Vec<Vec<f64>> = vec![x0.to_vec()];
    for k in 0..n {
        let mut y = x0.to_vec();
        y[k] = if y[k] != 0.0 { 1.05 * y[k] } else { 0.00025 };
        sim.push(y);
    }
    let mut fev = 0;
    let mut call = |x: &[f64], fev: &mut usize| {
        *fev += 1;
        f(x)
    };
    let mut fsim: Vec<f64> = sim.iter().map(|x| call(x, &mut fev)).collect();
    let sort = |sim: &mut Vec<Vec<f64>>, fsim: &mut Vec<f64>| {
        let mut idx: Vec<usize> = (0..fsim.len()).collect();
        idx.sort_by(|&a, &b| fsim[a].total_cmp(&fsim[b]));
        *sim = idx.iter().map(|&i| sim[i].clone()).collect();
        *fsim = idx.iter().map(|&i| fsim[i]).collect();
    };
    sort(&mut sim, &mut fsim);
    let comb = |a: f64, x: &[f64], b: f64, y: &[f64]| -> Vec<f64> { x.iter().zip(y).map(|(p, q)| a * p + b * q).collect() };
    let mut iterations = 1;
    while fev < max_fev && iterations < max_iter {
        let xspread = sim[1..].iter().flat_map(|s| s.iter().zip(&sim[0]).map(|(a, b)| (a - b).abs())).fold(0.0, f64::max);
        let fspread = fsim[1..].iter().map(|v| (fsim[0] - v).abs()).fold(0.0, f64::max);
        if xspread <= xatol && fspread <= fatol {
            break;
        }
        let mut xbar = vec![0.0; n];
        for s in &sim[..n] {
            for (b, v) in xbar.iter_mut().zip(s) {
                *b += v;
            }
        }
        for b in &mut xbar {
            *b /= n as f64;
        }
        let worst = sim[n].clone();
        let xr = comb(1.0 + rho, &xbar, -rho, &worst);
        let fxr = call(&xr, &mut fev);
        let mut shrink = false;
        if fxr < fsim[0] {
            let xe = comb(1.0 + rho * chi, &xbar, -rho * chi, &worst);
            let fxe = call(&xe, &mut fev);
            if fxe < fxr {
                (sim[n], fsim[n]) = (xe, fxe);
            } else {
                (sim[n], fsim[n]) = (xr, fxr);
            }
        } else if fxr < fsim[n - 1] {
            (sim[n], fsim[n]) = (xr, fxr);
        } else if fxr < fsim[n] {
            let xc = comb(1.0 + psi * rho, &xbar, -psi * rho, &worst);
            let fxc = call(&xc, &mut fev);
            if fxc <= fxr {
                (sim[n], fsim[n]) = (xc, fxc);
            } else {
                shrink = true;
            }
        } else {
            let xcc = comb(1.0 - psi, &xbar, psi, &worst);
            let fxcc = call(&xcc, &mut fev);
            if fxcc < fsim[n] {
                (sim[n], fsim[n]) = (xcc, fxcc);
            } else {
                shrink = true;
            }
        }
        if shrink {
            for j in 1..=n {
                let s: Vec<f64> = sim[0].iter().zip(&sim[j]).map(|(b, v)| b + sigma * (v - b)).collect();
                fsim[j] = call(&s, &mut fev);
                sim[j] = s;
            }
        }
        sort(&mut sim, &mut fsim);
        iterations += 1;
    }
    (sim[0].clone(), fsim[0])
}

/// Least squares of the residuals `r(x)` by Levenberg-Marquardt with a numerical Jacobian; runs until the step stops
/// changing `x` (relative 1e-13) or 500 iterations.
pub fn levenberg_marquardt(r: &impl Fn(&[f64]) -> Vec<f64>, x0: &[f64]) -> Vec<f64> {
    let n = x0.len();
    let cost = |x: &[f64]| r(x).iter().map(|v| v * v).sum::<f64>();
    let mut x = x0.to_vec();
    let mut c = cost(&x);
    let mut lambda = 1e-3;
    for _ in 0..500 {
        let r0 = r(&x);
        let m = r0.len();
        let mut jac = vec![vec![0.0; n]; m];
        for k in 0..n {
            let h = 1e-7 * x[k].abs().max(1e-3);
            let (mut xp, mut xm) = (x.clone(), x.clone());
            xp[k] += h;
            xm[k] -= h;
            let (rp, rm) = (r(&xp), r(&xm));
            for i in 0..m {
                jac[i][k] = (rp[i] - rm[i]) / (2.0 * h);
            }
        }
        // (JᵀJ + λ diag(JᵀJ)) dx = −Jᵀr
        let mut a = vec![vec![0.0; n + 1]; n];
        for i in 0..n {
            for j in 0..n {
                a[i][j] = (0..m).map(|t| jac[t][i] * jac[t][j]).sum();
            }
            a[i][n] = -(0..m).map(|t| jac[t][i] * r0[t]).sum::<f64>();
        }
        let mut improved = false;
        for _ in 0..30 {
            let mut aa = a.clone();
            for (i, row) in aa.iter_mut().enumerate() {
                row[i] += lambda * a[i][i].max(1e-30);
            }
            let Some(dx) = solve(aa) else { break };
            let xn: Vec<f64> = x.iter().zip(&dx).map(|(p, d)| p + d).collect();
            let cn = cost(&xn);
            if cn.is_finite() && cn <= c {
                let small = dx.iter().zip(&x).all(|(d, p)| d.abs() <= 1e-13 * p.abs().max(1e-3));
                x = xn;
                c = cn;
                lambda = (lambda / 10.0).max(1e-15);
                improved = !small;
                break;
            }
            lambda *= 10.0;
        }
        if !improved {
            break;
        }
    }
    x
}

/// Gauss elimination with partial pivoting of an augmented matrix (n rows, n + 1 columns).
fn solve(mut m: Vec<Vec<f64>>) -> Option<Vec<f64>> {
    let n = m.len();
    for col in 0..n {
        let piv = (col..n).max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))?;
        if m[piv][col].abs() < 1e-300 {
            return None;
        }
        m.swap(col, piv);
        let pivot = m[col].clone();
        for (r, row) in m.iter_mut().enumerate() {
            if r != col {
                let f = row[col] / pivot[col];
                for (x, p) in row[col..].iter_mut().zip(&pivot[col..]) {
                    *x -= f * p;
                }
            }
        }
    }
    Some((0..n).map(|i| m[i][n] / m[i][i]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_written_as_python_writes_them() {
        for (x, s) in [(4.0919e-9, "4.092e-09"), (0.0001, "0.0001"), (1e-5, "1e-05"), (100.0, "100"), (0.38551, "0.3855"), (150.0, "150"), (333.04, "333"), (2.5e11, "2.5e+11"), (-0.89571, "-0.8957"), (12345.0, "1.234e+04"), (9.99996, "10")] {
            assert_eq!(g(x, 4), s, "{x}");
        }
        assert_eq!(g(0.3, 6), "0.3");
        assert_eq!(g(40e-6 * 1e6, 6), "40");
    }

    #[test]
    fn brent_finds_a_root_and_refuses_a_bracket_without_one() {
        let r = brentq(|x| x * x - 2.0, 0.0, 2.0).unwrap();
        assert!((r - 2f64.sqrt()).abs() < 1e-11);
        assert!(brentq(|x| x * x + 1.0, 0.0, 2.0).is_none());
    }

    #[test]
    fn nelder_mead_finds_the_bottom_of_a_valley() {
        let mut f = |x: &[f64]| (1.0 - x[0]).powi(2) + 100.0 * (x[1] - x[0] * x[0]).powi(2);
        let (x, c) = nelder_mead(&mut f, &[-1.2, 1.0], 20000, 20000, 1e-10, 1e-14);
        assert!(c < 1e-12 && (x[0] - 1.0).abs() < 1e-5 && (x[1] - 1.0).abs() < 1e-5, "{x:?} {c}");
    }

    #[test]
    fn levenberg_marquardt_fits_a_line_through_three_points() {
        let r = |x: &[f64]| [(1.0, 2.1), (2.0, 3.9), (3.0, 6.0)].iter().map(|(t, y)| x[0] * t + x[1] - y).collect::<Vec<_>>();
        let x = levenberg_marquardt(&r, &[0.0, 0.0]);
        assert!((x[0] - 1.95).abs() < 1e-7 && (x[1] - 0.1).abs() < 1e-7, "{x:?}");
    }
}
