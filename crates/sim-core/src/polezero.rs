//! Poles and zeros of a transfer function, found by fitting a rational function to a frequency response.
//!
//! The fit is the Sanathanan-Koerner iteration: H(s) ~ N(s)/D(s) with real coefficients, solved as a weighted linear least squares
//! on the sampled response (relative error, so a -80 dB stop band counts as much as the pass band). The order is the smallest
//! (zeros, poles) pair whose largest relative error is small, so an RC filter gives one pole and no zero, not a bundle of fit noise.
//! The roots of N and D are the zeros and poles. It works on any sampled response (a circuit with models, a macromodel op-amp,
//! the loop gain), not only on circuits that are linear all the way down.

use crate::ac::{ac_analysis, AcSettings, C64};
use crate::circuit::Circuit;
use crate::dc::SimError;
use std::f64::consts::TAU;

/// One pole or zero. Complex ones come in conjugate pairs; only the upper one of a pair is listed (`pair` = true).
#[derive(Clone, Debug, PartialEq)]
pub struct Root {
    /// Real part and imaginary part in rad/s.
    pub re: f64,
    pub im: f64,
    pub pair: bool,
}

impl Root {
    /// Distance from the origin, in Hz (the natural frequency of a pair, the corner of a real one).
    pub fn freq_hz(&self) -> f64 {
        self.re.hypot(self.im) / TAU
    }

    /// Quality factor of a complex pair: how sharp the peak is. `None` for a real root.
    pub fn q(&self) -> Option<f64> {
        (self.pair && self.re != 0.0).then(|| self.re.hypot(self.im) / (2.0 * self.re.abs()))
    }

    /// Damping ratio of a complex pair (1 / 2Q); the response rings when it is below about 0.7.
    pub fn damping(&self) -> Option<f64> {
        self.q().map(|q| 0.5 / q)
    }

    /// In the right half plane: the response grows by itself. For a zero it only changes the phase.
    pub fn right_half(&self) -> bool {
        self.re > 0.0
    }
}

#[derive(Clone, Debug)]
pub struct PoleZero {
    pub poles: Vec<Root>,
    pub zeros: Vec<Root>,
    /// Gain at DC (a pole at 0 makes it infinite; then it is the gain at the lowest frequency).
    pub dc_gain: f64,
    /// Largest relative error of the fit over the sweep (0.01 = 1 %).
    pub error: f64,
    /// Fitted response on the same frequencies.
    pub fitted: Vec<C64>,
}

impl PoleZero {
    pub fn order(&self) -> usize {
        self.poles.iter().map(|p| if p.pair { 2 } else { 1 }).sum()
    }
}

