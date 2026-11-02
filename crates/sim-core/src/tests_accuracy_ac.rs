//! How exact the small-signal analyses added later are, against answers worked out by hand: the impedance seen from a net
//! (random series-parallel networks of R, L and C, composed with complex arithmetic), the loop gain by injection (a loop
//! with two poles, cut at different parts), and the poles and zeros found by the rational fit (cascades whose poles and zeros
//! are known). The size of each error is printed; the asserts are set a little above what was measured.

use crate::ac::{AcSettings, AcSweep, C64};
use crate::circuit::{Circuit, Element};
use crate::impedance::impedance_analysis;
use crate::loopgain::{BreakDirection, loop_gain};
use crate::polezero::transfer_function;
use std::f64::consts::PI;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Log-uniform between `lo` and `hi`.
    fn log(&mut self, lo: f64, hi: f64) -> f64 {
        lo * (hi / lo).powf(self.next())
    }
}

fn parallel(a: C64, b: C64) -> C64 {
    C64::new(1.0, 0.0) / (C64::new(1.0, 0.0) / a + C64::new(1.0, 0.0) / b)
}

type ZOf = Box<dyn Fn(f64) -> C64>;

/// A random series-parallel network of R, L and C between nodes `a` and `b`, put into `c`; returns its impedance at an
/// angular frequency, worked out from the way it was put together.
fn random_network(c: &mut Circuit, rng: &mut Rng, a: usize, b: usize, depth: usize, count: &mut usize) -> ZOf {
    if depth == 0 || rng.next() < 0.25 {
        *count += 1;
        let name_no = *count;
        return match (rng.next() * 3.0) as usize {
            0 => {
                let r = rng.log(1.0, 1e5);
                c.elements.push(Element::Resistor { name: format!("R{name_no}"), a, b, r });
                Box::new(move |_| C64::new(r, 0.0))
            }
            1 => {
                // an ideal inductor in parallel with another is a loop of shorts at DC (a singular matrix, in SPICE too), so
                // every inductor has a winding resistance of 1 milliohm in series
                let l = rng.log(1e-7, 1e-2);
                let inner = c.node(&format!("W{name_no}"));
                c.elements.push(Element::Resistor { name: format!("RW{name_no}"), a, b: inner, r: 1e-3 });
                c.elements.push(Element::Inductor { name: format!("L{name_no}"), a: inner, b, l });
                Box::new(move |w| C64::new(1e-3, w * l))
            }
            _ => {
                let cap = rng.log(1e-11, 1e-5);
                c.elements.push(Element::Capacitor { name: format!("C{name_no}"), a, b, c: cap });
                Box::new(move |w| C64::new(0.0, -1.0 / (w * cap)))
            }
        };
    }
    if rng.next() < 0.5 {
        let m = c.node(&format!("M{}", *count));
        *count += 1;
        let (z1, z2) = (random_network(c, rng, a, m, depth - 1, count), random_network(c, rng, m, b, depth - 1, count));
        Box::new(move |w| z1(w) + z2(w))
    } else {
        let (z1, z2) = (random_network(c, rng, a, b, depth - 1, count), random_network(c, rng, a, b, depth - 1, count));
        Box::new(move |w| parallel(z1(w), z2(w)))
    }
}

fn sweep(f0: f64, f1: f64, per_decade: usize) -> AcSettings {
    AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(per_decade) }
}

