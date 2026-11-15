//! Transient heat resistance (Zth): how hot a part gets a time `t` after a power step, as a sum of first-order stages (the Foster form
//! data sheets fit their curves with): `Zth(t) = Σ R (1 − e^(−t/τ))`, `R` in K/W and `τ` in s. A single stage (θ with one heat capacity)
//! gets the peak of a short pulse wrong: the junction warms within milliseconds, the case and the board in seconds.
//!
//! `fit_foster` finds the stages from points read off a data sheet's Zth curve: the time constants are spread evenly on a log scale
//! over the points' times (the spread and the place are searched), and for each spread the heat resistances are a non-negative
//! least-squares fit (all subsets of the stages are tried, which is cheap for a few stages), on the relative error (the curve is read
//! on log axes).

/// `Zth(t)` of the stages `[R, τ]` (K/W).
pub fn zth_at(stages: &[[f64; 2]], t: f64) -> f64 {
    stages.iter().map(|[r, tau]| r * (1.0 - (-t / tau).exp())).sum()
}

/// Least squares for `R` on the chosen stages (columns), weighted by `1/z`: `None` when a heat resistance comes out negative or the
/// system is singular. Returns (R by column, relative rms error).
fn fit_r(points: &[(f64, f64)], taus: &[f64]) -> Option<(Vec<f64>, f64)> {
    let n = taus.len();
    let mut a = vec![vec![0.0; n]; n];
    let mut b = vec![0.0; n];
    for &(t, z) in points {
        let phi: Vec<f64> = taus.iter().map(|tau| (1.0 - (-t / tau).exp()) / z).collect();
        for i in 0..n {
            b[i] += phi[i];
            for j in 0..n {
                a[i][j] += phi[i] * phi[j];
            }
        }
    }
    // Gaussian elimination with pivoting
    for i in 0..n {
        let p = (i..n).max_by(|&x, &y| a[x][i].abs().total_cmp(&a[y][i].abs()))?;
        if a[p][i].abs() < 1e-30 {
            return None;
        }
        a.swap(i, p);
        b.swap(i, p);
        for j in i + 1..n {
            let f = a[j][i] / a[i][i];
            for k in i..n {
                a[j][k] -= f * a[i][k];
            }
            b[j] -= f * b[i];
        }
    }
    let mut r = vec![0.0; n];
    for i in (0..n).rev() {
        r[i] = (b[i] - (i + 1..n).map(|k| a[i][k] * r[k]).sum::<f64>()) / a[i][i];
    }
    if r.iter().any(|x| !(x.is_finite() && *x > 0.0)) {
        return None;
    }
    let stages: Vec<[f64; 2]> = r.iter().zip(taus).map(|(r, t)| [*r, *t]).collect();
    let err = (points.iter().map(|&(t, z)| ((zth_at(&stages, t) - z) / z).powi(2)).sum::<f64>() / points.len() as f64).sqrt();
    Some((r, err))
}

/// The best non-negative fit on some of the time constants `taus` (every non-empty subset is tried).
fn best_subset(points: &[(f64, f64)], taus: &[f64]) -> Option<(Vec<[f64; 2]>, f64)> {
    let mut best: Option<(Vec<[f64; 2]>, f64)> = None;
    for mask in 1u32..(1 << taus.len()) {
        let chosen: Vec<f64> = (0..taus.len()).filter(|i| mask & (1 << i) != 0).map(|i| taus[i]).collect();
        if let Some((r, err)) = fit_r(points, &chosen) {
            if best.as_ref().is_none_or(|b| err < b.1) {
                best = Some((r.into_iter().zip(chosen).map(|(r, t)| [r, t]).collect(), err));
            }
        }
    }
    best
}

