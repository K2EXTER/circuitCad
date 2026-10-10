//! Parameter sweep: run one analysis several times with one part's value changed, to see how a
//! signal moves as the value moves (R of a low-pass filter vs its cutoff, a damping resistor vs
//! the ringing, ...). The curves are meant to be drawn on top of each other.

use crate::{ac_analysis, format_value, from_schematic, parse_value, transient, AcSettings, AcSweep, SimError, TranSettings};
use sch_core::Schematic;

/// What to run for every value.
#[derive(Clone, Debug)]
pub enum ParamAnalysis {
    /// Transient response; the curve is V(node) over time.
    Tran { tstop: f64, tstep: f64, uic: bool },
    /// Frequency response; the curve is the gain of V(node) in dB over frequency.
    Ac { fstart: f64, fstop: f64, points_per_decade: usize },
}

#[derive(Clone, Debug)]
pub struct ParamSettings {
    /// Part number (R1, C2, ...) whose value is swept.
    pub refdes: String,
    pub values: Vec<f64>,
    /// Node (net name) whose voltage is drawn.
    pub node: String,
    pub temp_c: f64,
    pub analysis: ParamAnalysis,
}

#[derive(Clone, Debug)]
pub struct ParamCurve {
    pub value: f64,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct ParamResult {
    pub curves: Vec<ParamCurve>,
    /// Values whose run failed (no curve), with the reason.
    pub failed: Vec<(f64, String)>,
}

/// `n` values from `a` to `b`, spaced evenly or (if `log`, both positive) by a constant ratio.
pub fn sweep_values(a: f64, b: f64, n: usize, log: bool) -> Vec<f64> {
    if n < 2 {
        return vec![a];
    }
    (0..n)
        .map(|i| {
            let t = i as f64 / (n - 1) as f64;
            if log && a > 0.0 && b > 0.0 { a * (b / a).powf(t) } else { a + (b - a) * t }
        })
        .collect()
}

/// Value text with the number replaced and any trailing options (`ESR=20m`) kept.
pub fn with_number(old: &str, v: f64) -> String {
    match old.trim().split_once(char::is_whitespace) {
        Some((_, rest)) => format!("{} {}", format_value(v), rest.trim()),
        None => format_value(v),
    }
}

pub const MAX_RUNS: usize = 40;

pub fn param_sweep(base: &Schematic, s: &ParamSettings) -> Result<ParamResult, SimError> {
    if s.values.is_empty() || s.values.len() > MAX_RUNS {
        return Err(SimError::Invalid(format!("値の数は 1〜{MAX_RUNS} にしてください")));
    }
    let Some(idx) = base.components.iter().position(|c| c.refdes == s.refdes) else {
        return Err(SimError::Invalid(format!("部品 {} が見つかりません。部品番号 (R1 など) を確かめてください", s.refdes)));
    };
    if parse_value(base.components[idx].value.split_whitespace().next().unwrap_or("")).is_none() {
        return Err(SimError::Invalid(format!("{} の値「{}」は数値ではないのでスイープできません。抵抗・コンデンサ・コイルなど、値が数字の部品を選んでください", s.refdes, base.components[idx].value)));
    }
    let signal = format!("V({})", s.node);
    let mut out = ParamResult { curves: vec![], failed: vec![] };
    for &v in &s.values {
        let mut sch = base.clone();
        sch.components[idx].value = with_number(&base.components[idx].value, v);
        let run = || -> Result<(Vec<f64>, Vec<f64>), SimError> {
            let c = from_schematic(&sch).map_err(|e| SimError::Invalid(e.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" / ")))?.at_temperature(s.temp_c);
            match &s.analysis {
                ParamAnalysis::Tran { tstop, tstep, uic } => {
                    let r = transient(&c, &TranSettings { tstop: *tstop, tstep: *tstep, uic: *uic })?;
                    let sg = r.signal(&signal).ok_or_else(|| SimError::Invalid(format!("ノード {} が見つかりません", s.node)))?;
                    Ok((r.time.clone(), sg.values.clone()))
                }
                ParamAnalysis::Ac { fstart, fstop, points_per_decade } => {
                    let r = ac_analysis(&c, &AcSettings { fstart: *fstart, fstop: *fstop, sweep: AcSweep::Decade(*points_per_decade) })?;
                    let sg = r.signal(&signal).ok_or_else(|| SimError::Invalid(format!("ノード {} が見つかりません", s.node)))?;
                    Ok((r.freq.clone(), sg.db()))
                }
            }
        };
        match run() {
            Ok((x, y)) => out.curves.push(ParamCurve { value: v, x, y }),
            // a wrong node name fails every run the same way: report it once, as an error
            Err(e @ SimError::Invalid(_)) if out.curves.is_empty() && v == s.values[0] => return Err(e),
            Err(e) => out.failed.push((v, e.to_string())),
        }
    }
    if out.curves.is_empty() {
        let why = out.failed.first().map(|f| f.1.clone()).unwrap_or_default();
        return Err(SimError::Invalid(format!("どの値でも計算できませんでした: {why}")));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example(name: &str) -> Schematic {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn values_are_spaced_evenly_or_by_ratio() {
        assert_eq!(sweep_values(1.0, 3.0, 3, false), vec![1.0, 2.0, 3.0]);
        let l = sweep_values(10.0, 1000.0, 3, true);
        assert!((l[1] - 100.0).abs() < 1e-9);
        assert_eq!(sweep_values(5.0, 9.0, 1, false), vec![5.0]);
    }

    #[test]
    fn trailing_options_are_kept() {
        assert_eq!(with_number("100n ESR=20m", 2e-7), "200n ESR=20m");
        assert_eq!(with_number("10k", 4700.0), "4.7k");
    }

    #[test]
    fn bigger_r_lowers_the_lowpass_cutoff() {
        let sch = example("rc_lowpass.schjson");
        let r = sch.components.iter().find(|c| c.kind == sch_core::Kind::Resistor).unwrap().refdes.clone();
        let node = sch_core::extract(&sch).into_iter().find(|n| n.pins.iter().any(|p| p.refdes == "C1") && n.name != "GND").expect("the filter output net").name;
        let s = ParamSettings {
            refdes: r,
            values: vec![1e3, 10e3],
            node,
            temp_c: 27.0,
            analysis: ParamAnalysis::Ac { fstart: 10.0, fstop: 1e6, points_per_decade: 10 },
        };
        let res = param_sweep(&sch, &s).unwrap();
        assert_eq!(res.curves.len(), 2);
        // at a mid frequency the larger R attenuates more
        let k = res.curves[0].x.iter().position(|f| *f >= 1e3).unwrap();
        assert!(res.curves[1].y[k] < res.curves[0].y[k] - 3.0, "{} vs {}", res.curves[1].y[k], res.curves[0].y[k]);
    }

    #[test]
    fn unknown_part_and_node_are_reported() {
        let sch = example("rc_lowpass.schjson");
        let mk = |refdes: &str, node: &str| ParamSettings { refdes: refdes.into(), values: vec![1e3], node: node.into(), temp_c: 27.0, analysis: ParamAnalysis::Ac { fstart: 10.0, fstop: 1e3, points_per_decade: 5 } };
        assert!(param_sweep(&sch, &mk("R99", "OUT")).is_err());
        assert!(param_sweep(&sch, &mk("R1", "NOPE")).is_err());
    }
}