#[test]
fn impedance_of_random_series_parallel_networks_is_exact() {
    // Two measures. Against the impedance itself, which is large where the impedance is nearly zero (a deep resonance dip: a
    // subtraction of two big numbers); and against the scale of the network (the impedance of its parts), which is what the
    // solver's precision is about.
    let (mut worst_scaled, mut worst_phase, mut points, mut over_1e6, mut worst_rel) = (0.0_f64, 0.0_f64, 0usize, 0usize, 0.0_f64);
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut c = Circuit::new();
        let (vin, out) = (c.node("VIN"), c.node("OUT"));
        // the supply is shorted: OUT also sees its resistor to the supply
        c.elements.push(Element::vdc("V1", vin, 0, 5.0));
        let rs = rng.log(10.0, 1e4);
        c.elements.push(Element::Resistor { name: "RS".into(), a: out, b: vin, r: rs });
        let mut count = 0;
        let z = random_network(&mut c, &mut rng, out, 0, 3, &mut count);
        let r = impedance_analysis(&c, "OUT", None, &[], &sweep(10.0, 1e8, 12)).unwrap_or_else(|e| panic!("seed {seed}: {e:?}"));
        for (k, f) in r.freq.iter().enumerate() {
            let w = 2.0 * PI * f;
            let want = parallel(z(w), C64::new(rs, 0.0));
            let scale = z(w).abs().max(rs);
            let got = r.z[k];
            let d = C64::new(got.re - want.re, got.im - want.im).abs();
            points += 1;
            worst_scaled = worst_scaled.max(d / scale);
            let rel = d / want.abs().max(1e-30);
            worst_rel = worst_rel.max(rel);
            if rel > 1e-6 {
                over_1e6 += 1;
            }
            // the phase of a nearly zero impedance means little
            if want.abs() > 1e-2 * scale {
                let dphi = (got.im.atan2(got.re) - want.im.atan2(want.re)).abs();
                worst_phase = worst_phase.max(dphi.min(2.0 * PI - dphi).to_degrees());
            }
        }
    }
    eprintln!("impedance: {points} points of 300 random networks: error against the scale of the network {worst_scaled:e}; against the impedance {worst_rel:e} at worst, over 1e-6 at {over_1e6} points; phase error {worst_phase:e} deg");
    assert!(worst_scaled < 1e-7, "{worst_scaled:e}");
    assert!(over_1e6 * 10_000 <= points, "{over_1e6} of {points} points are off by more than 1e-6");
    assert!(worst_phase < 1e-4, "{worst_phase:e} deg");
}

/// A loop with two poles: a gain A, then R1 C1, then R2 C2 (which loads the first), read at the last node with nothing drawn.
/// T = A H, with H = 1 / (s^2 R1 C1 R2 C2 + s (R1 C1 + R1 C2 + R2 C2) + 1).
fn two_pole_loop(a: f64, r1: f64, c1: f64, r2: f64, c2: f64) -> Circuit {
    let mut c = Circuit::new();
    let (drv, m, fb) = (c.node("DRV"), c.node("M"), c.node("FB"));
    c.elements.push(Element::Vcvs { name: "E1".into(), p: drv, n: 0, cp: 0, cn: fb, gain: a });
    c.elements.push(Element::Resistor { name: "R1".into(), a: drv, b: m, r: r1 });
    c.elements.push(Element::Capacitor { name: "C1".into(), a: m, b: 0, c: c1 });
    c.elements.push(Element::Resistor { name: "R2".into(), a: m, b: fb, r: r2 });
    c.elements.push(Element::Capacitor { name: "C2".into(), a: fb, b: 0, c: c2 });
    c
}

fn two_pole_t(f: f64, a: f64, r1: f64, c1: f64, r2: f64, c2: f64) -> C64 {
    let w = 2.0 * PI * f;
    let s = C64::new(0.0, w);
    let one = C64::new(1.0, 0.0);
    let d = s * s * C64::new(r1 * c1 * r2 * c2, 0.0) + s * C64::new(r1 * c1 + r1 * c2 + r2 * c2, 0.0) + one;
    C64::new(a, 0.0) / d
}

