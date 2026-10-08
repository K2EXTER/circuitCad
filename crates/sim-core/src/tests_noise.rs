use crate::*;
use sch_core::{extract, Kind, Schematic, Wire};
use std::f64::consts::PI;

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

fn rms_ac(r: &TranResult, name: &str, t0: f64, t1: f64) -> f64 {
    let s = r.signal(name).unwrap();
    stats(&r.time, &s.values, t0, t1).unwrap().rms_ac
}

#[test]
fn a_capacitor_filters_white_noise_as_predicted() {
    let (vrms, fmax) = (1.0, 5e3);
    let fc = 50.0;
    let cc = 1.0 / (2.0 * PI * 1000.0 * fc);
    let run = |with_cap: bool| {
        let mut c = Circuit::new();
        src(&mut c, "V1", "in", "0", &format!("NOISE({vrms} {fmax} 3)"));
        res(&mut c, "R1", "in", "out", 1000.0);
        res(&mut c, "R2", "out", "0", 1000.0);
        if with_cap {
            cap(&mut c, "C1", "out", "0", cc);
        }
        transient(&c, &TranSettings { tstop: 2.0, tstep: 25e-6, uic: false }).unwrap()
    };
    let bare = run(false);
    let filtered = run(true);
    assert!(bare.warnings.is_empty(), "{:?}", bare.warnings);

    // source: RMS as requested, no DC
    let s = stats(&bare.time, &bare.signal("V(in)").unwrap().values, 0.1, 2.0).unwrap();
    assert!((s.rms_ac - vrms).abs() < 0.05, "source rms {}", s.rms_ac);
    assert!(s.mean.abs() < 0.05);

    // without the capacitor the 1k/1k divider just halves the noise
    let half = rms_ac(&bare, "V(out)", 0.1, 2.0);
    assert!((half - 0.5 * s.rms_ac).abs() < 0.01, "{half}");

    // with it: the divider's Thevenin resistance is 500 ohm, so the RC pole is at 1/(2 pi 500 C)
    // = 2*fc... compute the prediction from that pole: out = 0.5 * lowpass(pole) of the noise
    let pole = 1.0 / (2.0 * PI * 500.0 * cc);
    let predicted = 0.5 * vrms * (1.5 * PI * pole / (2.0 * fmax)).sqrt();
    let got = rms_ac(&filtered, "V(out)", 0.1, 2.0);
    assert!((got - predicted).abs() / predicted < 0.15, "{got} vs {predicted}");
    assert!(got < 0.4 * half, "the capacitor must remove most of the noise");
}

#[test]
fn noise_runs_are_reproducible_and_seed_dependent() {
    let make = |seed: u32| {
        let mut c = Circuit::new();
        src(&mut c, "V1", "in", "0", &format!("NOISE(0.1 10k {seed})"));
        res(&mut c, "R1", "in", "0", 1000.0);
        transient(&c, &TranSettings { tstop: 5e-3, tstep: 10e-6, uic: false }).unwrap()
    };
    let (a, b, d) = (make(1), make(1), make(2));
    assert_eq!(a.signal("V(in)").unwrap().values, b.signal("V(in)").unwrap().values);
    assert_ne!(a.signal("V(in)").unwrap().values, d.signal("V(in)").unwrap().values);
}

#[test]
fn dc_plus_ripple_plus_noise_source() {
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "5 + SIN(0 0.1 1k) + NOISE(0.01 100k)");
    res(&mut c, "R1", "in", "0", 1000.0);
    let r = transient(&c, &TranSettings { tstop: 10e-3, tstep: 1e-6, uic: false }).unwrap();
    let s = stats(&r.time, &r.signal("V(in)").unwrap().values, 0.0, 10e-3).unwrap();
    assert!((s.mean - 5.0).abs() < 0.01, "{}", s.mean);
    // ripple rms 0.1/sqrt2 = 0.0707, noise 0.01 on top
    let want = (0.1f64.powi(2) / 2.0 + 0.01f64.powi(2)).sqrt();
    assert!((s.rms_ac - want).abs() / want < 0.05, "{} vs {want}", s.rms_ac);
    // the DC operating point sees only the DC part
    let dc = dc_op(&{
        let mut c = Circuit::new();
        src(&mut c, "V1", "in", "0", "5 + SIN(0 0.1 1k) + NOISE(0.01 100k)");
        res(&mut c, "R1", "in", "0", 1000.0);
        c
    })
    .unwrap();
    assert!((dc.voltage("in").unwrap() - 5.0).abs() < 1e-9);
}

#[test]
fn spectrum_shows_the_removed_ripple() {
    // 10 kHz ripple on 5 V through RC with fc = 1 kHz is attenuated by about 20 dB
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "5 + SIN(0 0.1 10k)");
    res(&mut c, "R1", "in", "out", 1000.0);
    cap(&mut c, "C1", "out", "0", 159.155e-9);
    let r = transient(&c, &TranSettings { tstop: 20e-3, tstep: 1e-6, uic: false }).unwrap();
    let peak = |name: &str| {
        let s = r.signal(name).unwrap();
        let sp = spectrum(&r.time, &s.values, 5e-3, 20e-3).unwrap();
        sp.freq.iter().zip(&sp.amp).filter(|(f, _)| (**f - 10e3).abs() < 300.0).map(|(_, a)| *a).fold(0.0, f64::max)
    };
    let (a_in, a_out) = (peak("V(in)"), peak("V(out)"));
    assert!((a_in - 0.1).abs() < 0.005, "{a_in}");
    let h = 1.0 / (1.0 + (10e3f64 / 1000.0).powi(2)).sqrt();
    assert!((a_out / a_in - h).abs() / h < 0.03, "{} vs {h}", a_out / a_in);
}

