//! Signal statistics and amplitude spectrum of a (non-uniformly sampled) transient waveform.

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub mean: f64,
    /// RMS of the signal with its mean removed (the "noise" or ripple amplitude).
    pub rms_ac: f64,
    pub min: f64,
    pub max: f64,
}

/// Value of the piecewise-linear signal at `t`.
fn value_at(t: &[f64], v: &[f64], x: f64) -> f64 {
    if x <= t[0] {
        return v[0];
    }
    if x >= t[t.len() - 1] {
        return v[v.len() - 1];
    }
    let i = t.partition_point(|&a| a < x).clamp(1, t.len() - 1);
    let (t0, t1) = (t[i - 1], t[i]);
    if t1 > t0 { v[i - 1] + (v[i] - v[i - 1]) * (x - t0) / (t1 - t0) } else { v[i] }
}

/// Time-weighted statistics over `[t0, t1]`, exact for the piecewise-linear signal.
pub fn stats(t: &[f64], v: &[f64], t0: f64, t1: f64) -> Option<Stats> {
    if t.len() < 2 || t.len() != v.len() || !(t1 > t0) {
        return None;
    }
    let (t0, t1) = (t0.max(t[0]), t1.min(t[t.len() - 1]));
    if !(t1 > t0) {
        return None;
    }
    // breakpoints inside the window
    let a = t.partition_point(|&x| x <= t0);
    let b = t.partition_point(|&x| x < t1);
    let mut ts = vec![t0];
    ts.extend_from_slice(&t[a..b.max(a)]);
    ts.push(t1);
    let vs: Vec<f64> = ts.iter().map(|&x| value_at(t, v, x)).collect();

    let (mut int, mut min, mut max) = (0.0, f64::MAX, f64::MIN);
    for k in 0..ts.len() - 1 {
        int += 0.5 * (vs[k] + vs[k + 1]) * (ts[k + 1] - ts[k]);
    }
    for &x in &vs {
        min = min.min(x);
        max = max.max(x);
    }
    let mean = int / (t1 - t0);
    // integral of (v - mean)^2 over a linear segment from p to q: (p^2 + p q + q^2)/3 * dt
    let mut sq = 0.0;
    for k in 0..ts.len() - 1 {
        let (p, q) = (vs[k] - mean, vs[k + 1] - mean);
        sq += (p * p + p * q + q * q) / 3.0 * (ts[k + 1] - ts[k]);
    }
    Some(Stats { mean, rms_ac: (sq / (t1 - t0)).sqrt(), min, max })
}

#[derive(Clone, Debug)]
pub struct Spectrum {
    pub freq: Vec<f64>,
    /// Peak amplitude of each frequency component (same unit as the signal), Hann-windowed.
    pub amp: Vec<f64>,
    pub stats: Stats,
}

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (xr, xi) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len <<= 1;
    }
}

/// Amplitude spectrum of the window `[t0, t1]`: resampled uniformly (power-of-two points),
/// Hann window, coherent-gain corrected so that a sine of amplitude A shows A.
/// The DC bin is left out (see `stats().mean`).
pub fn spectrum(t: &[f64], v: &[f64], t0: f64, t1: f64) -> Option<Spectrum> {
    let st = stats(t, v, t0, t1)?;
    let (t0, t1) = (t0.max(t[0]), t1.min(t[t.len() - 1]));
    let in_window = t.partition_point(|&x| x <= t1) - t.partition_point(|&x| x < t0);
    let n = in_window.next_power_of_two().clamp(256, 1 << 18);
    let dt = (t1 - t0) / n as f64;
    let mut re: Vec<f64> = (0..n).map(|k| value_at(t, v, t0 + k as f64 * dt) - st.mean).collect();
    let win: Vec<f64> = (0..n).map(|k| 0.5 - 0.5 * (2.0 * PI * k as f64 / n as f64).cos()).collect();
    let gain: f64 = win.iter().sum();
    for (x, w) in re.iter_mut().zip(&win) {
        *x *= w;
    }
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    let df = 1.0 / (n as f64 * dt);
    let (mut freq, mut amp) = (vec![], vec![]);
    for k in 1..n / 2 {
        freq.push(k as f64 * df);
        amp.push(2.0 * re[k].hypot(im[k]) / gain);
    }
    Some(Spectrum { freq, amp, stats: st })
}

