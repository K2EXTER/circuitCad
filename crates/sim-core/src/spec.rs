//! Pass / fail checks of a design against target values (a "spec table").
//! Each spec reads one number from an analysis result that already exists and compares it with a range.

use crate::{minus_3db, stability_margins, stats, wave_measure, AcResult, DcResult, TranResult, WorstCase};

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
    /// Voltage of a node in the DC operating point at the worst case of the part tolerances: passes only when the smallest and
    /// the largest value both lie inside the range.
    DcVoltageWorst,
    /// -3 dB frequency at the worst case of the part tolerances.
    AcCutoffWorst,
}

impl SpecKind {
    pub const ALL: [SpecKind; 11] = [
        SpecKind::DcVoltage,
        SpecKind::DcCurrent,
        SpecKind::TranMean,
        SpecKind::TranPp,
        SpecKind::TranMax,
        SpecKind::TranMin,
        SpecKind::AcCutoff,
        SpecKind::AcGainAt,
        SpecKind::PhaseMargin,
        SpecKind::DcVoltageWorst,
        SpecKind::AcCutoffWorst,
    ];

    /// Stable name used in saved files (never rename; the labels may change).
    pub fn key(self) -> &'static str {
        match self {
            SpecKind::DcVoltage => "dc_voltage",
            SpecKind::DcCurrent => "dc_current",
            SpecKind::TranMean => "tran_mean",
            SpecKind::TranPp => "tran_pp",
            SpecKind::TranMax => "tran_max",
            SpecKind::TranMin => "tran_min",
            SpecKind::AcCutoff => "ac_cutoff",
            SpecKind::AcGainAt => "ac_gain_at",
            SpecKind::PhaseMargin => "phase_margin",
            SpecKind::DcVoltageWorst => "dc_voltage_worst",
            SpecKind::AcCutoffWorst => "ac_cutoff_worst",
        }
    }

    pub fn from_key(key: &str) -> Option<SpecKind> {
        SpecKind::ALL.into_iter().find(|k| k.key() == key)
    }

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
            SpecKind::DcVoltageWorst => "DC電圧 (最悪ケース)",
            SpecKind::AcCutoffWorst => "-3dB点 (最悪ケース)",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            SpecKind::DcVoltage | SpecKind::DcVoltageWorst | SpecKind::TranMean | SpecKind::TranPp | SpecKind::TranMax | SpecKind::TranMin => "V",
            SpecKind::DcCurrent => "A",
            SpecKind::AcCutoff | SpecKind::AcCutoffWorst => "Hz",
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
            SpecKind::DcVoltageWorst | SpecKind::AcCutoffWorst => "最悪ケース解析 (回路図から直接。許容差は「温度とばらつき」の欄)",
        }
    }

    /// Judged on the worst case of the part tolerances (`evaluate_worst`), not from an analysis result on screen.
    pub fn is_worst(self) -> bool {
        matches!(self, SpecKind::DcVoltageWorst | SpecKind::AcCutoffWorst)
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
        SpecKind::DcVoltageWorst | SpecKind::AcCutoffWorst => Err("最悪ケースは回路図から直接計算します (画面の表と、CLI の `worst` コマンド)".into()),
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

/// Judges a worst-case spec: it passes only when the whole range from the smallest to the largest value is inside `[lo, hi]`.
/// `value` is the nominal value; the note says what the range is and, on a miss, which end gets out and by how much.
pub fn evaluate_worst(spec: &Spec, w: &WorstCase) -> SpecEval {
    let unit = spec.kind.unit();
    let show = |v: f64| crate::format_value(v) + unit;
    let range = format!("最悪ケース {} 〜 {}", show(w.min), show(w.max));
    let (lo_ok, lo_note) = in_range(w.min, spec.lo, None, unit);
    let (hi_ok, hi_note) = in_range(w.max, None, spec.hi, unit);
    let note = match (lo_ok, hi_ok) {
        (true, true) => range,
        (false, true) => format!("{range} (最小が{lo_note})"),
        (true, false) => format!("{range} (最大が{hi_note})"),
        (false, false) => format!("{range} (最小が{lo_note}、最大が{hi_note})"),
    };
    SpecEval { value: Some(w.nominal), pass: Some(lo_ok && hi_ok), note }
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

// ------------------------------------------------------------------ bench measurement vs simulation

/// How well a bench measurement agrees with the simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agreement {
    /// Within what the parts' tolerances and the meter explain.
    Close,
    /// A visible difference that part tolerances or the measuring method can still explain.
    Some,
    /// Too large for that: something is connected, measured or modelled differently.
    Far,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredCompare {
    /// measured - simulated.
    pub diff: f64,
    /// `diff` as a fraction of the simulated value (None when that is 0).
    pub rel: Option<f64>,
    pub level: Agreement,
    /// Things to check, most likely first.
    pub hints: Vec<&'static str>,
}

/// (close, some) limits of |measured - simulated|: a fraction of the simulated value, but never less than a floor
/// (the meter's own resolution and accuracy).
fn allowance(kind: SpecKind) -> ((f64, f64), (f64, f64)) {
    match kind {
        // 1 % + a few digits of a hand-held meter; a 78xx alone is +/-4 %
        SpecKind::DcVoltage | SpecKind::DcVoltageWorst | SpecKind::TranMean | SpecKind::TranMax | SpecKind::TranMin => ((0.02, 0.02), (0.08, 0.08)),
        // a meter's current range drops a few 100 mV across itself
        SpecKind::DcCurrent => ((0.05, 2e-4), (0.15, 1e-3)),
        // an oscilloscope's bandwidth, probe and ground lead decide a ripple reading
        SpecKind::TranPp => ((0.25, 5e-3), (0.6, 2e-2)),
        SpecKind::AcCutoff | SpecKind::AcCutoffWorst => ((0.10, 0.0), (0.30, 0.0)),
        // in dB and degrees the allowance is absolute
        SpecKind::AcGainAt => ((0.0, 1.0), (0.0, 3.0)),
        SpecKind::PhaseMargin => ((0.0, 5.0), (0.0, 15.0)),
    }
}

fn hints_for(kind: SpecKind, level: Agreement, measured_is_more: bool) -> Vec<&'static str> {
    if level == Agreement::Close {
        return vec![];
    }
    let mut v: Vec<&'static str> = match kind {
        SpecKind::DcVoltage | SpecKind::DcVoltageWorst | SpecKind::TranMean | SpecKind::TranMax | SpecKind::TranMin => vec![
            "部品の誤差: 抵抗は ±1〜5%、ツェナーや基準電圧は ±5% ほど、7805 は ±4% ほどあります (データシートの範囲を見ます)",
            "測り方: テスタの精度 (±0.5〜1% + 数桁)、テスタ棒の接触、基準 (GND) の取り方を確かめます",
            "電源側: 電池や AC アダプタは、負荷をつなぐと電圧が下がります。つないだ状態で入力電圧も測ります",
            "配線: ブレッドボードの接触抵抗や、細い配線の電圧降下で、数十 mV 変わります",
        ],
        SpecKind::DcCurrent => vec![
            "テスタの電流レンジは、中で電圧が落ちます。そのぶん電流が小さく測れます (抵抗の電圧と抵抗値から計算し直すと確かめられます)",
            "部品の誤差: 抵抗の値のずれが、そのまま電流のずれになります",
            "つなぎ方: 電流は測りたい部品と直列に入れます。並列につなぐと、ヒューズが切れることがあります",
        ],
        SpecKind::TranPp => vec![
            "オシロの測り方: プローブの GND は短く (長いワニ口線は、拾った雑音がリップルに見えます)。帯域制限と AC カップリングも確かめます",
            "部品: 電解コンデンサの容量は −20〜+20% ずれ、古いものや低温で ESR が増えます (シミュレーションの ESR と比べます)",
            "テスタの交流レンジでは、リップルの形と大きさは測れません (実効値の目安だけです)",
        ],
        SpecKind::AcCutoff | SpecKind::AcCutoffWorst | SpecKind::AcGainAt => vec![
            "部品の誤差: コンデンサは ±10〜20%、抵抗は ±1〜5%。カットオフ周波数にそのまま出ます",
            "測る機器の入力: オシロやテスタの入力容量・抵抗が、回路につながって周波数特性を変えます",
            "高い周波数では、ブレッドボードの浮遊容量や配線のインダクタンスが効きます",
        ],
        SpecKind::PhaseMargin => vec![
            "位相余裕は、部品の誤差と配線の浮遊容量に敏感です。実機では、大きな余裕 (45° 以上) を見込みます",
            "モデルの限界: オペアンプやトランジスタのモデルは、実際の部品の一部の特性しか表せません",
        ],
    };
    if level == Agreement::Far {
        v.insert(0, "差が大きいときは、まず配線の間違い (つなぎ違い、抜けた脚、極性) と、測っている場所がシミュレーションと同じかを確かめます");
        v.push("それでも合わないときは、シミュレーションの部品値が、実際に使った部品と同じか確かめます (回路図の値を、手元の部品に合わせます)");
    }
    if matches!(kind, SpecKind::DcVoltage | SpecKind::DcVoltageWorst) && measured_is_more && level != Agreement::Close {
        v.push("測った値のほうが大きいとき: 負荷が軽い (つないだ抵抗が大きい)、または入力電圧が想定より高いことがあります");
    }
    v
}

