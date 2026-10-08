use crate::*;
use sch_core::{Kind, Schematic, Wire};

fn sample(res: &TranResult, name: &str, t: f64) -> f64 {
    let s = res.signal(name).unwrap_or_else(|| panic!("no signal {name}"));
    let i = res.time.partition_point(|&x| x < t).clamp(1, res.time.len() - 1);
    let (t0, t1) = (res.time[i - 1], res.time[i]);
    let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
    s.values[i - 1] + f * (s.values[i] - s.values[i - 1])
}

fn max_err(res: &TranResult, name: &str, f: impl Fn(f64) -> f64) -> f64 {
    let s = res.signal(name).unwrap();
    res.time.iter().zip(&s.values).map(|(&t, &v)| (v - f(t)).abs()).fold(0.0, f64::max)
}

fn src(c: &mut Circuit, name: &str, p: &str, n: &str, spec: &str) {
    let (p, n) = (c.node(p), c.node(n));
    c.add(Element::VSource { name: name.into(), p, n, wave: parse_source(spec).unwrap(), ac: None });
}

fn res(c: &mut Circuit, name: &str, a: &str, b: &str, ohms: f64) {
    let (a, b) = (c.node(a), c.node(b));
    c.add(Element::Resistor { name: name.into(), a, b, r: ohms });
}

fn cap(c: &mut Circuit, name: &str, a: &str, b: &str, farads: f64) {
    let (a, b) = (c.node(a), c.node(b));
    c.add(Element::Capacitor { name: name.into(), a, b, c: farads });
}

fn ind(c: &mut Circuit, name: &str, a: &str, b: &str, henries: f64) {
    let (a, b) = (c.node(a), c.node(b));
    c.add(Element::Inductor { name: name.into(), a, b, l: henries });
}

fn settings(tstop: f64, tstep: f64, uic: bool) -> TranSettings {
    TranSettings { tstop, tstep, uic }
}

#[test]
fn rc_charging_from_zero_state() {
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "5");
    res(&mut c, "R1", "in", "a", 1000.0);
    cap(&mut c, "C1", "a", "0", 1e-6);
    let r = transient(&c, &settings(5e-3, 20e-6, true)).unwrap();
    let tau = 1e-3;
    // first backward-Euler step costs ~1 mV, trapezoidal steps after that are far tighter
    assert!(max_err(&r, "V(a)", |t| 5.0 * (1.0 - (-t / tau).exp())) < 2e-3);
    // capacitor current i = (5 - v)/R. At t = 0 the zero-state start has no meaningful current
    // (it jumps to 5 mA instantly), so compare from the second point on.
    let i = r.signal("I(C1)").unwrap();
    let err = r.time.iter().zip(&i.values).skip(2).map(|(&t, &v)| (v - 5e-3 * (-t / tau).exp()).abs()).fold(0.0, f64::max);
    assert!(err < 5e-5, "{err}");
}

#[test]
fn rc_charging_with_a_pulse_from_the_dc_point() {
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "PULSE(0 5 0 1n 1n)");
    res(&mut c, "R1", "in", "a", 1000.0);
    cap(&mut c, "C1", "a", "0", 1e-6);
    let r = transient(&c, &settings(5e-3, 20e-6, false)).unwrap();
    assert!(max_err(&r, "V(a)", |t| 5.0 * (1.0 - (-t / 1e-3).exp())) < 2e-3);
    assert_eq!(r.signal("V(a)").unwrap().values[0], 0.0);
}

#[test]
fn dc_source_without_uic_stays_at_the_operating_point() {
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "5");
    res(&mut c, "R1", "in", "a", 1000.0);
    cap(&mut c, "C1", "a", "0", 1e-6);
    let r = transient(&c, &settings(2e-3, 50e-6, false)).unwrap();
    assert!(max_err(&r, "V(a)", |_| 5.0) < 1e-6);
}

#[test]
fn rl_current_rise() {
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "10");
    res(&mut c, "R1", "in", "a", 10.0);
    ind(&mut c, "L1", "a", "0", 10e-3);
    let r = transient(&c, &settings(5e-3, 20e-6, true)).unwrap();
    assert!(max_err(&r, "I(L1)", |t| 1.0 - (-t / 1e-3).exp()) < 2e-3);
}

#[test]
fn underdamped_series_rlc_step_response() {
    let (rr, l, cc) = (10.0, 1e-3, 1e-6);
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "5");
    res(&mut c, "R1", "in", "a", rr);
    ind(&mut c, "L1", "a", "b", l);
    cap(&mut c, "C1", "b", "0", cc);
    let r = transient(&c, &settings(1e-3, 1e-6, true)).unwrap();
    let alpha = rr / (2.0 * l);
    let w0 = (1.0 / (l * cc)).sqrt();
    let wd = (w0 * w0 - alpha * alpha).sqrt();
    let exact = |t: f64| 5.0 * (1.0 - (-alpha * t).exp() * ((wd * t).cos() + alpha / wd * (wd * t).sin()));
    assert!(max_err(&r, "V(b)", exact) < 0.05);
    // it really rings: overshoot well above the final value
    let peak = r.signal("V(b)").unwrap().values.iter().cloned().fold(0.0, f64::max);
    assert!(peak > 7.0 && peak < 8.5, "{peak}");
}

