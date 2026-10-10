//! Step-response figures of a transient waveform: rise time, overshoot and settling time.
//! The waveform is assumed to start at rest, jump to a new level and settle there; anything
//! else (a periodic signal, a response that has not settled yet) is reported with the reason.

use crate::spectrum::stats;

#[derive(Clone, Debug, PartialEq)]
pub struct StepMetrics {
    /// Value at the start.
    pub initial: f64,
    /// Value it settles at (mean of the last tenth of the run).
    pub final_value: f64,
    /// Time to go from 10 % to 90 % of the step.
    pub rise_time: Option<f64>,
    /// How far it shoots past the final value, in percent of the step.
    pub overshoot_pct: f64,
    /// Time from the 10 % point until it stays within ±2 % of the final value.
    pub settling_time: Option<f64>,
}

/// Interpolated first time the normalised response `(v - initial) / step` reaches `frac`.
fn crossing(t: &[f64], v: &[f64], initial: f64, step: f64, frac: f64) -> Option<f64> {
    let norm = |i: usize| (v[i] - initial) / step;
    (1..t.len()).find(|&i| norm(i - 1) < frac && norm(i) >= frac).map(|i| {
        let (a, b) = (norm(i - 1), norm(i));
        t[i - 1] + (t[i] - t[i - 1]) * (frac - a) / (b - a)
    })
}

pub fn step_metrics(t: &[f64], v: &[f64]) -> Result<StepMetrics, String> {
    if t.len() < 5 || t.len() != v.len() {
        return Err("点数が足りません".into());
    }
    let (t0, t1) = (t[0], t[t.len() - 1]);
    let tail = stats(t, v, t1 - 0.1 * (t1 - t0), t1).ok_or("最後の区間を調べられません")?;
    let (initial, final_value) = (v[0], tail.mean);
    let step = final_value - initial;
    let (lo, hi) = (v.iter().cloned().fold(f64::INFINITY, f64::min), v.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    let ptp = hi - lo;
    if ptp < 1e-12 || step.abs() < 0.1 * ptp {
        return Err("最初と最後の値がほぼ同じです。ステップ応答ではない波形 (周期的な信号など) には使えません".into());
    }
    let tail_from = t1 - 0.1 * (t1 - t0);
    let (tlo, thi) = (0..t.len()).filter(|&i| t[i] >= tail_from).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), i| (a.min(v[i]), b.max(v[i])));
    if tail.rms_ac > 0.02 * step.abs() || thi - tlo > 0.05 * step.abs() {
        return Err("最後まで落ち着いていません。終了時間を長くして、もう一度解析してください".into());
    }

    let t10 = crossing(t, v, initial, step, 0.1);
    let t90 = crossing(t, v, initial, step, 0.9);
    let rise_time = t10.zip(t90).map(|(a, b)| b - a);
    let beyond = if step > 0.0 { hi - final_value } else { final_value - lo };
    let overshoot_pct = (beyond / step.abs() * 100.0).max(0.0);

    let band = 0.02 * step.abs();
    let last_out = (0..t.len()).rev().find(|&i| (v[i] - final_value).abs() > band);
    let settling_time = t10.map(|start| match last_out {
        None => 0.0,
        Some(i) if i + 1 < t.len() => {
            // linear interpolation to the instant the waveform re-enters the band
            let (a, b) = ((v[i] - final_value).abs(), (v[i + 1] - final_value).abs());
            let k = if a > b { (a - band) / (a - b) } else { 1.0 };
            (t[i] + (t[i + 1] - t[i]) * k - start).max(0.0)
        }
        Some(_) => t1 - start,
    });
    Ok(StepMetrics { initial, final_value, rise_time, overshoot_pct, settling_time })
}
