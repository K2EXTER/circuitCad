//! Pass / fail checks of a design against target values (a "spec table").
//! Each spec reads one number from an analysis result that already exists and compares it with a range.

use crate::{minus_3db, stability_margins, stats, wave_measure, AcResult, DcResult, TranResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecKind {
    /// Voltage of a node in the DC operating point.
    DcVoltage,
    /// Current of a part in the DC operating point.
    DcCurrent,
    /// Mean of a transient signal over the second half of the run (after start-up).
    TranMean,
    /// Peak-to-peak of a transient signal over the second half (ripple).
    TranPp,
    /// Highest value over the whole run (overshoot, peak current).
    TranMax,
    /// Lowest value over the whole run (undershoot, dip).
    TranMin,
    /// -3 dB frequency of an AC signal.
    AcCutoff,
    /// Gain in dB at one frequency.
    AcGainAt,
    /// Phase margin of an AC loop-gain signal.
    PhaseMargin,
}

impl SpecKind {
    pub const ALL: [SpecKind; 9] = [
        SpecKind::DcVoltage,
        SpecKind::DcCurrent,
        SpecKind::TranMean,
        SpecKind::TranPp,
        SpecKind::TranMax,
        SpecKind::TranMin,
        SpecKind::AcCutoff,
        SpecKind::AcGainAt,
        SpecKind::PhaseMargin,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SpecKind::DcVoltage => "DC電圧",
            SpecKind::DcCurrent => "DC電流",
            SpecKind::TranMean => "平均値 (過渡の後半)",
            SpecKind::TranPp => "リップル p-p (過渡の後半)",
            SpecKind::TranMax => "最大値 (過渡)",
            SpecKind::TranMin => "最小値 (過渡)",
            SpecKind::AcCutoff => "-3dB点 (AC)",
            SpecKind::AcGainAt => "ゲイン (AC・ある周波数)",
            SpecKind::PhaseMargin => "位相余裕 (AC)",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            SpecKind::DcVoltage | SpecKind::TranMean | SpecKind::TranPp | SpecKind::TranMax | SpecKind::TranMin => "V",
            SpecKind::DcCurrent => "A",
            SpecKind::AcCutoff => "Hz",
            SpecKind::AcGainAt => "dB",
            SpecKind::PhaseMargin => "°",
        }
    }

    /// Which analysis has to be on screen for this spec.
    pub fn needs(self) -> &'static str {
        match self {
            SpecKind::DcVoltage | SpecKind::DcCurrent => "DC動作点",
            SpecKind::TranMean | SpecKind::TranPp | SpecKind::TranMax | SpecKind::TranMin => "過渡解析",
            SpecKind::AcCutoff | SpecKind::AcGainAt | SpecKind::PhaseMargin => "AC解析",
        }
    }

    /// The kind's target is a part name (otherwise a node / signal name).
    pub fn target_is_part(self) -> bool {
        self == SpecKind::DcCurrent
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub kind: SpecKind,
    /// Node name (DC), signal name `V(out)` (transient / AC) or part name (DC current).
    pub target: String,
    /// Frequency for `AcGainAt` (Hz); unused otherwise.
    pub at: f64,
    pub lo: Option<f64>,
    pub hi: Option<f64>,
}

/// The analysis results a spec can read.
#[derive(Clone, Copy, Default)]
pub struct Inputs<'a> {
    pub dc: Option<&'a DcResult>,
    pub tran: Option<&'a TranResult>,
    pub ac: Option<&'a AcResult>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpecEval {
    pub value: Option<f64>,
    /// `None` when the value could not be measured (see `note`).
    pub pass: Option<bool>,
    /// Why it could not be measured, or by how much it misses the range.
    pub note: String,
}