/// Compares a bench measurement with the simulated value of the same spec.
pub fn compare_measured(kind: SpecKind, sim: f64, measured: f64) -> MeasuredCompare {
    let diff = measured - sim;
    let rel = (sim != 0.0).then(|| diff / sim.abs());
    let ((cr, cf), (sr, sf)) = allowance(kind);
    let limit = |frac: f64, floor: f64| (frac * sim.abs()).max(floor);
    let level = if diff.abs() <= limit(cr, cf) {
        Agreement::Close
    } else if diff.abs() <= limit(sr, sf) {
        Agreement::Some
    } else {
        Agreement::Far
    };
    MeasuredCompare { diff, rel, level, hints: hints_for(kind, level, diff > 0.0) }
}

#[cfg(test)]
mod measured_tests {
    use super::*;

    #[test]
    fn a_measurement_is_close_some_or_far_by_kind() {
        // 5.00 V simulated: 4.93 V is within the meter and the parts, 4.7 V is a visible miss, 4.0 V is something wrong
        let c = compare_measured(SpecKind::DcVoltage, 5.0, 4.93);
        assert_eq!(c.level, Agreement::Close);
        assert!(c.hints.is_empty());
        assert!((c.rel.unwrap() + 0.014).abs() < 1e-9);
        assert_eq!(compare_measured(SpecKind::DcVoltage, 5.0, 4.7).level, Agreement::Some);
        let far = compare_measured(SpecKind::DcVoltage, 5.0, 4.0);
        assert_eq!(far.level, Agreement::Far);
        assert!(far.hints[0].contains("配線の間違い"), "{:?}", far.hints);
        // the floor: 20 mV is fine around 0 V
        assert_eq!(compare_measured(SpecKind::DcVoltage, 0.0, 0.015).level, Agreement::Close);
        assert_eq!(compare_measured(SpecKind::DcVoltage, 0.0, 0.015).rel, None);
    }

