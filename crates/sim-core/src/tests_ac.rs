use crate::*;
use sch_core::{extract, Kind, Schematic, Wire};
use std::f64::consts::PI;

const VT: f64 = 0.025852;

fn src(c: &mut Circuit, name: &str, p: &str, n: &str, spec: &str) {
    let (p, n) = (c.node(p), c.node(n));
    let s = parse_source_full(spec).unwrap();
    c.add(Element::VSource { name: name.into(), p, n, wave: s.wave, ac: s.ac });
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

fn sweep(f0: f64, f1: f64, ppd: usize) -> AcSettings {
    AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd) }
}

fn at(r: &AcResult, name: &str, f: f64) -> C64 {
    let i = r.freq.iter().position(|x| (x - f).abs() <= 1e-9 * f).unwrap_or_else(|| panic!("no point at {f}"));
    let s = r.signal(name).unwrap();
    C64::new(s.re[i], s.im[i])
}

#[test]
fn rc_lowpass_matches_the_transfer_function() {
    let (rr, cc) = (1000.0, 159.155e-9);
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "AC 1");
    res(&mut c, "R1", "in", "out", rr);
    cap(&mut c, "C1", "out", "0", cc);
    let r = ac_analysis(&c, &sweep(1.0, 1e6, 20)).unwrap();
    assert_eq!(r.freq.len(), 6 * 20 + 1);
    let out = r.signal("V(out)").unwrap();
    for (k, f) in r.freq.iter().enumerate() {
        let w = 2.0 * PI * f;
        let h = C64::new(1.0, 0.0) / C64::new(1.0, w * rr * cc);
        assert!((out.re[k] - h.re).abs() < 1e-9 && (out.im[k] - h.im).abs() < 1e-9, "f = {f}");
    }
    // corner frequency: -3.01 dB and -45 degrees
    let fc = 1.0 / (2.0 * PI * rr * cc);
    let h = {
        let w = 2.0 * PI * fc;
        C64::new(1.0, 0.0) / C64::new(1.0, w * rr * cc)
    };
    assert!((20.0 * h.abs().log10() + 3.0103).abs() < 1e-3);
    assert!((h.arg_deg() + 45.0).abs() < 1e-6);
    // helper accessors: dB falls, unwrapped phase goes towards -90
    let db = out.db();
    assert!(db[0].abs() < 1e-3 && *db.last().unwrap() < -50.0);
    let ph = out.phase_deg();
    assert!(*ph.last().unwrap() < -89.0 && *ph.last().unwrap() > -90.1);
    // resistor current i = (1 - H)/R, checked at the lowest and the highest frequency
    for k in [0, r.freq.len() - 1] {
        let w = 2.0 * PI * r.freq[k];
        let h = C64::new(1.0, 0.0) / C64::new(1.0, w * rr * cc);
        let want = (C64::new(1.0, 0.0) - h) * C64::new(1.0 / rr, 0.0);
        let got = at(&r, "I(R1)", r.freq[k]);
        assert!((got.re - want.re).abs() < 1e-9 && (got.im - want.im).abs() < 1e-9, "f = {}", r.freq[k]);
    }
}

#[test]
fn series_rlc_resonance() {
    let (rr, l, cc) = (10.0, 1e-3, 1e-6);
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "AC 1");
    res(&mut c, "R1", "in", "a", rr);
    ind(&mut c, "L1", "a", "b", l);
    cap(&mut c, "C1", "b", "0", cc);
    let r = ac_analysis(&c, &sweep(100.0, 100e3, 40)).unwrap();
    let out = r.signal("V(b)").unwrap();
    for (k, f) in r.freq.iter().enumerate() {
        let w = 2.0 * PI * f;
        // H = 1 / (1 - w^2 LC + j w R C)
        let h = C64::new(1.0, 0.0) / C64::new(1.0 - w * w * l * cc, w * rr * cc);
        assert!((out.re[k] - h.re).abs() < 1e-8 && (out.im[k] - h.im).abs() < 1e-8, "f = {f}");
    }
    // the peak sits near f0 and has the textbook height Q = 1/(w0 R C)
    let mags = out.mag();
    let (imax, peak) = mags.iter().cloned().enumerate().fold((0, 0.0), |a, (i, m)| if m > a.1 { (i, m) } else { a });
    let f0 = 1.0 / (2.0 * PI * (l * cc).sqrt());
    assert!((r.freq[imax] / f0 - 1.0).abs() < 0.07, "{} vs {f0}", r.freq[imax]);
    let q = 1.0 / ((1.0 / (l * cc).sqrt()) * rr * cc);
    assert!((peak - q).abs() / q < 0.02, "{peak} vs {q}");
}