/// Is `v` inside `[lo, hi]` (either bound may be absent)? On a miss, says which side and by how much.
pub fn in_range(v: f64, lo: Option<f64>, hi: Option<f64>, unit: &str) -> (bool, String) {
    if let Some(l) = lo
        && v < l
    {
        return (false, format!("下限より {} 小さい", crate::format_value(l - v) + unit));
    }
    if let Some(h) = hi
        && v > h
    {
        return (false, format!("上限より {} 大きい", crate::format_value(v - h) + unit));
    }
    (true, String::new())
}

/// Gain in dB of an AC signal at `f`, interpolated on the log-frequency axis.
fn db_at(freq: &[f64], db: &[f64], f: f64) -> Option<f64> {
    if freq.len() < 2 || f < freq[0] || f > freq[freq.len() - 1] || f <= 0.0 {
        return None;
    }
    let i = freq.partition_point(|&x| x < f).clamp(1, freq.len() - 1);
    let (f0, f1) = (freq[i - 1], freq[i]);
    if f1 <= f0 {
        return Some(db[i]);
    }
    let t = (f.ln() - f0.ln()) / (f1.ln() - f0.ln());
    Some(db[i - 1] + (db[i] - db[i - 1]) * t)
}

/// Reads the number a spec asks for. `Err` carries a reason (analysis not run, signal missing ...).
pub fn measure(spec: &Spec, inp: &Inputs) -> Result<f64, String> {
    let need = |what: &str| format!("{what}を実行すると測れます");
    match spec.kind {
        SpecKind::DcVoltage => {
            let dc = inp.dc.ok_or_else(|| need("DC動作点"))?;
            dc.voltage(&spec.target).ok_or_else(|| format!("ノード {} が結果にありません", spec.target))
        }
        SpecKind::DcCurrent => {
            let dc = inp.dc.ok_or_else(|| need("DC動作点"))?;
            dc.element(&spec.target).map(|e| e.current).ok_or_else(|| format!("部品 {} が結果にありません", spec.target))
        }
        SpecKind::TranMean | SpecKind::TranPp | SpecKind::TranMax | SpecKind::TranMin => {
            let tr = inp.tran.ok_or_else(|| need("過渡解析"))?;
            let sg = tr.signal(&spec.target).ok_or_else(|| format!("信号 {} が結果にありません (波形の一覧で選べる名前です)", spec.target))?;
            let end = *tr.time.last().unwrap_or(&0.0);
            let (t0, t1) = if matches!(spec.kind, SpecKind::TranMean | SpecKind::TranPp) { (end / 2.0, end) } else { (0.0, end) };
            match spec.kind {
                SpecKind::TranMean => stats(&tr.time, &sg.values, t0, t1).map(|s| s.mean),
                SpecKind::TranPp => wave_measure(&tr.time, &sg.values, t0, t1).map(|m| m.pp),
                SpecKind::TranMax => stats(&tr.time, &sg.values, t0, t1).map(|s| s.max),
                _ => stats(&tr.time, &sg.values, t0, t1).map(|s| s.min),
            }
            .ok_or_else(|| "波形の点が足りず測れません".to_string())
        }
        SpecKind::AcCutoff | SpecKind::AcGainAt | SpecKind::PhaseMargin => {
            let ac = inp.ac.ok_or_else(|| need("AC解析"))?;
            let sg = ac.signal(&spec.target).ok_or_else(|| format!("信号 {} が結果にありません", spec.target))?;
            match spec.kind {
                SpecKind::AcCutoff => minus_3db(&ac.freq, &sg.mag()).ok_or_else(|| "-3dB点が計算範囲の中にありません".to_string()),
                SpecKind::AcGainAt => db_at(&ac.freq, &sg.db(), spec.at).ok_or_else(|| "その周波数が計算範囲の外です".to_string()),
                _ => stability_margins(&ac.freq, &sg.db(), &sg.phase_deg())
                    .and_then(|m| m.phase_margin)
                    .ok_or_else(|| "ゲインが 0dB を横切る点がなく、位相余裕を求められません".to_string()),
            }
        }
    }
}

