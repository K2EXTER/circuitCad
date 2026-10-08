//! DC sweep: step one source through a range of values and solve the operating point at each
//! step (SPICE `.dc`). The result is a transfer characteristic, e.g. the output of an inverter
//! against its input.

use crate::circuit::{Circuit, Element};
use crate::dc::{dc_op, SimError};
use crate::waveform::Waveform;

#[derive(Clone, Debug)]
pub struct DcSweepSettings {
    /// Name of the V or I source to sweep (e.g. `V2`).
    pub source: String,
    pub start: f64,
    pub stop: f64,
    /// Step size; its sign is ignored (the sweep runs from `start` towards `stop`).
    pub step: f64,
}

#[derive(Clone, Debug)]
pub struct DcSweepSignal {
    /// `V(node)` or `I(element)`.
    pub name: String,
    pub unit: &'static str,
    pub values: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct DcSweepResult {
    pub source: String,
    /// The swept source's value at every point.
    pub x: Vec<f64>,
    pub signals: Vec<DcSweepSignal>,
    pub warnings: Vec<String>,
}

impl DcSweepResult {
    pub fn signal(&self, name: &str) -> Option<&DcSweepSignal> {
        self.signals.iter().find(|s| s.name == name)
    }
}

const MAX_POINTS: usize = 20_001;

fn sweep_values(s: &DcSweepSettings) -> Result<Vec<f64>, SimError> {
    if !(s.start.is_finite() && s.stop.is_finite() && s.step.is_finite()) || s.step == 0.0 {
        return Err(SimError::Invalid("開始・終了・刻みを有限の数値にしてください (刻みは 0 以外)".into()));
    }
    let span = s.stop - s.start;
    let step = s.step.abs();
    let n = (span.abs() / step + 1e-9).floor() as usize;
    if n + 1 > MAX_POINTS {
        return Err(SimError::Invalid(format!("点数が多すぎます (最大 {MAX_POINTS} 点)。刻みを大きくしてください")));
    }
    let dir = if span >= 0.0 { 1.0 } else { -1.0 };
    let mut v: Vec<f64> = (0..=n).map(|k| s.start + dir * step * k as f64).collect();
    // always end exactly on the stop value
    if (v[n] - s.stop).abs() > 1e-9 * span.abs().max(1e-30) {
        v.push(s.stop);
    } else {
        v[n] = s.stop;
    }
    Ok(v)
}

pub fn dc_sweep(circuit: &Circuit, s: &DcSweepSettings) -> Result<DcSweepResult, SimError> {
    let idx = circuit
        .elements
        .iter()
        .position(|e| matches!(e, Element::VSource { name, .. } | Element::ISource { name, .. } if *name == s.source))
        .ok_or_else(|| SimError::Invalid(format!("スイープする電源 {} が見つかりません (V / I 電源の番号を指定)", s.source)))?;
    let values = sweep_values(s)?;

    let mut c = circuit.clone();
    let mut signals: Vec<DcSweepSignal> = vec![];
    let mut warnings = vec![];
    for (k, &val) in values.iter().enumerate() {
        match &mut c.elements[idx] {
            Element::VSource { wave, .. } | Element::ISource { wave, .. } => *wave = Waveform::Dc(val),
            _ => unreachable!(),
        }
        let r = dc_op(&c).map_err(|e| match e {
            SimError::NoConvergence => SimError::Invalid(format!("{} = {val} で収束しませんでした", s.source)),
            other => other,
        })?;
        if k == 0 {
            warnings = r.warnings.clone();
            for (n, _) in r.voltages.iter().filter(|(n, _)| n != "GND") {
                signals.push(DcSweepSignal { name: format!("V({n})"), unit: "V", values: vec![] });
            }
            for e in &r.elements {
                signals.push(DcSweepSignal { name: format!("I({})", e.name), unit: "A", values: vec![] });
            }
        }
        for sg in signals.iter_mut() {
            let v = if let Some(n) = sg.name.strip_prefix("V(").and_then(|x| x.strip_suffix(')')) {
                r.voltage(n)
            } else {
                sg.name.strip_prefix("I(").and_then(|x| x.strip_suffix(')')).and_then(|n| r.element(n)).map(|e| e.current)
            };
            sg.values.push(v.unwrap_or(f64::NAN));
        }
    }
    Ok(DcSweepResult { source: s.source.clone(), x: values, signals, warnings })
}

/// Key numbers of an inverting transfer curve (output against input), the ones a digital
/// designer reads off: levels, switching threshold and noise margins.
#[derive(Clone, Debug, PartialEq)]
pub struct InverterMetrics {
    /// Output at the lowest input / highest input.
    pub voh: f64,
    pub vol: f64,
    /// Input at which output = input (switching threshold).
    pub vth: f64,
    /// Input range where the gain is steeper than -1: below `vil` the input counts as a clean
    /// low, above `vih` as a clean high.
    pub vil: f64,
    pub vih: f64,
    /// Steepest slope (most negative gain).
    pub max_gain: f64,
}

impl InverterMetrics {
    /// Low-level noise margin `VIL - VOL`.
    pub fn nml(&self) -> f64 {
        self.vil - self.vol
    }
    /// High-level noise margin `VOH - VIH`.
    pub fn nmh(&self) -> f64 {
        self.voh - self.vih
    }
}

/// `None` unless the curve is a falling one with a switching threshold.
pub fn inverter_metrics(x: &[f64], y: &[f64]) -> Option<InverterMetrics> {
    if x.len() != y.len() || x.len() < 5 || y.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let mut p: Vec<(f64, f64)> = x.iter().copied().zip(y.iter().copied()).collect();
    p.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (voh, vol) = (p[0].1, p[p.len() - 1].1);
    if voh - vol < 1e-3 {
        return None;
    }
    // switching threshold: y - x changes sign
    let mut vth = None;
    for w in p.windows(2) {
        let (f0, f1) = (w[0].1 - w[0].0, w[1].1 - w[1].0);
        if f0 > 0.0 && f1 <= 0.0 {
            let t = f0 / (f0 - f1);
            vth = Some(w[0].0 + t * (w[1].0 - w[0].0));
            break;
        }
    }
    let vth = vth?;
    // slope at segment midpoints; the unity-gain points are where it crosses -1
    let mid: Vec<(f64, f64)> = p.windows(2).filter(|w| w[1].0 > w[0].0).map(|w| ((w[0].0 + w[1].0) / 2.0, (w[1].1 - w[0].1) / (w[1].0 - w[0].0))).collect();
    let max_gain = mid.iter().map(|m| m.1).fold(f64::MAX, f64::min);
    if max_gain > -1.0 {
        // never steeper than unity: no regenerative region (not a logic inverter)
        return Some(InverterMetrics { voh, vol, vth, vil: vth, vih: vth, max_gain });
    }
    let cross = |a: (f64, f64), b: (f64, f64)| a.0 + (-1.0 - a.1) / (b.1 - a.1) * (b.0 - a.0);
    let mut vil = None;
    let mut vih = None;
    for w in mid.windows(2) {
        if w[0].1 > -1.0 && w[1].1 <= -1.0 && vil.is_none() {
            vil = Some(cross(w[0], w[1]));
        }
        if w[0].1 <= -1.0 && w[1].1 > -1.0 {
            vih = Some(cross(w[0], w[1]));
        }
    }
    // the transition may start at the first / end at the last segment
    let vil = vil.unwrap_or(mid[0].0);
    let vih = vih.unwrap_or(mid[mid.len() - 1].0);
    Some(InverterMetrics { voh, vol, vth, vil, vih, max_gain })
}
