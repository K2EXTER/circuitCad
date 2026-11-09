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

/// How a response settles, measured from the instant `from` (when the stimulus was applied).
#[derive(Clone, Debug, PartialEq)]
pub struct Settling {
    /// Value at `from`.
    pub initial: f64,
    /// Value it ends at (mean of the last twentieth of the run).
    pub final_value: f64,
    /// Highest excursion beyond the final value, in percent of the step (0 if it never passes it).
    pub overshoot_pct: f64,
    /// For every tolerance (a fraction of the step, e.g. 1e-3): the time after `from` from which the waveform stays within
    /// `tolerance × |step|` of the final value to the end of the run (None: it never does).
    pub times: Vec<(f64, Option<f64>)>,
}

/// Settling times of the waveform `v(t)` for several tolerances. Needs a step that has settled by the end of the run.
pub fn settling_times(t: &[f64], v: &[f64], from: f64, tolerances: &[f64]) -> Result<Settling, String> {
    if t.len() < 5 || t.len() != v.len() {
        return Err("点数が足りません".into());
    }
    let i0 = t.iter().position(|x| *x >= from).ok_or("開始の時刻が、解析の時間より後です")?;
    let (tt, vv) = (&t[i0..], &v[i0..]);
    if tt.len() < 5 {
        return Err("開始の時刻の後の点が足りません".into());
    }
    let tail_from = tt[tt.len() - 1] - 0.05 * (tt[tt.len() - 1] - tt[0]);
    let tail: Vec<f64> = (0..tt.len()).filter(|&i| tt[i] >= tail_from).map(|i| vv[i]).collect();
    let final_value = tail.iter().sum::<f64>() / tail.len() as f64;
    let initial = vv[0];
    let step = final_value - initial;
    if step.abs() < 1e-30 {
        return Err("最初と最後の値が同じです。ステップ応答ではない波形には使えません".into());
    }
    let (lo, hi) = tail.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(*x), b.max(*x)));
    let times = tolerances
        .iter()
        .map(|&tol| {
            let band = tol * step.abs();
            // the last sample outside the band: the waveform is inside from the next instant on
            let last_out = (0..tt.len()).rev().find(|&i| (vv[i] - final_value).abs() > band);
            let when = match last_out {
                None => Some(0.0),
                // outside at the very end: has not settled to this tolerance
                Some(i) if i + 1 >= tt.len() || hi - lo > 2.0 * band => None,
                Some(i) => {
                    let (a, b) = ((vv[i] - final_value).abs(), (vv[i + 1] - final_value).abs());
                    let k = if a > b { (a - band) / (a - b) } else { 1.0 };
                    Some((tt[i] + (tt[i + 1] - tt[i]) * k - tt[0]).max(0.0))
                }
            };
            (tol, when)
        })
        .collect();
    let beyond = if step > 0.0 { vv.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - final_value } else { final_value - vv.iter().cloned().fold(f64::INFINITY, f64::min) };
    Ok(Settling { initial, final_value, overshoot_pct: (beyond / step.abs() * 100.0).max(0.0), times })
}

/// What a supply does when its load changes at once (a current source stepping from one current to another): how far the voltage
/// moves away, where it settles, and how long it takes to get there.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadStep {
    /// The voltage before the change (mean of the first twentieth of the window).
    pub before: f64,
    /// The voltage it settles at after the change (mean of the last tenth of the window).
    pub after: f64,
    /// The value farthest from `before`, and when.
    pub extreme: f64,
    pub extreme_at: f64,
    /// When the voltage first moved.
    pub start: f64,
    /// Time from `start` until it stays within a tenth of its largest distance from `after` (None: not within the window).
    pub recovery: Option<f64>,
}

impl LoadStep {
    /// The largest dip (negative) or bump, from the value before.
    pub fn excursion(&self) -> f64 {
        self.extreme - self.before
    }
}

/// Reads a load change inside the window [w0, w1] of a waveform. The window should hold one change (zoom to it): the value at its
/// start is "before", at its end "after".
pub fn load_step(t: &[f64], v: &[f64], w0: f64, w1: f64) -> Result<LoadStep, String> {
    let idx: Vec<usize> = (0..t.len().min(v.len())).filter(|&i| t[i] >= w0 && t[i] <= w1).collect();
    if idx.len() < 10 {
        return Err("表示している範囲の点が少なすぎます (広げてください)".into());
    }
    let mean = |s: &[usize]| s.iter().map(|&i| v[i]).sum::<f64>() / s.len() as f64;
    let before = mean(&idx[..(idx.len() / 20).max(1)]);
    let after = mean(&idx[idx.len() - (idx.len() / 10).max(1)..]);
    let (&far, _) = idx.iter().map(|i| (i, (v[*i] - before).abs())).max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
    let size = (v[far] - before).abs();
    if size < 1e-6_f64.max(1e-4 * before.abs()) {
        return Err("この範囲では電圧がほとんど動いていません (負荷が変わる所を表示してください)".into());
    }
    // the first move: a twentieth of the largest one
    let start = idx.iter().find(|&&i| (v[i] - before).abs() > 0.05 * size).map_or(t[idx[0]], |&i| t[i]);
    let band = 0.1 * idx.iter().filter(|&&i| t[i] >= start).map(|&i| (v[i] - after).abs()).fold(0.0, f64::max);
    let last_out = idx.iter().rev().find(|&&i| (v[i] - after).abs() > band).map(|&i| t[i]);
    let end_in = idx.iter().rev().take((idx.len() / 20).max(2)).all(|&i| (v[i] - after).abs() <= band);
    let recovery = match last_out {
        Some(tl) if end_in => Some((tl - start).max(0.0)),
        None => Some(0.0),
        _ => None,
    };
    Ok(LoadStep { before, after, extreme: v[far], extreme_at: t[far], start, recovery })
}

#[cfg(test)]
mod load_step_tests {
    use super::*;

    #[test]
    fn a_dip_and_its_recovery_are_read() {
        // 3.3 V; at 1 ms it dips by 100 mV and comes back to 3.29 V with a 50 µs time constant
        let t: Vec<f64> = (0..3000).map(|i| i as f64 * 1e-6).collect();
        let v: Vec<f64> = t.iter().map(|&x| if x < 1e-3 { 3.3 } else { 3.29 - 0.09 * (-(x - 1e-3) / 50e-6).exp() }).collect();
        let s = load_step(&t, &v, 0.0, 3e-3).unwrap();
        assert!((s.before - 3.3).abs() < 1e-9 && (s.after - 3.29).abs() < 1e-6);
        assert!((s.excursion() + 0.1).abs() < 1e-3, "{}", s.excursion());
        assert!((s.start - 1e-3).abs() < 2e-6);
        // within a tenth of the 90 mV distance after ln(10) time constants: 115 µs
        let r = s.recovery.unwrap();
        assert!((r - 115e-6).abs() < 5e-6, "{r}");
        // nothing moves in the first millisecond
        assert!(load_step(&t, &v, 0.0, 0.9e-3).is_err());
    }
}
