//! Average power and efficiency from transient waveforms.

/// Time-weighted mean of `v(t) * i(t)` over `[t0, t1]` (watts when v is volts and i is amps).
/// Both signals share the time axis `t`; the product is integrated with the trapezoid rule on the samples.
pub fn mean_power(t: &[f64], v: &[f64], i: &[f64], t0: f64, t1: f64) -> Option<f64> {
    if t.len() < 2 || v.len() != t.len() || i.len() != t.len() || !(t1 > t0) {
        return None;
    }
    let (mut sum, mut span) = (0.0, 0.0);
    for k in 1..t.len() {
        let (a, b) = (t[k - 1].max(t0), t[k].min(t1));
        if b <= a || t[k] <= t[k - 1] {
            continue;
        }
        // value of the sample pair at the clipped ends (linear in between)
        let f = |x: f64| (x - t[k - 1]) / (t[k] - t[k - 1]);
        let lerp = |s: &[f64], x: f64| s[k - 1] + (s[k] - s[k - 1]) * f(x);
        let (pa, pb) = (lerp(v, a) * lerp(i, a), lerp(v, b) * lerp(i, b));
        sum += 0.5 * (pa + pb) * (b - a);
        span += b - a;
    }
    (span > 0.0).then(|| sum / span)
}

/// Efficiency (0..) of output power over input power; `None` when no power goes in.
pub fn efficiency(p_in: f64, p_out: f64) -> Option<f64> {
    (p_in.abs() > 1e-15).then(|| p_out.abs() / p_in.abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dc_power_is_v_times_i_and_window_is_respected() {
        let t = [0.0, 1.0, 2.0, 3.0];
        let (v, i) = ([5.0; 4], [0.2, 0.2, 1.0, 1.0]);
        assert!((mean_power(&t, &v, &i, 0.0, 1.0).unwrap() - 1.0).abs() < 1e-12);
        assert!((mean_power(&t, &v, &i, 2.0, 3.0).unwrap() - 5.0).abs() < 1e-12);
        assert!(mean_power(&t, &v, &i, 5.0, 6.0).is_none());
    }

    #[test]
    fn ac_power_of_a_resistive_load_is_half_the_peak_product() {
        let n = 2001;
        let t: Vec<f64> = (0..n).map(|k| k as f64 * 1e-3 / 100.0).collect(); // 20 periods of 1 kHz... 20 ms
        let v: Vec<f64> = t.iter().map(|x| 10.0 * (2.0 * std::f64::consts::PI * 1e3 * x).sin()).collect();
        let i: Vec<f64> = v.iter().map(|x| x / 100.0).collect();
        let p = mean_power(&t, &v, &i, 0.0, *t.last().unwrap()).unwrap();
        assert!((p - 0.5).abs() < 0.01, "{p}"); // 10 V peak across 100 ohm: 0.5 W
    }

    #[test]
    fn efficiency_ignores_the_sign_of_a_source_current() {
        assert!((efficiency(-2.0, 1.5).unwrap() - 0.75).abs() < 1e-12);
        assert!(efficiency(0.0, 1.0).is_none());
    }
}