#[test]
fn coarse_time_step_is_reported() {
    let mut c = Circuit::new();
    src(&mut c, "V1", "in", "0", "NOISE(1 100k)"); // samples every 5 us
    res(&mut c, "R1", "in", "0", 1000.0);
    let r = transient(&c, &TranSettings { tstop: 1e-3, tstep: 4e-6, uic: false }).unwrap();
    assert!(r.warnings.iter().any(|w| w.contains("V1") && w.contains("時間刻み")), "{:?}", r.warnings);
}

// ---------------------------------------------------------------- capacitor parasitics

fn wire(s: &mut Schematic, a: (i32, i32), b: (i32, i32)) {
    s.wires.push(Wire { a, b });
}

/// V1 --R1(10)-- out --C1-- GND
fn rc_with_cap(value: &str, source: &str) -> Schematic {
    let mut s = Schematic::default();
    s.add(Kind::VSource, (0, 0), 0);
    s.components[0].value = source.into();
    s.add(Kind::Resistor, (6, -6), 0);
    s.components[1].value = "10".into();
    s.add(Kind::Capacitor, (12, 0), 1);
    s.components[2].value = value.into();
    s.add(Kind::Ground, (0, 6), 0);
    for (a, b) in [((0, -2), (0, -6)), ((0, -6), (4, -6)), ((8, -6), (12, -6)), ((12, -6), (12, -2)), ((12, 2), (12, 6)), ((12, 6), (0, 6)), ((0, 2), (0, 6))] {
        wire(&mut s, a, b);
    }
    s
}

#[test]
fn real_capacitor_has_a_self_resonance() {
    let (cc, esr, esl) = (100e-9, 0.05, 10e-9);
    let s = rc_with_cap("100n ESR=0.05 ESL=10n", "AC 1");
    let out_net = extract(&s).into_iter().find(|n| n.pins.iter().any(|p| p.refdes == "C1" && p.pin == "1")).unwrap().name;
    let c = from_schematic(&s).unwrap();
    assert!(c.elements.iter().any(|e| e.name() == "C1.ESR") && c.elements.iter().any(|e| e.name() == "C1.ESL"));
    let r = ac_analysis(&c, &AcSettings { fstart: 1e3, fstop: 1e9, sweep: AcSweep::Decade(40) }).unwrap();
    let out = r.signal(&format!("V({out_net})")).unwrap();
    let mut min = (f64::MAX, 0.0);
    for (k, f) in r.freq.iter().enumerate() {
        let w = 2.0 * PI * f;
        let z = C64::new(esr, w * esl - 1.0 / (w * cc));
        let h = z / (C64::new(10.0, 0.0) + z);
        assert!((out.re[k] - h.re).abs() < 1e-7 && (out.im[k] - h.im).abs() < 1e-7, "f = {f}");
        let m = out.re[k].hypot(out.im[k]);
        if m < min.0 {
            min = (m, *f);
        }
    }
    // deepest notch sits at the self-resonant frequency, with depth ESR/(R+ESR)
    let srf = 1.0 / (2.0 * PI * (esl * cc).sqrt());
    assert!((min.1 / srf - 1.0).abs() < 0.07, "{} vs {srf}", min.1);
    assert!((min.0 - esr / (10.0 + esr)).abs() / (esr / 10.05) < 0.05, "{}", min.0);
    // above the resonance it is an inductor again: the attenuation gets worse
    let at = |f: f64| {
        let k = r.freq.iter().position(|x| (x / f - 1.0).abs() < 0.03).unwrap();
        out.re[k].hypot(out.im[k])
    };
    assert!(at(500e6) > 3.0 * min.0 * 10.0);
}

#[test]
fn ideal_capacitor_has_no_notch() {
    let s = rc_with_cap("100n", "AC 1");
    let c = from_schematic(&s).unwrap();
    assert!(!c.elements.iter().any(|e| e.name().contains('.')));
}

#[test]
fn inductor_winding_resistance() {
    let mut s = Schematic::default();
    s.add(Kind::VSource, (0, 0), 0);
    s.components[0].value = "5".into();
    s.add(Kind::Inductor, (6, -6), 0);
    s.components[1].value = "10u DCR=2".into();
    s.add(Kind::Resistor, (12, 0), 1);
    s.components[2].value = "3".into();
    s.add(Kind::Ground, (0, 6), 0);
    for (a, b) in [((0, -2), (0, -6)), ((0, -6), (4, -6)), ((8, -6), (12, -6)), ((12, -6), (12, -2)), ((12, 2), (12, 6)), ((12, 6), (0, 6)), ((0, 2), (0, 6))] {
        wire(&mut s, a, b);
    }
    let r = dc_op(&from_schematic(&s).unwrap()).unwrap();
    assert!((r.element("L1").unwrap().current - 1.0).abs() < 1e-6);
    assert!((r.element("L1.DCR").unwrap().voltage - 2.0).abs() < 1e-6);
}

#[test]
fn unknown_or_misplaced_parasitics_are_errors() {
    let mut s = rc_with_cap("100n ESQ=1", "5");
    assert!(from_schematic(&s).unwrap_err()[0].message.contains("ESQ"));
    s.components[2].value = "100n ESR=oops".into();
    assert!(from_schematic(&s).is_err());
    // ESR does not belong on a resistor
    s.components[2].value = "100n".into();
    s.components[1].value = "10 ESR=1".into();
    let e = from_schematic(&s).unwrap_err();
    assert_eq!(e[0].refdes, "R1");
}
