//! Monte Carlo: real parts are never exactly their printed value. Draw every resistor, capacitor
//! and inductor at random inside its tolerance, solve the circuit each time, and see how far one
//! result wanders: a node voltage (DC) or the -3 dB frequency of a filter (AC). A one-at-a-time
//! sensitivity run tells which part is responsible.

use crate::ac::{ac_analysis, AcSettings, AcSweep};
use crate::circuit::{Circuit, Element};
use crate::dc::{dc_op, SimError};
use crate::stepmetrics::step_metrics;
use crate::tran::{transient, TranSettings};

/// A figure of the step response of a transient run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TranMetric {
    RiseTime,
    Overshoot,
    SettlingTime,
    FinalValue,
}

/// What is measured on every random circuit.
#[derive(Clone, Debug, PartialEq)]
pub enum McMeasure {
    /// DC voltage of the output node.
    DcVoltage,
    /// The -3 dB frequency of the output node's gain (needs a source with `AC`). The gain is
    /// compared with its peak over the sweep; for a band-pass the lower edge is reported.
    Cutoff { fstart: f64, fstop: f64, points_per_decade: usize },
    /// A step-response figure of the output node in a transient run (the circuit must be driven by
    /// a step or pulse and the run long enough to settle).
    Tran { tstop: f64, tstep: f64, uic: bool, metric: TranMetric },
}

#[derive(Clone, Debug)]
pub struct McSettings {
    /// Number of random circuits.
    pub runs: usize,
    pub seed: u64,
    /// Tolerances as fractions (0.05 = ±5 %).
    pub tol_r: f64,
    pub tol_c: f64,
    pub tol_l: f64,
    /// Node that is measured.
    pub output: String,
    pub measure: McMeasure,
}

#[derive(Clone, Debug)]
pub struct McResult {
    pub output: String,
    /// "V", "Hz", "s" or "%".
    pub unit: &'static str,
    /// The value with every part at its printed value.
    pub nominal: f64,
    /// One value per successful run.
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

/// The frequency where the gain has dropped 3 dB from its peak, found by log-frequency
/// interpolation. `None` when the gain never crosses that level inside the sweep.
fn minus_3db(freq: &[f64], mag: &[f64]) -> Option<f64> {
    let peak = (0..mag.len()).max_by(|&a, &b| mag[a].total_cmp(&mag[b]))?;
    if mag[peak] <= 0.0 {
        return None;
    }
    let level = mag[peak] / std::f64::consts::SQRT_2;
    let cross = |lo: usize, hi: usize| {
        // gain is on opposite sides of `level` at lo and hi
        let (a, b) = (mag[lo].ln() - level.ln(), mag[hi].ln() - level.ln());
        let t = a / (a - b);
        (freq[lo].ln() + t * (freq[hi].ln() - freq[lo].ln())).exp()
    };
    let before = (0..peak).rev().find(|&j| mag[j] < level).map(|j| cross(j, j + 1));
    let after = (peak + 1..mag.len()).find(|&j| mag[j] < level).map(|j| cross(j - 1, j));
    before.or(after)
}

fn measure(c: &Circuit, node: &str, how: &McMeasure) -> Result<f64, SimError> {
    match how {
        McMeasure::DcVoltage => {
            let r = dc_op(c)?;
            r.voltage(node).ok_or_else(|| SimError::Invalid(format!("ノード {node} が見つかりません")))
        }
        McMeasure::Cutoff { fstart, fstop, points_per_decade } => {
            let r = ac_analysis(c, &AcSettings { fstart: *fstart, fstop: *fstop, sweep: AcSweep::Decade(*points_per_decade) })?;
            let sig = r.signal(&format!("V({node})")).ok_or_else(|| SimError::Invalid(format!("ノード {node} が見つかりません")))?;
            minus_3db(&r.freq, &sig.mag()).ok_or_else(|| SimError::Invalid("利得が -3dB に下がる周波数が、掃引の範囲内にありません。周波数の範囲を広げてください".into()))
        }
        McMeasure::Tran { tstop, tstep, uic, metric } => {
            let r = transient(c, &TranSettings { tstop: *tstop, tstep: *tstep, uic: *uic })?;
            let sig = r.signal(&format!("V({node})")).ok_or_else(|| SimError::Invalid(format!("ノード {node} が見つかりません")))?;
            let m = step_metrics(&r.time, &sig.values).map_err(SimError::Invalid)?;
            let missing = || SimError::Invalid("波形が 10% 〜 90% を通過していません".into());
            match metric {
                TranMetric::RiseTime => m.rise_time.ok_or_else(missing),
                TranMetric::Overshoot => Ok(m.overshoot_pct),
                TranMetric::SettlingTime => m.settling_time.ok_or_else(missing),
                TranMetric::FinalValue => Ok(m.final_value),
            }
        }
    }
}

pub fn monte_carlo(circuit: &Circuit, s: &McSettings) -> Result<McResult, SimError> {
    if s.runs == 0 || s.runs > 20_000 {
        return Err(SimError::Invalid("回数は 1〜20000 にしてください".into()));
    }
    if [s.tol_r, s.tol_c, s.tol_l].iter().any(|t| !(0.0..1.0).contains(t)) {
        return Err(SimError::Invalid("許容差は 0〜100 % 未満にしてください".into()));
    }
    let output = s.output.trim().to_string();
    let nominal = measure(circuit, &output, &s.measure)?;
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
        match measure(&c, &output, &s.measure) {
            Ok(v) if v.is_finite() => values.push(v),
            _ => failed += 1,
        }
    }

    let mut sensitivity = vec![];
    for &(i, tol) in &toleranced {
        let mut c = circuit.clone();
        scale(&mut c.elements[i], 1.0 + tol);
        if let Ok(v) = measure(&c, &output, &s.measure) {
            sensitivity.push((circuit.elements[i].name().to_string(), v - nominal));
        }
    }
    sensitivity.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
    let unit = match &s.measure {
        McMeasure::DcVoltage => "V",
        McMeasure::Cutoff { .. } => "Hz",
        McMeasure::Tran { metric: TranMetric::RiseTime | TranMetric::SettlingTime, .. } => "s",
        McMeasure::Tran { metric: TranMetric::Overshoot, .. } => "%",
        McMeasure::Tran { metric: TranMetric::FinalValue, .. } => "V",
    };
    Ok(McResult { output, unit, nominal, values, failed, sensitivity })
}