/// One harmonic of a periodic signal.
#[derive(Clone, Copy, Debug)]
pub struct Harmonic {
    /// 1 = the fundamental, 2 = twice its frequency ...
    pub order: usize,
    pub freq: f64,
    /// Peak amplitude (same unit as the signal).
    pub amp: f64,
}

/// Total harmonic distortion of a periodic signal.
#[derive(Clone, Debug)]
pub struct Thd {
    pub f0: f64,
    /// Whole periods of the window that was used (the window is cut to a whole number of periods, which makes the result exact).
    pub periods: usize,
    pub t0: f64,
    pub t1: f64,
    pub dc: f64,
    /// The fundamental first, then the harmonics 2..=max.
    pub harmonics: Vec<Harmonic>,
    /// sqrt(sum of the harmonics squared) / fundamental, as a ratio (0.01 = 1 %).
    pub thd: f64,
    /// Everything but the fundamental (harmonics, noise, other tones) over the fundamental: THD + N.
    pub thd_n: f64,
    /// Points of the waveform per period: with few, the straight lines between them are the distortion that shows.
    pub points_per_period: f64,
}

impl Thd {
    pub fn fundamental(&self) -> f64 {
        self.harmonics.first().map_or(0.0, |h| h.amp)
    }
}

/// The frequency of a repeating signal from its rising crossings of the mean in `[t0, t1]`
/// (first to last crossing, so a part of a period at either end does not matter).
pub fn estimate_frequency(t: &[f64], v: &[f64], t0: f64, t1: f64) -> Option<f64> {
    let st = stats(t, v, t0, t1)?;
    let (a, b) = (t0.max(t[0]), t1.min(t[t.len() - 1]));
    let mut cross: Vec<f64> = vec![];
    // a little hysteresis so that noise around the mean does not count
    let band = (st.max - st.min) * 0.02;
    let mut armed = false;
    for k in 1..t.len() {
        if t[k] < a || t[k - 1] > b {
            continue;
        }
        if v[k] < st.mean - band {
            armed = true;
        }
        if armed && v[k - 1] < st.mean && v[k] >= st.mean && v[k] > v[k - 1] {
            cross.push(t[k - 1] + (st.mean - v[k - 1]) / (v[k] - v[k - 1]) * (t[k] - t[k - 1]));
            armed = false;
        }
    }
    if cross.len() < 3 {
        return None;
    }
    Some((cross.len() - 1) as f64 / (cross[cross.len() - 1] - cross[0]))
}