#[test]
fn diode_is_linearised_at_its_operating_point() {
    // 5 V bias + 1 V AC through 1k into a diode: the AC divides between R and rd = n*Vt/Id
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "5 AC 1");
    res(&mut c, "R1", "in", "a", 1000.0);
    let a = c.node("a");
    c.add(Element::Diode { name: "D1".into(), a, k: 0, model: DiodeModel::default() });
    let dc = dc_op(&c).unwrap();
    let id = dc.element("D1").unwrap().current;
    let rd = VT / id;
    let r = ac_analysis(&c, &sweep(1e3, 1e3, 1)).unwrap();
    let got = at(&r, "V(a)", 1e3);
    let want = rd / (1000.0 + rd);
    assert!((got.re - want).abs() / want < 1e-4, "{} vs {want}", got.re);
    assert!(got.im.abs() < 1e-9);
}

#[test]
fn common_emitter_gain_is_minus_gm_rc() {
    let mut c = Circuit::new();
    src(&mut c, "V1", "vcc", "0", "10");
    res(&mut c, "Rb", "vcc", "b", 1e6);
    res(&mut c, "Rc", "vcc", "col", 1000.0);
    // AC source coupled into the base through a large capacitor
    src(&mut c, "Vin", "src", "0", "AC 1");
    cap(&mut c, "Cc", "src", "b", 1.0);
    let (cc, b) = (c.node("col"), c.node("b"));
    c.add(Element::Bjt { name: "Q1".into(), c: cc, b, e: 0, model: BjtModel { bf: 200.0, ..BjtModel::default() } });
    let dc = dc_op(&c).unwrap();
    let ic = dc.element("Q1").unwrap().current;
    let r = ac_analysis(&c, &sweep(1e3, 1e3, 1)).unwrap();
    let h = at(&r, "V(col)", 1e3);
    let gm_rc = ic / VT * 1000.0;
    assert!((h.abs() - gm_rc).abs() / gm_rc < 0.01, "{} vs {gm_rc}", h.abs());
    assert!((h.arg_deg().abs() - 180.0).abs() < 1.0, "inverting, got {}", h.arg_deg());
}

#[test]
fn ac_current_source_into_rc() {
    let mut c = Circuit::new();
    let a = c.node("a");
    c.add(Element::ISource { name: "I1".into(), from: 0, to: a, wave: Waveform::Dc(0.0), ac: Some((1e-3, 0.0)) });
    res(&mut c, "R1", "a", "0", 1000.0);
    cap(&mut c, "C1", "a", "0", 1e-6);
    let r = ac_analysis(&c, &sweep(10.0, 10e3, 10)).unwrap();
    let out = r.signal("V(a)").unwrap();
    for (k, f) in r.freq.iter().enumerate() {
        let w = 2.0 * PI * f;
        // Z = R / (1 + jwRC), V = I * Z
        let z = C64::new(1000.0, 0.0) / C64::new(1.0, w * 1000.0 * 1e-6);
        assert!((out.re[k] - 1e-3 * z.re).abs() < 1e-9 && (out.im[k] - 1e-3 * z.im).abs() < 1e-9);
    }
}