    #[test]
    fn ripple_and_ac_have_their_own_allowances() {
        assert_eq!(compare_measured(SpecKind::TranPp, 0.100, 0.120).level, Agreement::Close);
        assert_eq!(compare_measured(SpecKind::TranPp, 0.100, 0.200).level, Agreement::Far);
        assert_eq!(compare_measured(SpecKind::AcCutoff, 1000.0, 1080.0).level, Agreement::Close);
        assert_eq!(compare_measured(SpecKind::AcCutoff, 1000.0, 1250.0).level, Agreement::Some);
        assert_eq!(compare_measured(SpecKind::AcGainAt, -20.0, -22.5).level, Agreement::Some);
        assert_eq!(compare_measured(SpecKind::PhaseMargin, 60.0, 40.0).level, Agreement::Far);
    }

    #[test]
    fn every_kind_has_hints_when_it_does_not_agree() {
        for k in SpecKind::ALL {
            let c = compare_measured(k, 1.0, 100.0);
            assert_eq!(c.level, Agreement::Far, "{k:?}");
            assert!(c.hints.len() >= 3, "{k:?}: {:?}", c.hints);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AcSignal, Signal};

    #[test]
    fn every_kind_round_trips_through_its_key() {
        for k in SpecKind::ALL {
            assert_eq!(SpecKind::from_key(k.key()), Some(k));
        }
        assert_eq!(SpecKind::from_key("nope"), None);
    }

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
        let tr = TranResult { time, signals: vec![Signal { name: "V(out)".into(), unit: "V", values }], warnings: vec![], retries: 0, rejected: 0 };
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

    #[test]
    fn a_worst_case_spec_passes_only_when_the_whole_range_fits() {
        let w = WorstCase {
            output: "OUT".into(),
            unit: "V",
            nominal: 5.0,
            min: 4.8,
            max: 5.2,
            min_sum: 4.8,
            max_sum: 5.2,
            min_rss: 4.9,
            max_rss: 5.1,
            parts: vec![],
            min_corner: vec![],
            max_corner: vec![],
            unsolved: vec![],
        };
        let spec = |lo, hi| Spec { kind: SpecKind::DcVoltageWorst, target: "OUT".into(), at: 0.0, lo, hi };
        assert_eq!(evaluate_worst(&spec(Some(4.75), Some(5.25)), &w).pass, Some(true));
        let low = evaluate_worst(&spec(Some(4.9), Some(5.25)), &w);
        assert_eq!(low.pass, Some(false));
        assert!(low.note.contains("最小") && !low.note.contains("最大"), "{}", low.note);
        let both = evaluate_worst(&spec(Some(4.9), Some(5.1)), &w);
        assert!(both.note.contains("最小") && both.note.contains("最大"), "{}", both.note);
        // one side only
        assert_eq!(evaluate_worst(&spec(None, Some(5.25)), &w).pass, Some(true));
        // the plain `measure` does not read it from an analysis result
        assert!(measure(&spec(None, None), &Inputs::default()).is_err());
    }
}