/// Harmonics of the signal at `f0` (found from the signal when `None`) over the window `[t0, t1]`, up to `max_harmonic`.
/// The window is cut to a whole number of periods and the signal is resampled with 2048 points per period; each harmonic is
/// the Fourier sum at exactly k * f0, so a sine of amplitude A reads A with nothing leaking into its neighbours.
pub fn thd(t: &[f64], v: &[f64], f0: Option<f64>, t0: f64, t1: f64, max_harmonic: usize) -> Result<Thd, String> {
    if t.len() < 2 || t.len() != v.len() {
        return Err("波形がありません".into());
    }
    let (a, b) = (t0.max(t[0]), t1.min(t[t.len() - 1]));
    if !(b > a) {
        return Err("解析範囲に波形がありません".into());
    }
    let f0 = match f0 {
        Some(f) if f > 0.0 => f,
        Some(_) => return Err("基本周波数は正の値にしてください".into()),
        None => estimate_frequency(t, v, a, b).ok_or("基本周波数が見つかりません。範囲に 3 周期以上入るようにするか、基本周波数を入力してください")?,
    };
    let periods = ((b - a) * f0 + 1e-6).floor() as usize;
    if periods < 1 {
        return Err(format!("解析範囲が 1 周期 ({:.4e} 秒) より短いです。範囲を広げてください", 1.0 / f0));
    }
    let max_harmonic = max_harmonic.clamp(2, 100);
    const PER: usize = 2048;
    if max_harmonic >= PER / 8 {
        return Err("高調波の数が多すぎます".into());
    }
    let n = periods * PER;
    let span = periods as f64 / f0;
    let x: Vec<f64> = (0..n).map(|k| value_at(t, v, a + span * k as f64 / n as f64)).collect();
    let dc = x.iter().sum::<f64>() / n as f64;
    let rms2 = x.iter().map(|y| (y - dc).powi(2)).sum::<f64>() / n as f64;
    let mut harmonics = vec![];
    for k in 1..=max_harmonic {
        // angle of sample j is 2 pi k j / PER (the same in every period), so the sums repeat: a table of PER entries
        let (mut re, mut im) = (0.0, 0.0);
        for (j, y) in x.iter().enumerate() {
            let ang = 2.0 * PI * ((k * (j % PER)) % PER) as f64 / PER as f64;
            re += (y - dc) * ang.cos();
            im += (y - dc) * ang.sin();
        }
        harmonics.push(Harmonic { order: k, freq: k as f64 * f0, amp: 2.0 * re.hypot(im) / n as f64 });
    }
    let a1 = harmonics[0].amp;
    if a1 <= 1e-15 {
        return Err("基本周波数の成分がありません。基本周波数を確かめてください".into());
    }
    let h2: f64 = harmonics.iter().skip(1).map(|h| h.amp * h.amp).sum();
    let rest = (rms2 - a1 * a1 / 2.0).max(0.0);
    let in_window = t.partition_point(|&q| q <= b) - t.partition_point(|&q| q < a);
    Ok(Thd {
        f0,
        periods,
        t0: a,
        t1: a + span,
        dc,
        thd: h2.sqrt() / a1,
        thd_n: (rest / (a1 * a1 / 2.0)).sqrt(),
        harmonics,
        points_per_period: in_window as f64 / ((b - a) * f0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(f: f64, a: f64, offset: f64, t_end: f64, step: f64) -> (Vec<f64>, Vec<f64>) {
        let n = (t_end / step).round() as usize;
        let t: Vec<f64> = (0..=n).map(|k| k as f64 * step).collect();
        let v = t.iter().map(|x| offset + a * (2.0 * PI * f * x).sin()).collect();
        (t, v)
    }

    #[test]
    fn stats_of_a_sine_with_offset() {
        let (t, v) = sine(1000.0, 2.0, 5.0, 10e-3, 1e-6);
        let s = stats(&t, &v, 0.0, 10e-3).unwrap();
        assert!((s.mean - 5.0).abs() < 1e-6);
        assert!((s.rms_ac - 2.0 / 2f64.sqrt()).abs() < 1e-4, "{}", s.rms_ac);
        assert!((s.max - 7.0).abs() < 1e-3 && (s.min - 3.0).abs() < 1e-3);
    }

    #[test]
    fn stats_are_time_weighted_for_uneven_steps() {
        // 0 for 9 ms (one sample pair), then 10 for 1 ms: mean must be 1, not 5
        let t = [0.0, 9e-3, 9e-3 + 1e-9, 10e-3];
        let v = [0.0, 0.0, 10.0, 10.0];
        let s = stats(&t, &v, 0.0, 10e-3).unwrap();
        assert!((s.mean - 1.0).abs() < 1e-5, "{}", s.mean);
        // windowing
        let w = stats(&t, &v, 9.5e-3, 10e-3).unwrap();
        assert!((w.mean - 10.0).abs() < 1e-9 && w.rms_ac < 1e-9);
        assert!(stats(&t, &v, 1.0, 2.0).is_none());
    }

    #[test]
    fn spectrum_peak_has_the_sine_amplitude() {
        let (t, v) = sine(1000.0, 1.5, 3.0, 64e-3, 1e-6);
        let sp = spectrum(&t, &v, 0.0, 64e-3).unwrap();
        let (k, peak) = sp.amp.iter().cloned().enumerate().fold((0, 0.0), |a, (i, x)| if x > a.1 { (i, x) } else { a });
        assert!((sp.freq[k] - 1000.0).abs() < 20.0, "{}", sp.freq[k]);
        assert!((peak - 1.5).abs() < 0.03, "{peak}");
        // far from the tone the spectrum is empty (Hann leakage is tiny)
        let far = sp.freq.iter().zip(&sp.amp).filter(|(f, _)| **f > 5000.0).map(|(_, a)| *a).fold(0.0, f64::max);
        assert!(far < 1e-3, "{far}");
        assert!((sp.stats.mean - 3.0).abs() < 1e-6);
    }

    #[test]
    fn two_tones_are_separated() {
        let (t, a) = sine(2000.0, 1.0, 0.0, 32e-3, 1e-6);
        let (_, b) = sine(10000.0, 0.25, 0.0, 32e-3, 1e-6);
        let v: Vec<f64> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
        let sp = spectrum(&t, &v, 0.0, 32e-3).unwrap();
        let near = |f: f64| sp.freq.iter().zip(&sp.amp).filter(|(x, _)| (**x - f).abs() < 100.0).map(|(_, a)| *a).fold(0.0, f64::max);
        assert!((near(2000.0) - 1.0).abs() < 0.03 && (near(10000.0) - 0.25).abs() < 0.01);
    }

    #[test]
    fn a_sine_with_a_known_third_harmonic_reads_back() {
        // 1 V at 1 kHz plus 0.01 V at 3 kHz: THD 1 %
        let n = 20_000;
        let t: Vec<f64> = (0..=n).map(|k| k as f64 * 1e-6).collect();
        let v: Vec<f64> = t.iter().map(|x| 2.0 + (2.0 * PI * 1e3 * x).sin() + 0.01 * (2.0 * PI * 3e3 * x).sin()).collect();
        let r = thd(&t, &v, None, 0.0, 20e-3, 10).unwrap();
        assert!((r.f0 - 1000.0).abs() < 0.5, "{}", r.f0);
        assert!((r.fundamental() - 1.0).abs() < 1e-3 && (r.dc - 2.0).abs() < 1e-3);
        assert!((r.harmonics[2].amp - 0.01).abs() < 1e-4, "{}", r.harmonics[2].amp);
        assert!((r.thd - 0.01).abs() < 2e-4, "{}", r.thd);
        // nothing but the harmonic is left over, so THD + N is the same
        assert!((r.thd_n - r.thd).abs() < 2e-4, "{} {}", r.thd_n, r.thd);
    }

    #[test]
    fn a_clipped_sine_has_odd_harmonics_and_a_pure_one_has_almost_none() {
        let n = 20_000;
        let t: Vec<f64> = (0..=n).map(|k| k as f64 * 1e-6).collect();
        let pure: Vec<f64> = t.iter().map(|x| (2.0 * PI * 1e3 * x).sin()).collect();
        let clipped: Vec<f64> = pure.iter().map(|y| (y * 1.5).clamp(-1.0, 1.0)).collect();
        let p = thd(&t, &pure, Some(1000.0), 0.0, 20e-3, 10).unwrap();
        assert!(p.thd < 1e-4, "{}", p.thd);
        let c = thd(&t, &clipped, Some(1000.0), 0.0, 20e-3, 10).unwrap();
        assert!(c.thd > 0.05, "{}", c.thd);
        // a symmetric clip has no even harmonics
        assert!(c.harmonics[1].amp < 1e-3 * c.fundamental() && c.harmonics[2].amp > 0.03, "{:?}", c.harmonics);
    }

    #[test]
    fn a_window_shorter_than_a_period_is_refused() {
        let t: Vec<f64> = (0..=1000).map(|k| k as f64 * 1e-6).collect();
        let v: Vec<f64> = t.iter().map(|x| (2.0 * PI * 1e3 * x).sin()).collect();
        assert!(thd(&t, &v, Some(1000.0), 0.0, 0.5e-3, 5).is_err());
        assert!(thd(&t, &v, Some(-1.0), 0.0, 1e-3, 5).is_err());
    }
}
