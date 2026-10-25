//! Step-response figures, checked against closed-form first- and second-order responses.
use crate::*;

fn sample(f: impl Fn(f64) -> f64, tstop: f64, n: usize) -> (Vec<f64>, Vec<f64>) {
    let t: Vec<f64> = (0..n).map(|i| tstop * i as f64 / (n - 1) as f64).collect();
    let v = t.iter().map(|&x| f(x)).collect();
    (t, v)
}

#[test]
fn first_order_step_matches_the_textbook_numbers() {
    let tau = 1e-3;
    let (t, v) = sample(|x| 1.0 - (-x / tau).exp(), 20.0 * tau, 4001);
    let m = step_metrics(&t, &v).unwrap();
    assert!((m.final_value - 1.0).abs() < 1e-3);
    // 10-90 % rise time = ln(9) tau = 2.197 tau
    assert!((m.rise_time.unwrap() / tau - 2.197).abs() < 0.01, "{:?}", m);
    assert!(m.overshoot_pct < 0.01);
    // 2 % band is reached at 3.912 tau; measured from the 10 % point (0.105 tau)
    assert!((m.settling_time.unwrap() / tau - (3.912 - 0.105)).abs() < 0.02, "{:?}", m);
}

#[test]
fn an_underdamped_second_order_step_overshoots_as_predicted() {
    let (zeta, wn) = (0.5f64, 2.0 * std::f64::consts::PI * 100.0);
    let wd = wn * (1.0 - zeta * zeta).sqrt();
    let f = |x: f64| 1.0 - (-zeta * wn * x).exp() * ((wd * x).cos() + zeta * wn / wd * (wd * x).sin());
    let (t, v) = sample(f, 0.1, 8001);
    let m = step_metrics(&t, &v).unwrap();
    let want = 100.0 * (-std::f64::consts::PI * zeta / (1.0 - zeta * zeta).sqrt()).exp(); // 16.3 %
    assert!((m.overshoot_pct - want).abs() < 0.3, "{} vs {want}", m.overshoot_pct);
    assert!(m.settling_time.unwrap() > m.rise_time.unwrap());
}

#[test]
fn a_falling_step_is_measured_the_same_way() {
    let tau = 2e-3;
    let (t, v) = sample(|x| 5.0 * (-x / tau).exp(), 20.0 * tau, 4001);
    let m = step_metrics(&t, &v).unwrap();
    assert!((m.initial - 5.0).abs() < 1e-9 && m.final_value.abs() < 1e-3);
    assert!((m.rise_time.unwrap() / tau - 2.197).abs() < 0.02, "{:?}", m);
}

#[test]
fn waveforms_that_are_not_steps_are_refused_with_a_reason() {
    let (t, v) = sample(|x| (2.0 * std::f64::consts::PI * 50.0 * x).sin(), 0.1, 2001);
    assert!(step_metrics(&t, &v).is_err(), "a sine never settles");
    // a signal that ends where it began (a pulse) is not a step
    let (t, v) = sample(|x| if (0.02..0.04).contains(&x) { 1.0 } else { 0.0 }, 0.1, 2001);
    assert!(step_metrics(&t, &v).unwrap_err().contains("ステップ"));
    // still rising at the end of the run
    let (t, v) = sample(|x| 1.0 - (-x / 1e-2).exp(), 1e-2, 1001);
    assert!(step_metrics(&t, &v).unwrap_err().contains("落ち着いて"));
    assert!(step_metrics(&[0.0, 1.0], &[0.0, 1.0]).is_err());
}

fn rc_step() -> Circuit {
    let mut c = Circuit::new();
    let (inp, out) = (c.node("in"), c.node("out"));
    c.add(Element::VSource { name: "V1".into(), p: inp, n: 0, wave: Waveform::Pulse { v1: 0.0, v2: 5.0, td: 0.0, tr: 1e-6, tf: 1e-6, pw: 1.0, per: 2.0 }, ac: None });
    c.add(Element::Resistor { name: "R1".into(), a: inp, b: out, r: 1e3 });
    c.add(Element::Capacitor { name: "C1".into(), a: out, b: 0, c: 1e-6 });
    c
}

fn tran_measure(metric: TranMetric) -> McSettings {
    McSettings {
        runs: 30,
        seed: 3,
        tol_r: 0.05,
        tol_c: 0.1,
        tol_l: 0.1,
        output: "out".into(),
        measure: McMeasure::Tran { tstop: 10e-3, tstep: 50e-6, uic: true, metric },
    }
}

#[test]
fn monte_carlo_rise_time_of_an_rc_follows_r_times_c() {
    let r = monte_carlo(&rc_step(), &tran_measure(TranMetric::RiseTime)).unwrap();
    assert_eq!(r.unit, "s");
    // RC = 1 ms => 2.197 ms
    assert!((r.nominal / 2.197e-3 - 1.0).abs() < 0.03, "{}", r.nominal);
    let s = r.stats().unwrap();
    // R within 5 % and C within 10 %: the rise time moves by up to about 15 %
    assert!(s.min < r.nominal && s.max > r.nominal && s.max / s.min < 1.35, "{s:?}");
    assert!(r.sensitivity.iter().all(|(_, d)| *d > 0.0), "bigger R or C is slower: {:?}", r.sensitivity);
}

#[test]
fn monte_carlo_overshoot_units_and_non_step_errors() {
    let r = monte_carlo(&rc_step(), &tran_measure(TranMetric::Overshoot)).unwrap();
    assert_eq!(r.unit, "%");
    assert!(r.nominal < 0.5, "an RC step does not overshoot: {}", r.nominal);
    let too_short = McSettings { measure: McMeasure::Tran { tstop: 0.5e-3, tstep: 10e-6, uic: true, metric: TranMetric::RiseTime }, ..tran_measure(TranMetric::RiseTime) };
    match monte_carlo(&rc_step(), &too_short) {
        Err(SimError::Invalid(m)) => assert!(m.contains("落ち着いて"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn settling_times_of_a_first_order_response_are_the_logarithms_of_the_tolerances() {
    let tau = 1e-3;
    // the step is applied at 2 ms: before it the waveform is at rest
    let n = 20001;
    let t: Vec<f64> = (0..n).map(|i| 0.05 * i as f64 / (n - 1) as f64).collect();
    let v: Vec<f64> = t.iter().map(|&x| if x < 2e-3 { 0.0 } else { 1.0 - (-(x - 2e-3) / tau).exp() }).collect();
    let s = settling_times(&t, &v, 2e-3, &[1e-2, 1e-3, 3e-5]).unwrap();
    assert!((s.final_value - 1.0).abs() < 1e-6 && s.initial.abs() < 1e-9);
    for (tol, when) in &s.times {
        let want = -tol.ln() * tau;
        assert!((when.unwrap() - want).abs() < 0.02 * want, "tolerance {tol}: {when:?} vs {want}");
    }
    assert!(s.overshoot_pct < 1e-6);
    // a run that stops too early has not settled to the tight tolerance
    let short = settling_times(&t[..2500], &v[..2500], 2e-3, &[1e-6]).unwrap();
    assert_eq!(short.times[0].1, None);
}