/// Least squares min |A x - b| by Householder QR. Columns are scaled to unit length first (the monomials span many decades).
fn least_squares(rows: usize, cols: usize, a: &mut [f64], b: &mut [f64]) -> Option<Vec<f64>> {
    let mut scale = vec![1.0; cols];
    for (c, s) in scale.iter_mut().enumerate() {
        let norm = (0..rows).map(|r| a[r * cols + c].powi(2)).sum::<f64>().sqrt();
        if !(norm > 0.0 && norm.is_finite()) {
            return None;
        }
        *s = norm;
        for r in 0..rows {
            a[r * cols + c] /= norm;
        }
    }
    for c in 0..cols {
        let norm = (c..rows).map(|r| a[r * cols + c].powi(2)).sum::<f64>().sqrt();
        if norm == 0.0 {
            return None;
        }
        let alpha = if a[c * cols + c] > 0.0 { -norm } else { norm };
        let mut v: Vec<f64> = (c..rows).map(|r| a[r * cols + c]).collect();
        v[0] -= alpha;
        let vv: f64 = v.iter().map(|x| x * x).sum();
        if vv == 0.0 {
            continue;
        }
        for k in c..cols {
            let dot: f64 = (c..rows).map(|r| v[r - c] * a[r * cols + k]).sum();
            for r in c..rows {
                a[r * cols + k] -= 2.0 * v[r - c] * dot / vv;
            }
        }
        let dot: f64 = (c..rows).map(|r| v[r - c] * b[r]).sum();
        for r in c..rows {
            b[r] -= 2.0 * v[r - c] * dot / vv;
        }
    }
    let mut x = vec![0.0; cols];
    for r in (0..cols).rev() {
        let mut s = b[r];
        for c in r + 1..cols {
            s -= a[r * cols + c] * x[c];
        }
        let d = a[r * cols + r];
        if d.abs() < 1e-14 {
            return None;
        }
        x[r] = s / d;
    }
    for (xi, s) in x.iter_mut().zip(&scale) {
        *xi /= s;
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

fn poly_eval(c: &[f64], x: C64) -> C64 {
    c.iter().rev().fold(C64::ZERO, |acc, &k| acc * x + C64::new(k, 0.0))
}

/// Roots of a real polynomial (coefficients lowest power first) by the Aberth-Ehrlich iteration.
fn roots(coef: &[f64]) -> Vec<C64> {
    let mut c = coef.to_vec();
    while c.len() > 1 && c.last().is_some_and(|v| *v == 0.0) {
        c.pop();
    }
    let n = c.len() - 1;
    if n == 0 {
        return vec![];
    }
    let lead = c[n];
    let radius = 1.0 + c[..n].iter().map(|v| (v / lead).abs()).fold(0.0, f64::max);
    let mut z: Vec<C64> = (0..n).map(|k| C64::from_polar(radius.min(1e3).max(1e-3) * 0.5, 360.0 * k as f64 / n as f64 + 20.0)).collect();
    let dcoef: Vec<f64> = (1..=n).map(|i| c[i] * i as f64).collect();
    for _ in 0..500 {
        let mut worst = 0.0f64;
        for i in 0..n {
            let p = poly_eval(&c, z[i]);
            let d = poly_eval(&dcoef, z[i]);
            if d.abs() == 0.0 {
                z[i] = z[i] + C64::new(1e-6, 1e-6);
                worst = f64::INFINITY;
                continue;
            }
            let ratio = p / d;
            let mut sum = C64::ZERO;
            for j in 0..n {
                if j != i {
                    let diff = z[i] - z[j];
                    if diff.abs() > 0.0 {
                        sum = sum + C64::new(1.0, 0.0) / diff;
                    }
                }
            }
            let denom = C64::new(1.0, 0.0) - ratio * sum;
            let step = if denom.abs() < 1e-300 { ratio } else { ratio / denom };
            z[i] = z[i] - step;
            worst = worst.max(step.abs() / z[i].abs().max(1e-300));
        }
        if worst < 1e-13 {
            break;
        }
    }
    z
}

/// Roots to `Root`s (x is in units of w0 rad/s): real ones are kept, a complex pair is listed once.
fn to_roots(r: Vec<C64>, w0: f64) -> Vec<Root> {
    let mut out = vec![];
    for z in r {
        let tiny = 1e-7 * z.abs().max(1e-12);
        if z.im.abs() <= tiny {
            out.push(Root { re: z.re * w0, im: 0.0, pair: false });
        } else if z.im > 0.0 {
            out.push(Root { re: z.re * w0, im: z.im * w0, pair: true });
        }
    }
    out.sort_by(|a, b| a.freq_hz().total_cmp(&b.freq_hz()));
    out
}

struct Fit {
    num: Vec<f64>,
    den: Vec<f64>,
    error: f64,
    fitted: Vec<C64>,
}

fn fit_order(x: &[C64], h: &[C64], floor: f64, m: usize, n: usize) -> Option<Fit> {
    let cols = m + 1 + n;
    let k = x.len();
    if 2 * k <= cols + 2 {
        return None;
    }
    let mut den = vec![0.0; n + 1];
    den[0] = 1.0;
    let mut num = vec![0.0; m + 1];
    for _ in 0..12 {
        let mut a = vec![0.0; 2 * k * cols];
        let mut b = vec![0.0; 2 * k];
        for i in 0..k {
            let w = 1.0 / (h[i].abs().max(floor) * poly_eval(&den, x[i]).abs().max(1e-300));
            let mut xp = C64::new(1.0, 0.0);
            let mut powers = vec![];
            for _ in 0..=m.max(n) {
                powers.push(xp);
                xp = xp * x[i];
            }
            for j in 0..=m {
                a[(2 * i) * cols + j] = w * powers[j].re;
                a[(2 * i + 1) * cols + j] = w * powers[j].im;
            }
            for j in 1..=n {
                let t = h[i] * powers[j];
                a[(2 * i) * cols + m + j] = -w * t.re;
                a[(2 * i + 1) * cols + m + j] = -w * t.im;
            }
            b[2 * i] = w * h[i].re;
            b[2 * i + 1] = w * h[i].im;
        }
        let sol = least_squares(2 * k, cols, &mut a, &mut b)?;
        let moved = (1..=n).map(|j| (sol[m + j] - den[j]).abs() / (den[j].abs() + 1e-300)).fold(0.0, f64::max);
        num.copy_from_slice(&sol[..=m]);
        for j in 1..=n {
            den[j] = sol[m + j];
        }
        if n == 0 || moved < 1e-9 {
            break;
        }
    }
    let fitted: Vec<C64> = x.iter().map(|&xi| poly_eval(&num, xi) / poly_eval(&den, xi)).collect();
    let error = fitted.iter().zip(h).map(|(f, h)| (*f - *h).abs() / h.abs().max(floor)).fold(0.0, f64::max);
    error.is_finite().then_some(Fit { num, den, error, fitted })
}

/// Fit poles and zeros to `h` sampled at `freq` (Hz). The order is raised until the largest relative error is below `tolerance`
/// (0.02 is a good value); `max_poles` limits the search. Gives the best fit found when none reaches the tolerance, with its error,
/// so the caller can say the response is not a rational function of this size.
pub fn pole_zero(freq: &[f64], h: &[C64], max_poles: usize, tolerance: f64) -> Result<PoleZero, String> {
    if freq.len() != h.len() || freq.len() < 8 {
        return Err("極と零を求めるには、周波数の点が 8 点以上要ります (点数/デケードを増やしてください)".into());
    }
    let peak = h.iter().map(|v| v.abs()).fold(0.0, f64::max);
    if !(peak > 0.0 && peak.is_finite()) {
        return Err("応答がすべて 0 です。入力と出力のネットを確かめてください".into());
    }
    let floor = peak * 1e-9;
    // work in units of the middle frequency so the monomials stay near 1
    let w0 = TAU * (freq[0] * freq[freq.len() - 1]).sqrt();
    let x: Vec<C64> = freq.iter().map(|f| C64::new(0.0, TAU * f / w0)).collect();
    let max_poles = max_poles.clamp(1, 10);
    let mut orders: Vec<(usize, usize)> = (0..=max_poles).flat_map(|n| (0..=n).map(move |m| (m, n))).collect();
    orders.sort_by_key(|&(m, n)| (m + n, n));
    let mut best: Option<(usize, usize, Fit)> = None;
    for (m, n) in orders {
        let Some(fit) = fit_order(&x, h, floor, m, n) else { continue };
        let good = fit.error < tolerance;
        let better = best.as_ref().is_none_or(|(_, _, b)| fit.error < b.error * 0.8);
        if good {
            best = Some((m, n, fit));
            break;
        }
        if better {
            best = Some((m, n, fit));
        }
    }
    let Some((m, n, fit)) = best else {
        return Err("当てはめに失敗しました (周波数の範囲を狭めるか、点数を増やしてください)".into());
    };
    let poles = to_roots(roots(&fit.den), w0);
    let zeros = to_roots(roots(&fit.num), w0);
    let dc_gain = fit.num[0] / fit.den[0];
    let _ = (m, n);
    Ok(PoleZero { poles, zeros, dc_gain, error: fit.error, fitted: fit.fitted })
}

/// Transfer function V(output)/V(input) of a circuit (input = the node the AC source drives; without it, V(output) alone,
/// which is the transfer function when the source has an amplitude of 1) and its poles and zeros.
pub struct TransferFunction {
    pub freq: Vec<f64>,
    pub h: Vec<C64>,
    pub pz: PoleZero,
    pub warnings: Vec<String>,
}

pub fn transfer_function(circuit: &Circuit, output: &str, input: Option<&str>, s: &AcSettings, max_poles: usize, tolerance: f64) -> Result<TransferFunction, SimError> {
    let r = ac_analysis(circuit, s)?;
    let get = |name: &str| {
        let key = format!("V({name})");
        r.signals
            .iter()
            .find(|sig| sig.name.eq_ignore_ascii_case(&key))
            .map(|sig| sig.re.iter().zip(&sig.im).map(|(re, im)| C64::new(*re, *im)).collect::<Vec<_>>())
            .ok_or_else(|| SimError::Invalid(format!("ネット {name} が見つかりません")))
    };
    let out = get(output)?;
    let h: Vec<C64> = match input {
        Some(i) => {
            let inp = get(i)?;
            out.iter().zip(&inp).map(|(o, i)| if i.abs() > 0.0 { *o / *i } else { C64::ZERO }).collect()
        }
        None => out,
    };
    let pz = pole_zero(&r.freq, &h, max_poles, tolerance).map_err(SimError::Invalid)?;
    Ok(TransferFunction { freq: r.freq, h, pz, warnings: r.warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sweep() -> Vec<f64> {
        (0..=120).map(|i| 10f64.powf(1.0 + i as f64 * 0.05)).collect()
    }

    fn resp(f: f64, k: f64, zeros: &[f64], poles: &[f64]) -> C64 {
        // real zeros and poles in Hz, all in the left half plane
        let s = C64::new(0.0, f);
        let mut h = C64::new(k, 0.0);
        for z in zeros {
            h = h * (C64::new(1.0, 0.0) + s / C64::new(*z, 0.0));
        }
        for p in poles {
            h = h / (C64::new(1.0, 0.0) + s / C64::new(*p, 0.0));
        }
        h
    }

    #[test]
    fn one_pole_is_found_with_no_zero() {
        let f = sweep();
        let h: Vec<C64> = f.iter().map(|&f| resp(f, 2.0, &[], &[1000.0])).collect();
        let pz = pole_zero(&f, &h, 6, 0.01).unwrap();
        assert_eq!(pz.poles.len(), 1);
        assert!(pz.zeros.is_empty());
        assert!((pz.poles[0].freq_hz() - 1000.0).abs() < 1.0, "{:?}", pz.poles);
        assert!(pz.poles[0].re < 0.0);
        assert!((pz.dc_gain - 2.0).abs() < 1e-3);
    }

    #[test]
    fn two_poles_and_a_zero_over_four_decades() {
        let f = sweep();
        let h: Vec<C64> = f.iter().map(|&f| resp(f, 100.0, &[3000.0], &[50.0, 20000.0])).collect();
        let pz = pole_zero(&f, &h, 6, 0.01).unwrap();
        assert_eq!(pz.poles.len(), 2, "{pz:?}");
        assert_eq!(pz.zeros.len(), 1);
        assert!((pz.poles[0].freq_hz() / 50.0 - 1.0).abs() < 0.01);
        assert!((pz.poles[1].freq_hz() / 20000.0 - 1.0).abs() < 0.01);
        assert!((pz.zeros[0].freq_hz() / 3000.0 - 1.0).abs() < 0.01);
        assert!(pz.error < 0.01);
    }

    #[test]
    fn complex_pair_gives_q() {
        // 2nd-order low pass: f0 = 1 kHz, Q = 5
        let f = sweep();
        let w0 = TAU * 1000.0;
        let q = 5.0;
        let h: Vec<C64> = f
            .iter()
            .map(|&f| {
                let s = C64::new(0.0, TAU * f);
                C64::new(w0 * w0, 0.0) / (s * s + s * C64::new(w0 / q, 0.0) + C64::new(w0 * w0, 0.0))
            })
            .collect();
        let pz = pole_zero(&f, &h, 6, 0.01).unwrap();
        assert_eq!(pz.poles.len(), 1);
        assert!(pz.poles[0].pair);
        assert!((pz.poles[0].freq_hz() - 1000.0).abs() < 2.0);
        assert!((pz.poles[0].q().unwrap() - 5.0).abs() < 0.05);
        assert_eq!(pz.order(), 2);
    }

    #[test]
    fn right_half_plane_zero_is_flagged() {
        let f = sweep();
        let h: Vec<C64> = f
            .iter()
            .map(|&f| {
                let s = C64::new(0.0, f);
                (C64::new(1.0, 0.0) - s / C64::new(5000.0, 0.0)) / (C64::new(1.0, 0.0) + s / C64::new(500.0, 0.0))
            })
            .collect();
        let pz = pole_zero(&f, &h, 6, 0.01).unwrap();
        assert_eq!(pz.zeros.len(), 1);
        assert!(pz.zeros[0].right_half());
        assert!(!pz.poles[0].right_half());
    }

    #[test]
    fn too_few_points_is_an_error() {
        let f = [1.0, 2.0, 3.0];
        let h = [C64::new(1.0, 0.0); 3];
        assert!(pole_zero(&f, &h, 4, 0.01).is_err());
    }
}
