//! Diode model from data sheet readings: the saturation current `IS`, the emission coefficient `N` and
//! the series resistance `RS` that make `V = N·VT·ln(I/IS + 1) + I·RS` pass through points read off the
//! data sheet's forward characteristics.

/// Thermal voltage kT/q at 27 °C, the temperature the simulator takes as nominal.
pub const VT: f64 = 0.025865;

/// Result of [`fit`].
#[derive(Clone, Debug, PartialEq)]
pub struct DiodeFit {
    pub is: f64,
    pub n: f64,
    pub rs: f64,
    /// Things the learner should look at (a guessed value, an odd result).
    pub notes: Vec<String>,
}

/// Forward voltage of the model at current `i`.
pub fn vf(is: f64, n: f64, rs: f64, i: f64) -> f64 {
    n * VT * (i / is + 1.0).ln() + i * rs
}

/// Fit `IS`, `N`, `RS` to `(current A, voltage V)` points. Two points fix `IS` and `N` (the series
/// resistance is taken as 0); three or more also fix `RS`, so the highest-current point should be
/// well into the part's working range (the data sheet's rated current).
pub fn fit(points: &[(f64, f64)]) -> Result<DiodeFit, String> {
    if points.len() < 2 {
        return Err("点が足りません。電流と電圧の組を、2 点以上 (できれば小・中・大の電流で 3 点) 入れてください".into());
    }
    for (i, v) in points {
        if !(i.is_finite() && *i > 0.0) || !(v.is_finite() && *v > 0.0) {
            return Err("電流と電圧は、0 より大きい数で入れてください (例: 10m と 0.7)".into());
        }
    }
    let mut p = points.to_vec();
    p.sort_by(|a, b| a.0.total_cmp(&b.0));
    if p.windows(2).any(|w| w[1].0 < w[0].0 * 1.5) {
        return Err("電流が近すぎる点があります。電流は 10 倍ずつなど、離れた値で読んでください (近いと傾きが決まりません)".into());
    }
    // V = a·ln(I) + c + r·I with a = N·VT, c = -a·ln(IS): linear in (a, c, r)
    let rows: Vec<[f64; 3]> = p.iter().map(|(i, _)| [i.ln(), 1.0, *i]).collect();
    let vs: Vec<f64> = p.iter().map(|(_, v)| *v).collect();
    let mut notes = vec![];
    let mut sol = if p.len() >= 3 { least_squares(&rows, &vs, 3) } else { None };
    if let Some(s) = &sol
        && s[2] < 0.0
    {
        notes.push("電流が大きい所で電圧の上がりが小さいので、直列抵抗 RS は 0 にしました。大きい電流の点の読み取りを確かめてください".into());
        sol = None;
    }
    let (a, c, r) = match sol {
        Some(s) => (s[0], s[1], s[2]),
        None => {
            let s = least_squares(&rows, &vs, 2).ok_or("計算できませんでした。点の電流を変えて入れ直してください")?;
            if p.len() == 2 {
                notes.push("2 点だけなので、直列抵抗 RS は 0 にしました。定格電流に近い大電流の点を足すと、RS も決まります".into());
            }
            (s[0], s[1], 0.0)
        }
    };
    if a <= 0.0 {
        return Err("電流を増やしても電圧が増えていません。電圧の読み取りを確かめてください (電流が大きい点ほど、電圧も大きくなります)".into());
    }
    let n = a / VT;
    let is = (-c / a).exp();
    if !(0.5..=3.0).contains(&n) {
        notes.push(format!("N が {n:.2} になりました。ふつうの整流・スイッチングダイオードは 1〜2 ほどです。読み取った点を確かめてください"));
    }
    if !(is.is_finite() && is > 0.0) {
        return Err("IS を求められませんでした。点を確かめてください".into());
    }
    Ok(DiodeFit { is, n, rs: r, notes })
}

/// Least squares for the first `k` columns of `rows` (k = 2 or 3) by the normal equations.
fn least_squares(rows: &[[f64; 3]], y: &[f64], k: usize) -> Option<Vec<f64>> {
    let mut m = vec![vec![0.0; k + 1]; k];
    for (row, yv) in rows.iter().zip(y) {
        for i in 0..k {
            for j in 0..k {
                m[i][j] += row[i] * row[j];
            }
            m[i][k] += row[i] * yv;
        }
    }
    // Gauss elimination with partial pivoting
    for col in 0..k {
        let piv = (col..k).max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))?;
        if m[piv][col].abs() < 1e-12 {
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
    Some((0..k).map(|i| m[i][k] / m[i][i]).collect())
}

