//! Temperature and Monte Carlo.
use crate::*;

fn divider() -> Circuit {
    let mut c = Circuit::new();
    let (top, mid) = (c.node("top"), c.node("mid"));
    c.add(Element::vdc("V1", top, 0, 5.0));
    c.add(Element::Resistor { name: "R1".into(), a: top, b: mid, r: 10e3 });
    c.add(Element::Resistor { name: "R2".into(), a: mid, b: 0, r: 10e3 });
    c
}

fn led_circuit() -> Circuit {
    let mut c = Circuit::new();
    let (top, a) = (c.node("top"), c.node("a"));
    c.add(Element::vdc("V1", top, 0, 5.0));
    c.add(Element::Resistor { name: "R1".into(), a: top, b: a, r: 1e3 });
    c.add(Element::Diode { name: "D1".into(), a, k: 0, model: DiodeModel::default() });
    c
}

fn settings(runs: usize, tol: f64) -> McSettings {
    McSettings { runs, seed: 7, tol_r: tol, tol_c: tol, tol_l: tol, output: "mid".into() }
}

#[test]
fn a_silicon_diode_drops_about_two_millivolts_less_per_degree() {
    let c = led_circuit();
    let vf = |t: f64| dc_op(&c.at_temperature(t)).unwrap().voltage("a").unwrap();
    let slope = (vf(77.0) - vf(27.0)) / 50.0;
    assert!((-2.6e-3..-1.6e-3).contains(&slope), "{slope} V/°C");
    assert!(vf(27.0) > 0.55 && vf(27.0) < 0.85);
    assert_eq!(vf(27.0), dc_op(&c).unwrap().voltage("a").unwrap(), "27 °C is the nominal temperature");
}

#[test]
fn temperature_sweep_returns_one_point_per_step() {
    let r = temp_sweep(&led_circuit(), -20.0, 80.0, 10.0).unwrap();
    assert_eq!(r.x.len(), 11);
    assert_eq!((r.x[0], r.x[10]), (-20.0, 80.0));
    let v = &r.signal("V(a)").unwrap().values;
    assert!(v.windows(2).all(|w| w[1] < w[0]), "forward voltage falls as it gets warmer");
    assert!(temp_sweep(&led_circuit(), 50.0, 0.0, 10.0).is_err());
    assert!(temp_sweep(&led_circuit(), 0.0, 50.0, 0.0).is_err());
}

#[test]
fn resistors_ignore_temperature() {
    let c = divider();
    let v = |t: f64| dc_op(&c.at_temperature(t)).unwrap().voltage("mid").unwrap();
    assert!((v(-40.0) - v(85.0)).abs() < 1e-12);
}

#[test]
fn monte_carlo_stays_inside_the_tolerance_band() {
    let r = monte_carlo(&divider(), &settings(300, 0.05)).unwrap();
    assert!((r.nominal - 2.5).abs() < 1e-6);
    assert_eq!((r.values.len(), r.failed), (300, 0));
    let s = r.stats().unwrap();
    // R2 / (R1 + R2) with both within ±5 % can only be between 0.4756 and 0.5256
    assert!(s.min > 2.37 && s.max < 2.63, "{s:?}");
    assert!(s.min < 2.45 && s.max > 2.55, "the spread is really used: {s:?}");
    assert!((s.mean - 2.5).abs() < 0.03);
    assert!(s.std > 0.02 && s.std < 0.08, "{}", s.std);
}

#[test]
fn same_seed_same_result_and_sensitivity_points_at_the_parts() {
    let a = monte_carlo(&divider(), &settings(50, 0.05)).unwrap();
    let b = monte_carlo(&divider(), &settings(50, 0.05)).unwrap();
    assert_eq!(a.values, b.values);
    let other = monte_carlo(&divider(), &McSettings { seed: 8, ..settings(50, 0.05) }).unwrap();
    assert_ne!(a.values, other.values);
    let names: Vec<&str> = a.sensitivity.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names.len(), 2);
    let r1 = a.sensitivity.iter().find(|(n, _)| n == "R1").unwrap().1;
    let r2 = a.sensitivity.iter().find(|(n, _)| n == "R2").unwrap().1;
    assert!(r1 < 0.0 && r2 > 0.0, "a larger top resistor lowers the midpoint, a larger bottom one raises it");
}

#[test]
fn histogram_counts_every_value() {
    let r = monte_carlo(&divider(), &settings(200, 0.1)).unwrap();
    let (lo, hi, bins) = r.histogram(12);
    assert_eq!(bins.len(), 12);
    assert_eq!(bins.iter().sum::<usize>(), 200);
    assert!(lo < hi);
}

#[test]
fn bad_monte_carlo_settings_are_errors() {
    assert!(monte_carlo(&divider(), &McSettings { output: "nope".into(), ..settings(10, 0.05) }).is_err());
    assert!(monte_carlo(&divider(), &settings(10, 0.0)).is_err(), "nothing to vary");
    assert!(monte_carlo(&divider(), &settings(0, 0.05)).is_err());
    assert!(monte_carlo(&divider(), &settings(10, 1.5)).is_err());
}