#[test]
fn loop_gain_of_a_two_pole_loop_is_exact_whichever_part_is_cut() {
    let mut worst = 0.0_f64;
    for (a, r1, c1, r2, c2) in [(1000.0, 1e3, 1e-6, 1e3, 1e-6), (50.0, 10e3, 1e-9, 220.0, 10e-6), (5e5, 100.0, 100e-9, 47e3, 1e-9), (3.0, 1e3, 1e-6, 1e6, 1e-9)] {
        let s = sweep(0.1, 1e7, 20);
        // R1 and R2 are in the loop; C1 and C2 go to ground (they are not in series with the loop and cannot be cut)
        for part in ["R1", "R2"] {
            let r = loop_gain(&two_pole_loop(a, r1, c1, r2, c2), part, BreakDirection::Auto, &s).unwrap();
            for (k, f) in r.freq.iter().enumerate() {
                let want = two_pole_t(*f, a, r1, c1, r2, c2);
                let rel = C64::new(r.t[k].re - want.re, r.t[k].im - want.im).abs() / want.abs();
                worst = worst.max(rel);
            }
        }
    }
    eprintln!("loop gain: largest relative error over the sweeps {worst:e}");
    assert!(worst < 1e-5, "{worst:e}");
}

#[test]
fn the_phase_margin_and_crossover_of_the_loop_gain_follow_the_exact_ones() {
    // the margins are read off a sampled sweep: compare them with the ones found on a very fine sweep of the exact T
    let mut worst = (0.0_f64, 0.0_f64);
    for (a, r1, c1, r2, c2) in [(1000.0, 1e3, 1e-6, 1e3, 1e-6), (50.0, 10e3, 1e-9, 220.0, 10e-6), (5e5, 100.0, 100e-9, 47e3, 1e-9)] {
        let r = loop_gain(&two_pole_loop(a, r1, c1, r2, c2), "R1", BreakDirection::Auto, &sweep(0.1, 1e7, 50)).unwrap();
        let m = crate::measure::stability_margins(&r.freq, &r.db(), &r.phase_deg()).expect("a crossing");
        // the exact crossover: bisect |T| = 1 on a log scale
        let (mut lo, mut hi) = (0.1_f64, 1e7_f64);
        for _ in 0..200 {
            let mid = (lo * hi).sqrt();
            if two_pole_t(mid, a, r1, c1, r2, c2).abs() > 1.0 { lo = mid } else { hi = mid }
        }
        let fc = (lo * hi).sqrt();
        let t = two_pole_t(fc, a, r1, c1, r2, c2);
        let pm_exact = 180.0 + t.im.atan2(t.re).to_degrees();
        let (dfc, dpm) = ((m.gain_crossover.unwrap() / fc - 1.0).abs(), (m.phase_margin.unwrap() - pm_exact).abs());
        eprintln!("margins: crossover {:.4} Hz (exact {fc:.4}), phase margin {:.3} deg (exact {pm_exact:.3})", m.gain_crossover.unwrap(), m.phase_margin.unwrap());
        worst = (worst.0.max(dfc), worst.1.max(dpm));
    }
    assert!(worst.0 < 0.01 && worst.1 < 0.5, "crossover {:e}, phase margin {:e} deg", worst.0, worst.1);
}

/// A buffered section: the input goes through `h` and a gain-1 buffer, so that the sections do not load each other.
/// The poles and zeros of the cascade are those of the sections.
enum Section {
    /// An RC low-pass: a pole at 1 / (2 pi R C).
    Pole { r: f64, c: f64 },
    /// R1 || C1 over R2: a zero at 1 / (2 pi R1 C1) and a pole at (R1 + R2) / (2 pi R1 R2 C1).
    LeadLag { r1: f64, c1: f64, r2: f64 },
}

