use crate::*;
use std::f64::consts::PI;

const KB: f64 = 1.380649e-23;

fn settings(f0: f64, f1: f64, out: &str, input: Option<&str>) -> NoiseSettings {
    NoiseSettings {
        sweep: AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(50) },
        output: out.into(),
        output_ref: None,
        input: input.map(String::from),
        temp_c: 26.85,
    }
}

fn vsrc(c: &mut Circuit, name: &str, p: &str, v: f64) {
    let p = c.node(p);
    c.add(Element::vdc(name, p, 0, v));
}

fn res(c: &mut Circuit, name: &str, a: &str, b: &str, r: f64) {
    let (a, b) = (c.node(a), c.node(b));
    c.add(Element::Resistor { name: name.into(), a, b, r });
}

#[test]
fn single_resistor_gives_4ktr() {
    let mut c = Circuit::new();
    res(&mut c, "R1", "out", "0", 10e3);
    let r = noise_analysis(&c, &settings(10.0, 1e5, "out", None)).unwrap();
    let want = 4.0 * KB * 300.0 * 10e3;
    for p in &r.out_psd {
        assert!((p / want - 1.0).abs() < 1e-6, "{p} vs {want}");
    }
    // band-limited rms: sqrt(S * (f1 - f0))
    assert!((r.out_rms / (want * (1e5 - 10.0)).sqrt() - 1.0).abs() < 1e-3);
}

#[test]
fn divider_noise_is_the_parallel_resistance() {
    let mut c = Circuit::new();
    vsrc(&mut c, "V1", "in", 5.0);
    res(&mut c, "R1", "in", "out", 1e3);
    res(&mut c, "R2", "out", "0", 3e3);
    let r = noise_analysis(&c, &settings(1.0, 1e3, "out", Some("V1"))).unwrap();
    let rp = 750.0;
    let want = 4.0 * KB * 300.0 * rp;
    assert!((r.out_psd[0] / want - 1.0).abs() < 1e-6);
    // R1 and R2 share the output noise in proportion to R2/(R1+R2) squared etc.
    let (n1, n2) = (r.contribution("R1").unwrap().psd[0], r.contribution("R2").unwrap().psd[0]);
    assert!((n1 / (4.0 * KB * 300.0 * 1e3 * 0.75f64.powi(2)) - 1.0).abs() < 1e-6);
    assert!((n1 + n2 - r.out_psd[0]).abs() < 1e-30);
    // referred to the input the gain is R2/(R1+R2)
    assert!((r.gain.as_ref().unwrap()[0] - 0.75).abs() < 1e-9);
    assert!((r.in_psd.as_ref().unwrap()[0] - want / 0.5625).abs() < 1e-3 * want);
}

#[test]
fn rc_integrates_to_sqrt_kt_over_c() {
    let cap = 1e-9;
    let mut c = Circuit::new();
    res(&mut c, "R1", "out", "0", 100e3);
    let o = c.node("out");
    c.add(Element::Capacitor { name: "C1".into(), a: o, b: 0, c: cap });
    let r = noise_analysis(&c, &settings(1e-3, 1e9, "out", None)).unwrap();
    let want = (KB * 300.0 / cap).sqrt();
    assert!((r.out_rms / want - 1.0).abs() < 0.01, "{} vs {}", r.out_rms, want);
    // corner: density is 3 dB below the DC value
    let fc = 1.0 / (2.0 * PI * 100e3 * cap);
    let i = r.freq.iter().position(|f| *f >= fc).unwrap();
    let ratio = r.out_psd[i] / r.out_psd[0];
    assert!((0.4..0.6).contains(&ratio));
}

#[test]
fn temperature_scales_the_noise() {
    let mut c = Circuit::new();
    res(&mut c, "R1", "out", "0", 1e3);
    let mut s = settings(1.0, 10.0, "out", None);
    let a = noise_analysis(&c, &s).unwrap().out_psd[0];
    s.temp_c = 126.85;
    let b = noise_analysis(&c, &s).unwrap().out_psd[0];
    assert!((b / a - 400.0 / 300.0).abs() < 1e-9);
}

#[test]
fn diode_shot_noise_and_unknown_nodes() {
    let mut c = Circuit::new();
    vsrc(&mut c, "V1", "in", 5.0);
    res(&mut c, "R1", "in", "a", 1e3);
    let a = c.node("a");
    c.add(Element::Diode { name: "D1".into(), a, k: 0, model: DiodeModel::default() });
    let r = noise_analysis(&c, &settings(1.0, 100.0, "a", None)).unwrap();
    let op = dc_op(&c).unwrap();
    let id = op.element("D1").unwrap().current;
    let gd = id / 0.025852;
    // shot current 2qI across the diode, in parallel with R1 (which also has its own thermal noise)
    let z = 1.0 / (gd + 1e-3);
    let want = (2.0 * 1.602176634e-19 * id + 4.0 * KB * 300.0 * 1e-3) * z * z;
    assert!((r.out_psd[0] / want - 1.0).abs() < 1e-3, "{} vs {want}", r.out_psd[0]);
    assert!(noise_analysis(&c, &settings(1.0, 100.0, "nope", None)).is_err());
    assert!(noise_analysis(&c, &settings(1.0, 100.0, "a", Some("nope"))).is_err());
}