#[test]
fn linear_sweep_and_settings_validation() {
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "AC 1");
    res(&mut c, "R1", "in", "0", 1000.0);
    let r = ac_analysis(&c, &AcSettings { fstart: 100.0, fstop: 500.0, sweep: AcSweep::Linear(5) }).unwrap();
    assert_eq!(r.freq, vec![100.0, 200.0, 300.0, 400.0, 500.0]);
    for bad in [
        AcSettings { fstart: 0.0, fstop: 10.0, sweep: AcSweep::Decade(10) },
        AcSettings { fstart: 10.0, fstop: 1.0, sweep: AcSweep::Decade(10) },
        AcSettings { fstart: 1.0, fstop: 10.0, sweep: AcSweep::Decade(0) },
        AcSettings { fstart: 1.0, fstop: 10.0, sweep: AcSweep::Linear(1) },
        AcSettings { fstart: 1e-300, fstop: 1e300, sweep: AcSweep::Decade(1000) },
    ] {
        assert!(matches!(ac_analysis(&c, &bad), Err(SimError::Invalid(_))), "{bad:?}");
    }
}

#[test]
fn a_circuit_without_an_ac_source_is_rejected_with_a_hint() {
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "5");
    res(&mut c, "R1", "in", "0", 1000.0);
    match ac_analysis(&c, &sweep(1.0, 10.0, 5)) {
        Err(SimError::Invalid(m)) => assert!(m.contains("AC")),
        other => panic!("{other:?}"),
    }
}

fn wire(s: &mut Schematic, a: (i32, i32), b: (i32, i32)) {
    s.wires.push(Wire { a, b });
}

fn lib_part(name: &str) -> part_lib::PartDef {
    let text = part_lib::EXAMPLES.iter().find(|(n, _)| n.starts_with(name)).unwrap().1;
    serde_json::from_str(text).unwrap()
}

#[test]
fn op_amp_macro_model_open_loop_response() {
    // LM358 example: DC gain 1e5 with a pole at 1/(2 pi 1k 16u) = 9.95 Hz
    let u = lib_part("LM358");
    let mut s = Schematic::default();
    s.add(Kind::VSource, (0, 0), 0);
    s.components[0].value = "AC 1".into();
    s.add_custom(u.symbol(), &u.default_value(), &u.footprint_name(), (20, 0), 0);
    s.add(Kind::Ground, (0, 6), 0);
    s.add(Kind::Ground, (16, 6), 0);
    wire(&mut s, (0, -2), (0, -4));
    wire(&mut s, (0, -4), (16, -4));
    wire(&mut s, (16, -4), (16, -1)); // IN+
    wire(&mut s, (16, 1), (16, 6)); // IN- to ground
    wire(&mut s, (0, 2), (0, 6));
    let out_net = extract(&s).into_iter().find(|n| n.pins.iter().any(|p| p.refdes == "U1" && p.pin == "1")).unwrap().name;
    let mut c = from_schematic(&s).unwrap();
    // +-5 V supplies so the output sits mid-range at zero input
    let net_of = |pin: &str| extract(&s).into_iter().find(|n| n.pins.iter().any(|p| p.refdes == "U1" && p.pin == pin)).unwrap().name;
    let (vp, vn) = (c.node(&net_of("8")), c.node(&net_of("4")));
    c.add(Element::vdc("Vp", vp, 0, 5.0));
    c.add(Element::vdc("Vn", vn, 0, -5.0));
    let r = ac_analysis(&c, &sweep(0.1, 10e6, 10)).unwrap();
    let out = r.signal(&format!("V({out_net})")).unwrap();
    let pole = 1.0 / (2.0 * PI * 1000.0 * 16e-6);
    for (k, f) in r.freq.iter().enumerate() {
        let h = C64::new(1e5, 0.0) / C64::new(1.0, f / pole);
        assert!((out.re[k] - h.re).abs() < 1e-3 * h.abs() + 1e-9 && (out.im[k] - h.im).abs() < 1e-3 * h.abs() + 1e-9, "f = {f}");
    }
    let db = out.db();
    assert!((db[0] - 100.0).abs() < 0.01, "DC gain {}", db[0]);
    // unity-gain crossover ~ GBW = 1 MHz
    let k = db.iter().position(|d| *d < 0.0).unwrap();
    assert!(r.freq[k] > 0.9e6 && r.freq[k] < 1.2e6, "{}", r.freq[k]);
}
