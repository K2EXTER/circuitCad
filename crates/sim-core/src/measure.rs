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

/// Stability margins read off a loop-gain frequency response.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Margins {
    /// Frequency where the gain falls through 0 dB, and the phase margin there (degrees).
    pub gain_crossover: Option<f64>,
    pub phase_margin: Option<f64>,
    /// Frequency where the phase falls through -180 degrees, and the gain margin there (dB, positive = safe).
    pub phase_crossover: Option<f64>,
    pub gain_margin: Option<f64>,
    /// The response started near +-180 degrees (an inverting path): the phase was shifted by 180 so it starts near 0.
    pub inverted: bool,
}

/// Interpolates `y` at the point where `v` crosses `level` between samples `i` and `i+1`
/// (`f` in log frequency). Returns (frequency, y).
fn cross_at(freq: &[f64], v: &[f64], y: &[f64], i: usize, level: f64) -> (f64, f64) {
    let t = (level - v[i]) / (v[i + 1] - v[i]);
    let f = (freq[i].ln() + t * (freq[i + 1].ln() - freq[i].ln())).exp();
    (f, y[i] + t * (y[i + 1] - y[i]))
}

/// Gain margin and phase margin of a loop gain T(jw): `db` is |T| in dB and `phase_deg` its
/// unwrapped phase. The response is taken as the gain once around the (broken) feedback loop,
/// so stability needs the phase to stay above -180 degrees where the gain reaches 0 dB.
pub fn stability_margins(freq: &[f64], db: &[f64], phase_deg: &[f64]) -> Option<Margins> {
    let n = freq.len();
    if n < 2 || db.len() != n || phase_deg.len() != n || freq.iter().any(|f| *f <= 0.0) {
        return None;
    }
    let k = (phase_deg[0] / 180.0).round();
    let ph: Vec<f64> = phase_deg.iter().map(|p| p - 180.0 * k).collect();
    let mut m = Margins { gain_crossover: None, phase_margin: None, phase_crossover: None, gain_margin: None, inverted: k as i64 % 2 != 0 };
    if let Some(i) = (0..n - 1).find(|&i| db[i] >= 0.0 && db[i + 1] < 0.0) {
        let (f, p) = cross_at(freq, db, &ph, i, 0.0);
        m.gain_crossover = Some(f);
        m.phase_margin = Some(180.0 + p);
    }
    if let Some(i) = (0..n - 1).find(|&i| ph[i] >= -180.0 && ph[i + 1] < -180.0) {
        let (f, g) = cross_at(freq, &ph, db, i, -180.0);
        m.phase_crossover = Some(f);
        m.gain_margin = Some(-g);
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

    fn three_pole(k: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        // T(s) = k / (1 + s/w)^3 with the pole at 1 kHz
        let f: Vec<f64> = (0..400).map(|i| 10f64.powf(0.0 + i as f64 * 0.0125)).collect();
        let (mut db, mut ph) = (vec![], vec![]);
        for x in &f {
            let r = x / 1000.0;
            db.push(20.0 * (k / (1.0 + r * r).powf(1.5)).log10());
            ph.push(-3.0 * r.atan().to_degrees());
        }
        (f, db, ph)
    }

    #[test]
    fn margins_of_a_three_pole_loop() {
        // phase crosses -180 at tan = sqrt(3) -> f = 1.732 kHz, |T| there = k / 8
        let (f, db, ph) = three_pole(4.0);
        let m = stability_margins(&f, &db, &ph).unwrap();
        assert!((m.phase_crossover.unwrap() / 1732.0 - 1.0).abs() < 0.02, "{m:?}");
        assert!((m.gain_margin.unwrap() - 20.0 * (8.0f64 / 4.0).log10()).abs() < 0.2, "{m:?}");
        assert!(m.phase_margin.unwrap() > 0.0 && m.gain_margin.unwrap() > 0.0);
        // a higher gain makes the same loop unstable: both margins go negative
        let (f, db, ph) = three_pole(20.0);
        let m = stability_margins(&f, &db, &ph).unwrap();
        assert!(m.phase_margin.unwrap() < 0.0 && m.gain_margin.unwrap() < 0.0, "{m:?}");
    }

    #[test]
    fn an_inverted_response_is_shifted_and_a_weak_loop_has_no_crossover() {
        let (f, db, ph) = three_pole(4.0);
        let inv: Vec<f64> = ph.iter().map(|p| p + 180.0).collect();
        let m = stability_margins(&f, &db, &inv).unwrap();
        assert!(m.inverted);
        let straight = stability_margins(&f, &db, &ph).unwrap();
        assert!((m.phase_margin.unwrap() - straight.phase_margin.unwrap()).abs() < 1e-6);
        // gain always below 0 dB: nothing to report
        let low: Vec<f64> = db.iter().map(|d| d - 40.0).collect();
        let w = stability_margins(&f, &low, &ph).unwrap();
        assert_eq!((w.gain_crossover, w.phase_margin), (None, None));
    }
}