#[test]
fn lowpass_filter_steady_state_amplitude() {
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "SIN(0 1 1k)");
    res(&mut c, "R1", "in", "out", 1000.0);
    cap(&mut c, "C1", "out", "0", 159.155e-9); // fc = 1 kHz
    let r = transient(&c, &settings(10e-3, 10e-6, false)).unwrap();
    let s = r.signal("V(out)").unwrap();
    let peak = r.time.iter().zip(&s.values).filter(|(t, _)| **t > 9e-3).map(|(_, v)| *v).fold(f64::MIN, f64::max);
    assert!((peak - 0.7071).abs() < 0.01, "{peak}");
    // the source itself is reproduced exactly
    assert!(max_err(&r, "V(in)", |t| (2.0 * std::f64::consts::PI * 1e3 * t).sin()) < 1e-6);
}

#[test]
fn half_wave_rectifier() {
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "SIN(0 5 1k)");
    let (a, o) = (c.node("in"), c.node("out"));
    c.add(Element::Diode { name: "D1".into(), a, k: o, model: DiodeModel::default() });
    res(&mut c, "RL", "out", "0", 1000.0);
    let r = transient(&c, &settings(3e-3, 5e-6, false)).unwrap();
    let s = &r.signal("V(out)").unwrap().values;
    let (max, min) = (s.iter().cloned().fold(f64::MIN, f64::max), s.iter().cloned().fold(f64::MAX, f64::min));
    assert!(max > 4.2 && max < 4.5, "peak {max}");
    assert!(min > -1e-3, "negative half must be blocked, min {min}");
}

#[test]
fn integrator_lands_on_source_corners() {
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "PULSE(0 5 1m 1u 1u 2m 5m)");
    res(&mut c, "R1", "in", "0", 1000.0);
    let r = transient(&c, &settings(8e-3, 0.5e-3, false)).unwrap();
    for t in [1e-3, 1.001e-3, 3.001e-3, 3.002e-3, 6e-3, 6.001e-3] {
        assert!(r.time.iter().any(|x| (x - t).abs() < 1e-12), "no sample at {t}");
    }
    assert!((sample(&r, "V(in)", 1.0005e-3) - 2.5).abs() < 1e-6);
    assert!((sample(&r, "V(in)", 2e-3) - 5.0).abs() < 1e-9);
    assert!(sample(&r, "V(in)", 4e-3).abs() < 1e-9);
    // monotonic time
    assert!(r.time.windows(2).all(|w| w[1] > w[0]));
    assert!((r.time.last().unwrap() - 8e-3).abs() < 1e-12);
}

#[test]
fn invalid_settings_are_rejected() {
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "5");
    res(&mut c, "R1", "in", "0", 1000.0);
    for s in [settings(0.0, 1e-6, false), settings(1e-3, 0.0, false), settings(1.0, 1e-9, false), settings(f64::NAN, 1e-6, false)] {
        assert!(matches!(transient(&c, &s), Err(SimError::Invalid(_))), "{s:?}");
    }
}

fn wire(s: &mut Schematic, a: (i32, i32), b: (i32, i32)) {
    s.wires.push(Wire { a, b });
}

#[test]
fn schematic_rc_with_pulse_source() {
    let mut s = Schematic::default();
    s.add(Kind::VSource, (0, 0), 0);
    s.components[0].value = "PULSE(0 5 0 1n 1n)".into();
    s.add(Kind::Resistor, (6, -6), 0);
    s.components[1].value = "1k".into();
    s.add(Kind::Capacitor, (12, 0), 1);
    s.components[2].value = "1u".into();
    s.add(Kind::Ground, (0, 6), 0);
    for (a, b) in [((0, -2), (0, -6)), ((0, -6), (4, -6)), ((8, -6), (12, -6)), ((12, -6), (12, -2)), ((12, 2), (12, 6)), ((12, 6), (0, 6)), ((0, 2), (0, 6))] {
        wire(&mut s, a, b);
    }
    let c = from_schematic(&s).unwrap();
    let r = transient(&c, &settings(4e-3, 10e-6, false)).unwrap();
    assert!((sample(&r, "I(R1)", 1e-3) - 5e-3 * (-1.0f64).exp()).abs() < 5e-5);
    assert!((sample(&r, "I(R1)", 3e-3) - 5e-3 * (-3.0f64).exp()).abs() < 5e-5);

    // a malformed waveform is reported against the part
    s.components[0].value = "SIN(0 1)".into();
    let errs = from_schematic(&s).unwrap_err();
    assert_eq!(errs[0].refdes, "V1");
}