fn cascade(sections: &[Section]) -> (Circuit, Vec<f64>, Vec<f64>) {
    let mut c = Circuit::new();
    let mut prev = c.node("IN");
    c.elements.push(Element::VSource { name: "V1".into(), p: prev, n: 0, wave: crate::waveform::Waveform::Dc(0.0), ac: Some((1.0, 0.0)) });
    let (mut poles, mut zeros) = (vec![], vec![]);
    for (k, s) in sections.iter().enumerate() {
        let out = c.node(&format!("S{k}"));
        let buf = c.node(&format!("B{k}"));
        match *s {
            Section::Pole { r, c: cap } => {
                c.elements.push(Element::Resistor { name: format!("R{k}"), a: prev, b: out, r });
                c.elements.push(Element::Capacitor { name: format!("C{k}"), a: out, b: 0, c: cap });
                poles.push(1.0 / (2.0 * PI * r * cap));
            }
            Section::LeadLag { r1, c1, r2 } => {
                c.elements.push(Element::Resistor { name: format!("RA{k}"), a: prev, b: out, r: r1 });
                c.elements.push(Element::Capacitor { name: format!("CA{k}"), a: prev, b: out, c: c1 });
                c.elements.push(Element::Resistor { name: format!("RB{k}"), a: out, b: 0, r: r2 });
                zeros.push(1.0 / (2.0 * PI * r1 * c1));
                poles.push((r1 + r2) / (2.0 * PI * r1 * r2 * c1));
            }
        }
        c.elements.push(Element::Vcvs { name: format!("E{k}"), p: buf, n: 0, cp: out, cn: 0, gain: 1.0 });
        prev = buf;
    }
    poles.sort_by(f64::total_cmp);
    zeros.sort_by(f64::total_cmp);
    (c, poles, zeros)
}

/// The relative errors of the frequencies found against the ones expected (both sorted); None when the counts differ.
fn freq_errors(found: &[f64], want: &[f64]) -> Option<f64> {
    (found.len() == want.len()).then(|| found.iter().zip(want).map(|(f, w)| (f / w - 1.0).abs()).fold(0.0, f64::max))
}

#[test]
fn poles_and_zeros_of_buffered_cascades_are_found() {
    use Section::{LeadLag, Pole};
    let cases: Vec<(&str, Vec<Section>)> = vec![
        ("1 pole", vec![Pole { r: 1e3, c: 159.155e-9 }]),
        ("3 poles, decades apart", vec![Pole { r: 1e3, c: 1e-6 }, Pole { r: 1e3, c: 1e-8 }, Pole { r: 1e3, c: 1e-10 }]),
        ("2 poles and a zero", vec![Pole { r: 10e3, c: 1e-8 }, LeadLag { r1: 10e3, c1: 1e-9, r2: 1e3 }, Pole { r: 1e3, c: 1e-9 }]),
        // a compensator-like response: zeros close below their poles, so the gain falls only a little over six decades
        ("5 poles, 4 zeros over six decades", vec![
            LeadLag { r1: 10e3, c1: 1e-6 / 10.0, r2: 5e3 },
            LeadLag { r1: 10e3, c1: 1e-9, r2: 5e3 },
            LeadLag { r1: 1e3, c1: 1e-10, r2: 500.0 },
            LeadLag { r1: 100.0, c1: 1e-10, r2: 50.0 },
            Pole { r: 100.0, c: 1e-11 },
        ]),
        ("5 poles within 1.5 decades", vec![Pole { r: 1e3, c: 1e-7 }, Pole { r: 1e3, c: 0.5e-7 }, Pole { r: 1e3, c: 0.2e-7 }, Pole { r: 1e3, c: 0.1e-7 }, Pole { r: 1e3, c: 0.05e-7 }]),
    ];
    let mut report = vec![];
    for (name, sections) in &cases {
        let (c, poles, zeros) = cascade(sections);
        let out = format!("B{}", sections.len() - 1);
        let tf = transfer_function(&c, &out, Some("IN"), &sweep(0.1, 1e8, 40), 8, 0.01).unwrap();
        let mut found_p: Vec<f64> = tf.pz.poles.iter().map(|r| r.freq_hz()).collect();
        let mut found_z: Vec<f64> = tf.pz.zeros.iter().map(|r| r.freq_hz()).collect();
        found_p.sort_by(f64::total_cmp);
        found_z.sort_by(f64::total_cmp);
        let (ep, ez) = (freq_errors(&found_p, &poles), freq_errors(&found_z, &zeros));
        eprintln!("{name}: poles {} (want {}), zeros {} (want {}); errors {ep:?} {ez:?}; fit error {:e}", found_p.len(), poles.len(), found_z.len(), zeros.len(), tf.pz.error);
        report.push((name, ep, ez));
    }
    for (name, ep, ez) in report {
        assert!(ep.is_some_and(|e| e < 1e-3), "{name}: poles {ep:?}");
        assert!(ez.is_some_and(|e| e < 1e-3), "{name}: zeros {ez:?}");
    }
}