/// Foster stages `[R K/W, τ s]` (at most `stages`, the shortest τ first) fitted to points `(t s, Zth K/W)` read off a data sheet's
/// curve, and the relative rms error of the fit.
pub fn fit_foster(points: &[(f64, f64)], stages: usize) -> Result<(Vec<[f64; 2]>, f64), String> {
    let pts: Vec<(f64, f64)> = points.iter().copied().filter(|(t, z)| t.is_finite() && z.is_finite()).collect();
    if pts.iter().any(|(t, z)| *t <= 0.0 || *z <= 0.0) {
        return Err("時間と Zth は、どちらも 0 より大きい数にしてください".into());
    }
    if pts.len() < 3 {
        return Err(format!("点が {} 個しかありません。曲線の上の点を 3 個以上 (できれば左の端から平らになる右の端まで 6〜10 個) 読んでください", pts.len()));
    }
    let n = stages.clamp(1, 6).min(pts.len());
    let (lo, hi) = pts.iter().fold((f64::MAX, 0.0_f64), |(lo, hi), (t, _)| (lo.min(*t), hi.max(*t)));
    let mut best: Option<(Vec<[f64; 2]>, f64)> = None;
    // the time constants: n of them evenly on a log scale; how far they reach past the points' times and where they sit is searched
    for spread in [0.5, 0.75, 1.0, 1.25, 1.5] {
        for shift in [-1.0, -0.5, -0.25, 0.0, 0.25, 0.5, 1.0] {
            let (a, b) = (lo.log10() - 0.5 * spread + shift, hi.log10() + 0.5 * spread + shift);
            let taus: Vec<f64> = (0..n).map(|i| 10f64.powf(if n == 1 { 0.5 * (a + b) } else { a + (b - a) * i as f64 / (n - 1) as f64 })).collect();
            if let Some(fit) = best_subset(&pts, &taus) {
                if best.as_ref().is_none_or(|b| fit.1 < b.1) {
                    best = Some(fit);
                }
            }
        }
    }
    let (mut st, mut err) = best.ok_or("当てはめられませんでした。点の時間と Zth の単位 (秒・K/W) を確かめてください")?;
    // then each time constant on its own: move it up or down by a factor, keep what lowers the error, halve the factor (on the log scale)
    let mut factor: f64 = 2.0;
    while factor > 1.001 {
        let mut moved = false;
        for i in 0..st.len() {
            for f in [factor, 1.0 / factor] {
                let mut taus: Vec<f64> = st.iter().map(|s| s[1]).collect();
                taus[i] *= f;
                if let Some((r, e)) = fit_r(&pts, &taus) {
                    if e < err {
                        st = r.into_iter().zip(taus).map(|(r, t)| [r, t]).collect();
                        err = e;
                        moved = true;
                    }
                }
            }
        }
        if !moved {
            factor = factor.sqrt();
        }
    }
    st.sort_by(|a, b| a[1].total_cmp(&b[1]));
    Ok((st, err))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_stages_are_found_again_from_their_curve() {
        // a MOSFET-like Zth: 0.2 K/W in 1 ms, 0.6 K/W in 20 ms, 1.2 K/W in 0.5 s (2 K/W in all)
        let truth = [[0.2, 1e-3], [0.6, 2e-2], [1.2, 0.5]];
        let pts: Vec<(f64, f64)> = (0..11).map(|k| 1e-4 * 10f64.powf(k as f64 * 0.5)).map(|t| (t, zth_at(&truth, t))).collect();
        let (st, err) = fit_foster(&pts, 4).unwrap();
        assert!(err < 0.01, "{err} {st:?}");
        for &(t, z) in &pts {
            assert!((zth_at(&st, t) / z - 1.0).abs() < 0.02, "t {t}: {} vs {z}", zth_at(&st, t));
        }
        let total: f64 = st.iter().map(|s| s[0]).sum();
        assert!((total - 2.0).abs() < 0.04, "{st:?}");
        assert!(st.windows(2).all(|w| w[0][1] < w[1][1]), "shortest first: {st:?}");
    }

    #[test]
    fn too_few_or_bad_points_are_explained() {
        assert!(fit_foster(&[(1e-3, 0.1), (1e-2, 0.5)], 3).unwrap_err().contains("3 個以上"));
        assert!(fit_foster(&[(1e-3, 0.1), (0.0, 0.5), (1.0, 1.0)], 3).unwrap_err().contains("0 より大きい"));
    }
}
