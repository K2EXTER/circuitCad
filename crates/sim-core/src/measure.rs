//! Automatic measurements on a transient waveform: peak-to-peak, frequency, duty cycle.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaveMeasure {
    pub min: f64,
    pub max: f64,
    /// Peak-to-peak.
    pub pp: f64,
    /// Repetition frequency and duty cycle (fraction of the period above the middle level);
    /// `None` when fewer than two full cycles are in the window.
    pub freq: Option<f64>,
    pub duty: Option<f64>,
}

/// Measures `v(t)` over `[t0, t1]`. Crossings of the middle level count only after the signal
/// has moved 10 % of its swing past it, so noise on a flat part is not read as a frequency.
pub fn wave_measure(t: &[f64], v: &[f64], t0: f64, t1: f64) -> Option<WaveMeasure> {
    if t.len() < 2 || t.len() != v.len() || !(t1 > t0) {
        return None;
    }
    let idx: Vec<usize> = (0..t.len()).filter(|&i| t[i] >= t0 && t[i] <= t1).collect();
    if idx.len() < 2 {
        return None;
    }
    let (mut min, mut max) = (f64::MAX, f64::MIN);
    for &i in &idx {
        min = min.min(v[i]);
        max = max.max(v[i]);
    }
    let pp = max - min;
    let mut m = WaveMeasure { min, max, pp, freq: None, duty: None };
    if pp < 1e-12 {
        return Some(m);
    }
    let (mid, h) = (min + pp / 2.0, pp * 0.1);
    let cross = |a: usize, b: usize| {
        let f = (mid - v[a]) / (v[b] - v[a]);
        t[a] + f * (t[b] - t[a])
    };
    let (mut rises, mut falls) = (vec![], vec![]);
    let mut high = v[idx[0]] > mid + h;
    let mut seeded = high || v[idx[0]] < mid - h;
    let mut last_opposite = idx[0];
    for w in idx.windows(2) {
        let (p, i) = (w[0], w[1]);
        if (v[i] > mid) != (v[p] > mid) {
            last_opposite = p;
        }
        if !seeded {
            if v[i] > mid + h || v[i] < mid - h {
                high = v[i] > mid;
                seeded = true;
            }
            continue;
        }
        if !high && v[i] > mid + h {
            high = true;
            rises.push(cross(last_opposite, last_opposite + 1));
        } else if high && v[i] < mid - h {
            high = false;
            falls.push(cross(last_opposite, last_opposite + 1));
        }
    }
    if rises.len() >= 2 {
        let period = (rises[rises.len() - 1] - rises[0]) / (rises.len() - 1) as f64;
        if period > 0.0 {
            m.freq = Some(1.0 / period);
        }
        let mut duties = vec![];
        for r in rises.windows(2) {
            if let Some(f) = falls.iter().find(|f| **f > r[0] && **f < r[1]) {
                duties.push((f - r[0]) / (r[1] - r[0]));
            }
        }
        if !duties.is_empty() {
            m.duty = Some(duties.iter().sum::<f64>() / duties.len() as f64);
        }
    }
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(freq: f64, duty: f64, n: usize, dt: f64) -> (Vec<f64>, Vec<f64>) {
        let t: Vec<f64> = (0..n).map(|i| i as f64 * dt).collect();
        let v = t.iter().map(|x| if (x * freq).fract() < duty { 5.0 } else { 0.0 }).collect();
        (t, v)
    }

    #[test]
    fn square_wave_frequency_and_duty() {
        let (t, v) = square(1000.0, 0.25, 10_000, 1e-6);
        let m = wave_measure(&t, &v, 0.0, 0.01).unwrap();
        assert_eq!((m.min, m.max, m.pp), (0.0, 5.0, 5.0));
        assert!((m.freq.unwrap() - 1000.0).abs() < 5.0, "{:?}", m.freq);
        assert!((m.duty.unwrap() - 0.25).abs() < 0.01, "{:?}", m.duty);
    }

    #[test]
    fn sine_frequency_and_flat_signal() {
        let t: Vec<f64> = (0..5000).map(|i| i as f64 * 1e-5).collect();
        let v: Vec<f64> = t.iter().map(|x| 2.0 * (2.0 * std::f64::consts::PI * 100.0 * x).sin()).collect();
        let m = wave_measure(&t, &v, 0.0, 0.05).unwrap();
        assert!((m.freq.unwrap() - 100.0).abs() < 1.0);
        assert!((m.duty.unwrap() - 0.5).abs() < 0.02);
        let flat = wave_measure(&t, &vec![3.0; t.len()], 0.0, 0.05).unwrap();
        assert_eq!((flat.pp, flat.freq), (0.0, None));
    }

    #[test]
    fn a_single_step_has_no_frequency() {
        let t: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let v: Vec<f64> = t.iter().map(|x| if *x < 50.0 { 0.0 } else { 1.0 }).collect();
        let m = wave_measure(&t, &v, 0.0, 99.0).unwrap();
        assert_eq!(m.freq, None);
    }
}
