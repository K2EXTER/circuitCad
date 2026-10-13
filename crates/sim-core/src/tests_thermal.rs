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
    let gd = id / crate::dc::VT;
    // shot current 2qI across the diode, in parallel with R1 (which also has its own thermal noise)
    let z = 1.0 / (gd + 1e-3);
    let want = (2.0 * 1.602176634e-19 * id + 4.0 * KB * 300.0 * 1e-3) * z * z;
    assert!((r.out_psd[0] / want - 1.0).abs() < 1e-3, "{} vs {want}", r.out_psd[0]);
    assert!(noise_analysis(&c, &settings(1.0, 100.0, "nope", None)).is_err());
    assert!(noise_analysis(&c, &settings(1.0, 100.0, "a", Some("nope"))).is_err());
}

fn bjt_amp(rb: f64) -> Circuit {
    // common-emitter stage with the base driven through a source resistor
    let mut c = Circuit::new();
    vsrc(&mut c, "VCC", "vcc", 10.0);
    vsrc(&mut c, "VB", "vb", 1.7);
    res(&mut c, "RB", "vb", "b", 100e3);
    res(&mut c, "RC", "vcc", "c", 4.7e3);
    res(&mut c, "RE", "e", "0", 1e3);
    let (cc, b, e) = (c.node("c"), c.node("b"), c.node("e"));
    c.add(Element::Bjt { name: "Q1".into(), c: cc, b, e, model: BjtModel { rb, ..BjtModel::default() } });
    c
}

#[test]
fn base_resistance_adds_thermal_noise_and_keeps_dc() {
    let (c0, c1) = (bjt_amp(0.0), bjt_amp(200.0));
    // 200 ohm in series with a 100k base drive barely moves the operating point
    let (i0, i1) = (dc_op(&c0).unwrap().element("Q1").unwrap().current, dc_op(&c1).unwrap().element("Q1").unwrap().current);
    assert!(i0 > 1e-4 && (i0 / i1 - 1.0).abs() < 1e-2, "{i0} {i1}");
    let s = settings(10.0, 1e3, "c", None);
    let (n0, n1) = (noise_analysis(&c0, &s).unwrap(), noise_analysis(&c1, &s).unwrap());
    assert!(n0.contribution("Q1.rb").is_none());
    let rb = n1.contribution("Q1.rb").unwrap();
    assert!(rb.rms > 0.0 && n1.out_psd[0] > n0.out_psd[0] * (1.0 - 1e-2));
    // with a low-impedance base drive rb dominates the base-side noise
    let mut low = bjt_amp(500.0);
    low.elements.retain(|e| e.name() != "RB");
    let (vb, b) = (low.node("vb"), low.node("b"));
    low.add(Element::Resistor { name: "RB".into(), a: vb, b, r: 1.0 });
    let r = noise_analysis(&low, &s).unwrap();
    assert!(r.contribution("Q1.rb").unwrap().rms > r.contribution("RB").unwrap().rms * 10.0);
}

#[test]
fn bjt_noise_example_runs() {
    let sch: sch_core::Schematic = serde_json::from_str(include_str!("../../../docs/examples/bjt_noise.schjson")).unwrap();
    let c = from_schematic(&sch).unwrap();
    let r = noise_analysis(&c, &settings(10.0, 1e5, "C", Some("V2"))).unwrap();
    // base shot noise dominates; rb is present but small
    assert_eq!(r.contributions[0].name, "Q1.ib");
    assert!(r.contribution("Q1.rb").unwrap().rms > 0.0);
    assert!(r.in_rms.unwrap() > 0.0);
}

#[test]
fn sources_are_described_and_ranked() {
    assert_eq!(describe_source("R1").kind, NoiseKind::Thermal);
    assert_eq!(describe_source("Q1.ic").kind, NoiseKind::Shot);
    assert_eq!(describe_source("Q1.rb").kind, NoiseKind::Thermal);
    assert_eq!(describe_source("D3").kind, NoiseKind::Shot);
    assert_eq!(describe_source("U1.R2").kind, NoiseKind::Thermal);
    let mut c = Circuit::new();
    vsrc(&mut c, "V1", "in", 5.0);
    res(&mut c, "R1", "in", "out", 1e3);
    res(&mut c, "R2", "out", "0", 9e3);
    let r = noise_analysis(&c, &settings(1.0, 1e3, "out", None)).unwrap();
    let (top, share) = r.dominant().unwrap();
    // R1 (1k) drives the output through the 9k/10k divider: 810 vs 90 in units of 4kT -> 90 %
    let sum: f64 = r.contributions.iter().map(|x| r.share(&x.name)).sum();
    assert!((sum - 1.0).abs() < 1e-12);
    assert!(top.name == "R1" && (share - 0.9).abs() < 1e-9, "{} {share}", top.name);
}