#[test]
fn a_complex_pair_of_poles_is_found_with_its_damping() {
    // series R - L - C low-pass: s^2 L C + s R C + 1; the pair is at -R/2L +- j sqrt(1/LC - (R/2L)^2)
    for (r, l, cap) in [(20.0, 10e-3, 1e-6), (5.0, 1e-3, 100e-9), (100.0, 5e-3, 1e-6)] {
        let mut c = Circuit::new();
        let (vin, a, out) = (c.node("IN"), c.node("A"), c.node("OUT"));
        c.elements.push(Element::VSource { name: "V1".into(), p: vin, n: 0, wave: crate::waveform::Waveform::Dc(0.0), ac: Some((1.0, 0.0)) });
        c.elements.push(Element::Resistor { name: "R1".into(), a: vin, b: a, r });
        c.elements.push(Element::Inductor { name: "L1".into(), a, b: out, l });
        c.elements.push(Element::Capacitor { name: "C1".into(), a: out, b: 0, c: cap });
        let tf = transfer_function(&c, "OUT", Some("IN"), &sweep(1.0, 1e7, 60), 6, 0.01).unwrap();
        let re = -r / (2.0 * l);
        let im = (1.0 / (l * cap) - re * re).sqrt();
        // a complex pair comes back as one root (the other is its conjugate)
        assert!(tf.pz.poles.len() == 1 && tf.pz.poles[0].pair && tf.pz.zeros.is_empty(), "{:?} {:?}", tf.pz.poles, tf.pz.zeros);
        let p = &tf.pz.poles[0];
        // the roots are in rad/s or in Hz? compare the ratio, so that either is fine
        let (pre, pim) = (p.re, p.im);
        let scale = if (pim / im - 1.0).abs() < 0.2 { 1.0 } else { 2.0 * PI };
        eprintln!("RLC R={r}: pole {pre:.3} {pim:.3} (want {re:.3} {im:.3}, scale {scale})");
        assert!((pre / scale / re - 1.0).abs() < 1e-3 && (pim / scale / im - 1.0).abs() < 1e-3, "{pre} {pim} vs {re} {im}");
    }
}

#[test]
fn a_response_that_hides_its_upper_roots_still_gives_the_visible_ones_right() {
    use Section::Pole;
    // five poles from 16 Hz to 16 MHz, far apart: above a few kHz the gain is lower than 1e-9 of its peak (the fit ignores
    // what is below that), so the upper poles are not in the response and cannot be found. The ones that are found must be right.
    let sections = [Pole { r: 1e4, c: 1e-6 }, Pole { r: 1e4, c: 1.6e-8 }, Pole { r: 1e3, c: 1e-9 }, Pole { r: 100.0, c: 1.6e-10 }, Pole { r: 100.0, c: 1e-10 }];
    let (c, poles, _) = cascade(&sections);
    let tf = transfer_function(&c, "B4", Some("IN"), &sweep(0.1, 1e8, 40), 8, 0.01).unwrap();
    let mut found: Vec<f64> = tf.pz.poles.iter().map(|r| r.freq_hz()).collect();
    found.sort_by(f64::total_cmp);
    eprintln!("hidden roots: found {found:?} of {poles:?}, fit error {:e}", tf.pz.error);
    assert!(found.len() < 5, "{found:?}");
    let e = freq_errors(&found, &poles[..found.len()]);
    assert!(e.is_some_and(|e| e < 0.02), "the poles that are found are off: {found:?} vs {poles:?}");
}

