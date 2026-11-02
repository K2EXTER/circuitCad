//! The worst-case analysis against brute force. `worst_case` finds its corners with a short cut (each part alone, then one
//! round of flips); here every corner (2^n of them) is solved and the true extremes are compared with what the short cut
//! found. A second check draws random circuits from the inside of the tolerance box: none may go beyond the extremes,
//! which is what the analysis assumes by looking only at the ends of each part.

use crate::circuit::{Circuit, Element};
use crate::montecarlo::{McMeasure, TranMetric, measure, scale};
use crate::waveform::Waveform;
use crate::worstcase::{WcSettings, corner, tolerance, worst_case};

/// The result of every one of the 2^n corners: (smallest, largest).
fn brute_force(circuit: &Circuit, s: &WcSettings) -> (f64, f64, usize) {
    let parts: Vec<(usize, f64)> = circuit.elements.iter().enumerate().filter_map(|(i, e)| tolerance(e, s).map(|t| (i, t))).collect();
    let n = parts.len();
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for mask in 0..(1u32 << n) {
        let high: Vec<bool> = (0..n).map(|k| mask >> k & 1 == 1).collect();
        if let Ok(v) = measure(&corner(circuit, &parts, &high), s.output.trim(), &s.measure)
            && v.is_finite()
        {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    (lo, hi, n)
}

/// Random circuits from inside the box: (smallest, largest).
fn inside_the_box(circuit: &Circuit, s: &WcSettings, samples: usize) -> (f64, f64) {
    let parts: Vec<(usize, f64)> = circuit.elements.iter().enumerate().filter_map(|(i, e)| tolerance(e, s).map(|t| (i, t))).collect();
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for _ in 0..samples {
        let mut c = circuit.clone();
        for &(i, tol) in &parts {
            scale(&mut c.elements[i], 1.0 + tol * (2.0 * next() - 1.0));
        }
        if let Ok(v) = measure(&c, s.output.trim(), &s.measure)
            && v.is_finite()
        {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    (lo, hi)
}

/// Compares the analysis with the brute force and with the inside of the box. `samples` is how many random circuits.
fn check(label: &str, circuit: &Circuit, s: &WcSettings, samples: usize) {
    let w = worst_case(circuit, s).unwrap();
    let (b_lo, b_hi, n) = brute_force(circuit, s);
    // the printed values are one of the possible circuits
    let (b_lo, b_hi) = (b_lo.min(w.nominal), b_hi.max(w.nominal));
    let (r_lo, r_hi) = inside_the_box(circuit, s, samples);
    let eps = 1e-9 + 1e-6 * b_lo.abs().max(b_hi.abs());
    eprintln!(
        "{label}: n={n}  min {:.6} (brute {:.6}, inside {:.6})  max {:.6} (brute {:.6}, inside {:.6})  nominal {:.6}",
        w.min, b_lo, r_lo, w.max, b_hi, r_hi, w.nominal
    );
    assert!(w.max >= b_hi - eps, "{label}: the short cut missed the largest corner: {} < {}", w.max, b_hi);
    assert!(w.min <= b_lo + eps, "{label}: the short cut missed the smallest corner: {} > {}", w.min, b_lo);
    // the short cut solves real corners, so it cannot go past the brute force either
    assert!(w.max <= b_hi + eps && w.min >= b_lo - eps, "{label}: beyond the corners: {}..{} vs {}..{}", w.min, w.max, b_lo, b_hi);
    // the inside of the box stays inside the limits
    assert!(r_hi <= w.max + eps && r_lo >= w.min - eps, "{label}: a circuit inside the tolerances is outside the limits: {r_lo}..{r_hi} vs {}..{}", w.min, w.max);
}

fn dc(tol_r: f64, tol_v: f64, output: &str) -> WcSettings {
    WcSettings { tol_r, tol_c: 0.0, tol_l: 0.0, tol_v, output: output.into(), measure: McMeasure::DcVoltage }
}

fn resistor(c: &mut Circuit, name: &str, a: usize, b: usize, r: f64) {
    c.elements.push(Element::Resistor { name: name.into(), a, b, r });
}

#[test]
fn a_divider_matches_brute_force() {
    let mut c = Circuit::new();
    let (vin, mid) = (c.node("VIN"), c.node("MID"));
    c.elements.push(Element::vdc("V1", vin, 0, 5.0));
    resistor(&mut c, "R1", vin, mid, 10e3);
    resistor(&mut c, "R2", mid, 0, 10e3);
    check("divider", &c, &dc(0.05, 0.05, "MID"), 500);
}

/// A bridge with a resistor across it. How the voltage at A moves with the bridge resistor depends on whether A is above or
/// below B, so the end that pushes the result up is not the same in every corner: the case the one round of flips is for.
fn bridge(r1: f64, r2: f64, r3: f64, r4: f64, rb: f64) -> Circuit {
    let mut c = Circuit::new();
    let (vin, a, b) = (c.node("VIN"), c.node("A"), c.node("B"));
    c.elements.push(Element::vdc("V1", vin, 0, 10.0));
    resistor(&mut c, "R1", vin, a, r1);
    resistor(&mut c, "R2", a, 0, r2);
    resistor(&mut c, "R3", vin, b, r3);
    resistor(&mut c, "R4", b, 0, r4);
    resistor(&mut c, "RB", a, b, rb);
    c
}

#[test]
fn a_balanced_bridge_matches_brute_force() {
    // balanced: A and B are at the same voltage, the case where the effect of RB changes sign
    check("bridge balanced", &bridge(1e3, 1e3, 1e3, 1e3, 1e3), &dc(0.1, 0.05, "A"), 800);
}

#[test]
fn bridges_that_are_off_balance_match_brute_force() {
    for (name, c) in [
        ("bridge 1:2", bridge(1e3, 2e3, 1e3, 1e3, 1e3)),
        ("bridge strong load", bridge(1e3, 1e3, 2.2e3, 1e3, 100.0)),
        ("bridge weak load", bridge(4.7e3, 1e3, 1e3, 4.7e3, 100e3)),
        ("bridge lopsided", bridge(100.0, 10e3, 10e3, 100.0, 1e3)),
    ] {
        // wide tolerances, so that the non-linear effects are strong
        check(name, &c, &dc(0.3, 0.0, "A"), 800);
        check(&format!("{name} (B)"), &c, &dc(0.3, 0.0, "B"), 800);
    }
}

#[test]
fn a_resistor_ladder_matches_brute_force() {
    let mut c = Circuit::new();
    let top = c.node("N0");
    c.elements.push(Element::vdc("V1", top, 0, 5.0));
    let mut prev = top;
    for k in 1..=4 {
        let n = c.node(&format!("N{k}"));
        resistor(&mut c, &format!("RS{k}"), prev, n, 1e3 * k as f64);
        resistor(&mut c, &format!("RP{k}"), n, 0, 2.2e3 + 500.0 * k as f64);
        prev = n;
    }
    // 8 resistors and the supply: 512 corners
    check("ladder N4", &c, &dc(0.2, 0.05, "N4"), 600);
    check("ladder N2", &c, &dc(0.2, 0.0, "N2"), 600);
}

fn ac_source(c: &mut Circuit, p: usize) {
    c.elements.push(Element::VSource { name: "V1".into(), p, n: 0, wave: Waveform::Dc(0.0), ac: Some((1.0, 0.0)) });
}

#[test]
fn an_rc_low_pass_cutoff_matches_brute_force() {
    let mut c = Circuit::new();
    let (vin, out) = (c.node("VIN"), c.node("OUT"));
    ac_source(&mut c, vin);
    resistor(&mut c, "R1", vin, out, 1e3);
    c.elements.push(Element::Capacitor { name: "C1".into(), a: out, b: 0, c: 100e-9 });
    let s = WcSettings { tol_r: 0.1, tol_c: 0.2, tol_l: 0.0, tol_v: 0.0, output: "OUT".into(), measure: McMeasure::Cutoff { fstart: 10.0, fstop: 1e6, points_per_decade: 100 } };
    check("rc cutoff", &c, &s, 300);
}

#[test]
fn a_two_stage_filter_cutoff_matches_brute_force() {
    // the second stage loads the first, so the four parts do not act independently
    let mut c = Circuit::new();
    let (vin, m, out) = (c.node("VIN"), c.node("M"), c.node("OUT"));
    ac_source(&mut c, vin);
    resistor(&mut c, "R1", vin, m, 1e3);
    c.elements.push(Element::Capacitor { name: "C1".into(), a: m, b: 0, c: 100e-9 });
    resistor(&mut c, "R2", m, out, 3.3e3);
    c.elements.push(Element::Capacitor { name: "C2".into(), a: out, b: 0, c: 47e-9 });
    let s = WcSettings { tol_r: 0.2, tol_c: 0.3, tol_l: 0.0, tol_v: 0.0, output: "OUT".into(), measure: McMeasure::Cutoff { fstart: 10.0, fstop: 1e6, points_per_decade: 100 } };
    check("two-stage cutoff", &c, &s, 300);
}

#[test]
fn a_wien_bridge_band_pass_cutoff_matches_brute_force() {
    // R1 + C1 in series, then R2 || C2 to ground: a band-pass; the lower edge is the one that is reported
    let mut c = Circuit::new();
    let (vin, m, out) = (c.node("VIN"), c.node("M"), c.node("OUT"));
    ac_source(&mut c, vin);
    resistor(&mut c, "R1", vin, m, 10e3);
    c.elements.push(Element::Capacitor { name: "C1".into(), a: m, b: out, c: 10e-9 });
    resistor(&mut c, "R2", out, 0, 10e3);
    c.elements.push(Element::Capacitor { name: "C2".into(), a: out, b: 0, c: 10e-9 });
    let s = WcSettings { tol_r: 0.2, tol_c: 0.3, tol_l: 0.0, tol_v: 0.0, output: "OUT".into(), measure: McMeasure::Cutoff { fstart: 10.0, fstop: 1e6, points_per_decade: 100 } };
    check("wien band-pass", &c, &s, 300);
}

#[test]
fn an_rlc_overshoot_matches_brute_force() {
    // a step into R - L - C: the overshoot falls as the damping rises, and the damping depends on all three parts
    let mut c = Circuit::new();
    let (vin, a, out) = (c.node("VIN"), c.node("A"), c.node("OUT"));
    c.elements.push(Element::VSource { name: "V1".into(), p: vin, n: 0, wave: Waveform::Pulse { v1: 0.0, v2: 1.0, td: 10e-6, tr: 1e-6, tf: 1e-6, pw: 1.0, per: 2.0 }, ac: None });
    resistor(&mut c, "R1", vin, a, 20.0);
    c.elements.push(Element::Inductor { name: "L1".into(), a, b: out, l: 10e-3 });
    c.elements.push(Element::Capacitor { name: "C1".into(), a: out, b: 0, c: 1e-6 });
    let s = WcSettings {
        tol_r: 0.1,
        tol_c: 0.2,
        tol_l: 0.2,
        tol_v: 0.0,
        output: "OUT".into(),
        // long enough for the least damped corner to settle (its envelope decays with a time constant of about 1.4 ms)
        measure: McMeasure::Tran { tstop: 40e-3, tstep: 10e-6, uic: false, metric: TranMetric::Overshoot },
    };
    check("rlc overshoot", &c, &s, 60);
}

/// A small random network of resistors: a chain from the supply to ground through `inner` nodes, and a few extra
/// resistors between random pairs of nodes (which is what makes a part push the result up in one corner and down in another).
fn random_network(seed: u64, inner: usize, extra: usize) -> (Circuit, String) {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut c = Circuit::new();
    let vin = c.node("VIN");
    c.elements.push(Element::vdc("V1", vin, 0, 10.0));
    let nodes: Vec<usize> = std::iter::once(vin).chain((1..=inner).map(|k| c.node(&format!("N{k}")))).chain(std::iter::once(0)).collect();
    let mut k = 0;
    for w in nodes.windows(2) {
        k += 1;
        let r = 100.0 * 10f64.powf(3.0 * next());
        resistor(&mut c, &format!("R{k}"), w[0], w[1], r);
    }
    for _ in 0..extra {
        let a = (next() * nodes.len() as f64) as usize % nodes.len();
        let b = (next() * nodes.len() as f64) as usize % nodes.len();
        let r = 100.0 * 10f64.powf(3.0 * next());
        if a != b {
            k += 1;
            resistor(&mut c, &format!("R{k}"), nodes[a], nodes[b], r);
        }
    }
    let out = format!("N{}", 1 + (next() * inner as f64) as usize % inner);
    (c, out)
}

/// How far the short cut is from the true extremes, as a fraction of the span of the true extremes: (low side, high side).
fn gaps(c: &Circuit, s: &WcSettings) -> (f64, f64, usize) {
    let w = worst_case(c, s).unwrap();
    let (lo, hi, n) = brute_force(c, s);
    let (lo, hi) = (lo.min(w.nominal), hi.max(w.nominal));
    let span = (hi - lo).max(1e-12);
    ((w.min - lo).max(0.0) / span, (hi - w.max).max(0.0) / span, n)
}

#[test]
fn random_resistor_networks_match_brute_force() {
    let (mut cases, mut missed, mut worst) = (0, 0, 0.0_f64);
    let mut worst_seed = 0;
    for seed in 1..=300u64 {
        let (c, out) = random_network(seed, 3 + (seed as usize) % 2, 3 + (seed as usize) % 3);
        let s = dc(0.3, 0.0, &out);
        let (g_lo, g_hi, _) = gaps(&c, &s);
        let gap = g_lo.max(g_hi);
        cases += 1;
        if gap > 1e-9 {
            missed += 1;
        }
        if gap > worst {
            (worst, worst_seed) = (gap, seed);
        }
    }
    eprintln!("random networks: {cases} cases, the short cut missed the true extreme in {missed}; largest miss {worst:.4} of the span (seed {worst_seed})");
    // The short cut (each part alone, then one round of flips) is not exact: with wide tolerances it can miss a corner. It is
    // rare and small, and this keeps it so (measured: +-5 % none in 2000 networks, +-30 % 0.1 % of them by 0.12 % of the span,
    // +-60 % 1.1 % of them by up to 1 %; see the ignored stress test).
    assert!(missed * 25 <= cases && worst < 0.005, "the short cut got worse: missed {missed} of {cases}, largest {worst}");
}

/// With tolerances a design really has, the short cut is exact.
#[test]
fn ordinary_tolerances_are_exact() {
    for seed in 1..=300u64 {
        let (c, out) = random_network(seed * 31, 4, 4);
        let (g_lo, g_hi, _) = gaps(&c, &dc(0.05, 0.0, &out));
        assert!(g_lo.max(g_hi) < 1e-9, "seed {seed}: {g_lo} {g_hi}");
    }
}

/// A larger run of the same comparison (bigger networks, wider tolerances, more of them). Run it with
/// `cargo test -p sim-core stress_random -- --ignored --nocapture` after changing `worstcase.rs`.
#[test]
#[ignore]
fn stress_random_resistor_networks() {
    for (tol, inner, extra, seeds) in [(0.05, 4, 4, 2000u64), (0.3, 4, 5, 2000), (0.6, 5, 6, 1000)] {
        let (mut missed, mut worst, mut worst_seed) = (0, 0.0_f64, 0);
        for seed in 1..=seeds {
            let (c, out) = random_network(seed * 7919, inner, extra);
            let (g_lo, g_hi, _) = gaps(&c, &dc(tol, 0.0, &out));
            let gap = g_lo.max(g_hi);
            if gap > 1e-9 {
                missed += 1;
            }
            if gap > worst {
                (worst, worst_seed) = (gap, seed);
            }
        }
        eprintln!("tol {tol}: {seeds} networks ({inner}+ nodes, {extra} extra): missed {missed}, largest miss {worst:.5} of the span (seed {worst_seed})");
    }
}