/// A name SPICE accepts for the `.model` card: letters, digits and `_`.
pub fn model_name(name: &str) -> String {
    let s: String = name.trim().chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if s.is_empty() { "DMODEL".into() } else { s }
}

/// `.model NAME D(IS=... N=... RS=... BV=... IBV=...)`; `bv` is the reverse voltage rating and `ibv` the
/// leakage current at it (both omitted when not given).
pub fn model_text(name: &str, is: f64, n: f64, rs: f64, bv: Option<f64>, ibv: Option<f64>) -> String {
    let mut t = format!(".model {} D(IS={} N={:.3} RS={}", model_name(name), sig(is), n, sig(rs));
    if let Some(b) = bv {
        t += &format!(" BV={}", sig(b));
        if let Some(i) = ibv {
            t += &format!(" IBV={}", sig(i));
        }
    }
    t + ")"
}

/// Three significant digits in a form SPICE reads back (`7.69e-11`).
fn sig(v: f64) -> String {
    format!("{v:.3e}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const IS: f64 = 76.9e-12;
    const N: f64 = 1.45;
    const RS: f64 = 0.042;

    fn pts(is: &[f64]) -> Vec<(f64, f64)> {
        is.iter().map(|&i| (i, vf(IS, N, RS, i))).collect()
    }

    #[test]
    fn three_points_recover_the_bundled_1n4007_model() {
        let f = fit(&pts(&[0.01, 0.1, 1.0])).unwrap();
        assert!((f.n - N).abs() < 0.02, "{f:?}");
        assert!((f.is / IS - 1.0).abs() < 0.2, "{f:?}");
        assert!((f.rs - RS).abs() < 0.005, "{f:?}");
        assert!(f.notes.is_empty(), "{:?}", f.notes);
    }

    #[test]
    fn the_fitted_model_passes_through_the_points() {
        let p = pts(&[0.001, 0.02, 0.3, 1.0]);
        let f = fit(&p).unwrap();
        for (i, v) in p {
            assert!((vf(f.is, f.n, f.rs, i) - v).abs() < 0.01, "{i}: {}", vf(f.is, f.n, f.rs, i));
        }
    }

    #[test]
    fn two_points_give_zero_series_resistance_and_say_so() {
        let f = fit(&pts(&[0.001, 0.01])).unwrap();
        assert_eq!(f.rs, 0.0);
        assert!((f.n - N).abs() < 0.05, "{f:?}");
        assert!(f.notes.iter().any(|n| n.contains("2 点")));
    }

    #[test]
    fn bad_readings_are_explained() {
        assert!(fit(&[(0.01, 0.7)]).unwrap_err().contains("点が足りません"));
        assert!(fit(&[(0.01, 0.7), (0.011, 0.71)]).unwrap_err().contains("近すぎ"));
        assert!(fit(&[(0.01, 0.8), (0.1, 0.7)]).unwrap_err().contains("電圧が増えていません"));
        assert!(fit(&[(0.0, 0.7), (0.1, 0.8)]).unwrap_err().contains("0 より大きい"));
    }

    #[test]
    fn a_negative_resistance_is_dropped_with_a_note() {
        // the top point's voltage is too low for any RS >= 0
        let f = fit(&[(0.001, 0.62), (0.01, 0.70), (0.1, 0.77), (1.0, 0.80)]).unwrap();
        assert_eq!(f.rs, 0.0);
        assert!(f.notes.iter().any(|n| n.contains("RS は 0")));
    }

    #[test]
    fn model_text_reads_back_in_the_simulator_format() {
        let t = model_text("1N4007", IS, N, RS, Some(1000.0), Some(5e-6));
        assert_eq!(t, ".model 1N4007 D(IS=7.690e-11 N=1.450 RS=4.200e-2 BV=1.000e3 IBV=5.000e-6)");
        assert_eq!(model_name("my part-1"), "my_part_1");
    }
}