pub fn evaluate(spec: &Spec, inp: &Inputs) -> SpecEval {
    match measure(spec, inp) {
        Err(note) => SpecEval { value: None, pass: None, note },
        Ok(v) => {
            let (ok, note) = in_range(v, spec.lo, spec.hi, spec.kind.unit());
            SpecEval { value: Some(v), pass: Some(ok), note }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AcSignal, Signal};

    fn spec(kind: SpecKind, target: &str, lo: Option<f64>, hi: Option<f64>) -> Spec {
        Spec { kind, target: target.into(), at: 0.0, lo, hi }
    }

    #[test]
    fn range_check_names_the_side_and_the_amount() {
        assert!(in_range(5.0, Some(4.75), Some(5.25), "V").0);
        let (ok, why) = in_range(4.5, Some(4.75), Some(5.25), "V");
        assert!(!ok && why.contains("下限") && why.contains("250m"), "{why}");
        let (ok, why) = in_range(6.0, None, Some(5.25), "V");
        assert!(!ok && why.contains("上限"), "{why}");
        assert!(in_range(1e9, None, None, "V").0);
    }

    #[test]
    fn a_missing_analysis_is_reported_not_failed() {
        let e = evaluate(&spec(SpecKind::DcVoltage, "OUT", Some(4.0), None), &Inputs::default());
        assert_eq!(e.pass, None);
        assert!(e.note.contains("DC動作点"));
    }

    #[test]
    fn transient_specs_use_the_second_half_for_mean_and_ripple() {
        // start-up ramp to 5 V in the first half, then 5 V +/- 0.1 V
        let time: Vec<f64> = (0..=200).map(|i| i as f64 * 1e-3).collect();
        let values: Vec<f64> = time.iter().map(|&t| if t < 0.1 { 50.0 * t } else { 5.0 + 0.1 * (t * 2000.0).sin().signum() }).collect();
        let tr = TranResult { time, signals: vec![Signal { name: "V(out)".into(), unit: "V", values }], warnings: vec![], retries: 0 };
        let inp = Inputs { tran: Some(&tr), ..Default::default() };
        let mean = evaluate(&spec(SpecKind::TranMean, "V(out)", Some(4.9), Some(5.1)), &inp);
        assert_eq!(mean.pass, Some(true), "{mean:?}");
        let pp = evaluate(&spec(SpecKind::TranPp, "V(out)", None, Some(0.1)), &inp);
        assert_eq!(pp.pass, Some(false), "ripple is 0.2 V p-p: {pp:?}");
        let max = evaluate(&spec(SpecKind::TranMax, "V(out)", None, Some(5.05)), &inp);
        assert_eq!(max.pass, Some(false));
        let bad = evaluate(&spec(SpecKind::TranMax, "V(nope)", None, None), &inp);
        assert_eq!(bad.pass, None);
    }

    #[test]
    fn ac_cutoff_and_gain_at_a_frequency() {
        // first-order low pass, fc = 1 kHz
        let freq: Vec<f64> = (0..=60).map(|i| 10f64.powf(1.0 + i as f64 * 0.1)).collect();
        let (re, im): (Vec<f64>, Vec<f64>) = freq
            .iter()
            .map(|f| {
                let d = 1.0 + (f / 1e3) * (f / 1e3);
                (1.0 / d, -(f / 1e3) / d)
            })
            .unzip();
        let ac = AcResult { freq, signals: vec![AcSignal { name: "V(out)".into(), unit: "V", re, im }], warnings: vec![] };
        let inp = Inputs { ac: Some(&ac), ..Default::default() };
        let fc = measure(&spec(SpecKind::AcCutoff, "V(out)", None, None), &inp).unwrap();
        assert!((fc / 1e3 - 1.0).abs() < 0.02, "{fc}");
        let mut g = spec(SpecKind::AcGainAt, "V(out)", Some(-23.0), Some(-17.0));
        g.at = 10e3; // one decade above fc: about -20 dB
        assert_eq!(evaluate(&g, &inp).pass, Some(true));
        g.at = 1e9;
        assert_eq!(evaluate(&g, &inp).pass, None, "out of range");
    }
}
