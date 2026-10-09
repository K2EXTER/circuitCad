//! Monte Carlo: real parts are never exactly their printed value. Draw every resistor, capacitor
//! and inductor at random inside its tolerance, solve the DC operating point each time, and see how
//! far one node voltage wanders. A one-at-a-time sensitivity run tells which part is responsible.

use crate::circuit::{Circuit, Element};
use crate::dc::{dc_op, SimError};

#[derive(Clone, Debug)]
pub struct McSettings {
    /// Number of random circuits.
    pub runs: usize,
    pub seed: u64,
    /// Tolerances as fractions (0.05 = ±5 %).
    pub tol_r: f64,
    pub tol_c: f64,
    pub tol_l: f64,
    /// Node whose DC voltage is measured.
    pub output: String,
}

#[derive(Clone, Debug)]
pub struct McResult {
    pub output: String,
    /// The voltage with every part at its printed value.
    pub nominal: f64,
    /// One voltage per successful run.
    pub values: Vec<f64>,
    /// Runs that did not converge.
    pub failed: usize,
    /// For each part with a tolerance: the change of the output when only that part is
    /// `+tolerance` high, sorted by size (largest first).
    pub sensitivity: Vec<(String, f64)>,
}

#[derive(Clone, Copy, Debug)]
pub struct McStats {
    pub mean: f64,
    pub std: f64,
    pub min: f64,
    pub max: f64,
}

impl McResult {
    pub fn stats(&self) -> Option<McStats> {
        let n = self.values.len();
        if n == 0 {
            return None;
        }
        let mean = self.values.iter().sum::<f64>() / n as f64;
        let var = self.values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
        Some(McStats {
            mean,
            std: var.sqrt(),
            min: self.values.iter().cloned().fold(f64::INFINITY, f64::min),
            max: self.values.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        })
    }

    /// Counts per bin over `[min, max]`; empty when there are no values.
    pub fn histogram(&self, bins: usize) -> (f64, f64, Vec<usize>) {
        let Some(s) = self.stats() else { return (0.0, 0.0, vec![]) };
        let (lo, hi) = if s.max - s.min < 1e-15 { (s.min - 0.5e-6, s.max + 0.5e-6) } else { (s.min, s.max) };
        let mut h = vec![0; bins.max(1)];
        for v in &self.values {
            let k = (((v - lo) / (hi - lo)) * h.len() as f64) as usize;
            let last = h.len() - 1;
            h[k.min(last)] += 1;
        }
        (lo, hi, h)
    }
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Uniform in [0, 1), number `k` of stream `seed`.
fn uniform(seed: u64, k: u64) -> f64 {
    (splitmix64(seed ^ k.wrapping_mul(0xD6E8_FEB8_6659_FD93)) >> 11) as f64 / (1u64 << 53) as f64
}

/// Tolerance of an element, if it has one. Parasitic parts (`R1.ESR`) are left alone.
fn tolerance(e: &Element, s: &McSettings) -> Option<f64> {
    if e.name().contains('.') {
        return None;
    }
    let t = match e {
        Element::Resistor { .. } => s.tol_r,
        Element::Capacitor { .. } => s.tol_c,
        Element::Inductor { .. } => s.tol_l,
        _ => return None,
    };
    (t > 0.0).then_some(t)
}

fn scale(e: &mut Element, f: f64) {
    match e {
        Element::Resistor { r, .. } => *r *= f,
        Element::Capacitor { c, .. } => *c *= f,
        Element::Inductor { l, .. } => *l *= f,
        _ => {}
    }
}

fn measure(c: &Circuit, node: &str) -> Result<f64, SimError> {
    let r = dc_op(c)?;
    r.voltage(node).ok_or_else(|| SimError::Invalid(format!("ノード {node} が見つかりません")))
}

pub fn monte_carlo(circuit: &Circuit, s: &McSettings) -> Result<McResult, SimError> {
    if s.runs == 0 || s.runs > 20_000 {
        return Err(SimError::Invalid("回数は 1〜20000 にしてください".into()));
    }
    if [s.tol_r, s.tol_c, s.tol_l].iter().any(|t| !(0.0..1.0).contains(t)) {
        return Err(SimError::Invalid("許容差は 0〜100 % 未満にしてください".into()));
    }
    let output = s.output.trim().to_string();
    let nominal = measure(circuit, &output)?;
    let toleranced: Vec<(usize, f64)> = circuit.elements.iter().enumerate().filter_map(|(i, e)| tolerance(e, s).map(|t| (i, t))).collect();
    if toleranced.is_empty() {
        return Err(SimError::Invalid("ばらつかせる部品 (抵抗・コンデンサ・コイル) がありません。許容差を 0 より大きくしてください".into()));
    }

    let mut values = vec![];
    let mut failed = 0;
    for run in 0..s.runs {
        let mut c = circuit.clone();
        for (k, &(i, tol)) in toleranced.iter().enumerate() {
            let u = uniform(s.seed, (run * toleranced.len() + k) as u64);
            scale(&mut c.elements[i], 1.0 + tol * (2.0 * u - 1.0));
        }
        match measure(&c, &output) {
            Ok(v) if v.is_finite() => values.push(v),
            _ => failed += 1,
        }
    }

    let mut sensitivity = vec![];
    for &(i, tol) in &toleranced {
        let mut c = circuit.clone();
        scale(&mut c.elements[i], 1.0 + tol);
        if let Ok(v) = measure(&c, &output) {
            sensitivity.push((circuit.elements[i].name().to_string(), v - nominal));
        }
    }
    sensitivity.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
    Ok(McResult { output, nominal, values, failed, sensitivity })
}
