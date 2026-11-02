//! The loop gain of feedback circuits built on the bundled op-amp models. Two checks that do not use the loop gain code to
//! get the answer:
//!
//! 1. The closed-loop response from the plain AC analysis must follow from the loop gain: H = (H_inf T + H_0) / (1 + T), where
//!    H_inf is the gain of an ideal amplifier (1, 1 + Rf/Rg, -Rf/Ri) and H_0 the response when the loop is dead (the same
//!    circuit with the gain of the model's first stage set to zero, solved by the AC analysis). It holds for any single-loop circuit, so it
//!    tests `loop_gain` on the real models, with and without a capacitive load.
//! 2. The LM358 model is one pole after a gain, a resistor and an output resistor, so its loop gain as a follower is worked
//!    out by hand (also with a load capacitor and a feedback resistor, where the two sides of the cut differ a lot).

use crate::ac::{AcSettings, AcSweep, C64, ac_analysis};
use crate::circuit::{Circuit, Element};
use crate::loopgain::{BreakDirection, loop_gain};
use crate::tests_parts::{part, place, res, vs};
use crate::waveform::Waveform;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug)]
enum Topology {
    Follower,
    /// Gain 10: Rf 9 k from the output, Rg 1 k to the reference.
    NonInverting,
    /// Gain -10: Ri 1 k from the input, Rf 10 k from the output.
    Inverting,
}

impl Topology {
    fn h_inf(self) -> f64 {
        match self {
            Topology::Follower => 1.0,
            Topology::NonInverting => 10.0,
            Topology::Inverting => -10.0,
        }
    }
}

/// (positive supply, negative supply, the DC level the signal sits on).
fn supply(name: &str) -> (f64, f64, f64) {
    match name {
        "TL072" | "NE5532" => (15.0, -15.0, 0.0),
        _ => (5.0, 0.0, 2.5),
    }
}

/// An amplifier on a bundled op-amp; the feedback resistor is `RF` (the loop is cut there). `cload` is a capacitor on the output.
fn amplifier(name: &str, topology: Topology, cload: f64) -> Circuit {
    let (vp, vn, bias) = supply(name);
    let mut c = Circuit::new();
    vs(&mut c, "Vp", "vp", "0", vp);
    vs(&mut c, "Vn", "vn", "0", vn);
    vs(&mut c, "Vref", "ref", "0", bias);
    let vin = c.node("in");
    c.add(Element::VSource { name: "Vin".into(), p: vin, n: 0, wave: Waveform::Dc(bias), ac: Some((1.0, 0.0)) });
    let plus = match topology {
        Topology::Inverting => "ref",
        _ => "in",
    };
    match topology {
        Topology::Follower => res(&mut c, "RF", "out", "fb", 1e3),
        Topology::NonInverting => {
            res(&mut c, "RF", "out", "fb", 9e3);
            res(&mut c, "RG", "fb", "ref", 1e3);
        }
        Topology::Inverting => {
            res(&mut c, "RI", "in", "fb", 1e3);
            res(&mut c, "RF", "out", "fb", 10e3);
        }
    }
    res(&mut c, "RL", "out", "ref", 10e3);
    if cload > 0.0 {
        let out = c.node("out");
        c.add(Element::Capacitor { name: "CL".into(), a: out, b: 0, c: cload });
    }
    // the model's pins are the package pin numbers, in the order of the subcircuit: inp inn out vp vn
    let def = part(name);
    let pins = &def.sim.as_ref().unwrap().pins;
    assert_eq!(pins.len(), 5, "{name}");
    place(&mut c, &def, "U1", &[(pins[0].as_str(), plus), (pins[1].as_str(), "fb"), (pins[2].as_str(), "out"), (pins[3].as_str(), "vp"), (pins[4].as_str(), "vn")]);
    c
}

fn sweep() -> AcSettings {
    AcSettings { fstart: 0.1, fstop: 1e10, sweep: AcSweep::Decade(20) }
}

fn cx(sig: &crate::ac::AcSignal) -> Vec<C64> {
    sig.re.iter().zip(&sig.im).map(|(re, im)| C64::new(*re, *im)).collect()
}

fn dist(a: C64, b: C64) -> f64 {
    C64::new(a.re - b.re, a.im - b.im).abs()
}

/// How far the closed-loop response of the AC analysis is from the one the loop gain predicts, as a fraction of the ideal gain,
/// and the phase margin and the crossover that the loop gain gives. `cload` is the load capacitor.
fn consistency(name: &str, topology: Topology, cload: f64) -> (f64, Option<f64>, Option<f64>) {
    let c = amplifier(name, topology, cload);
    let r = ac_analysis(&c, &sweep()).unwrap();
    let h = cx(r.signal("V(out)").unwrap());
    let lg = loop_gain(&c, "RF", BreakDirection::Auto, &sweep()).unwrap();
    let h_inf = C64::new(topology.h_inf(), 0.0);
    // H_0: the response with the loop dead, from the same circuit with the gain of the first stage of the model set to zero
    // (the load capacitor and the resistors still carry the input to the output, at every frequency)
    let mut dead = c.clone();
    let eop = dead.elements.iter().position(|e| matches!(e, Element::Vcvs { name, .. } if name.ends_with("Eop"))).expect("the model's first stage Eop");
    if let Element::Vcvs { gain, .. } = &mut dead.elements[eop] {
        *gain = 0.0;
    }
    let h0 = cx(ac_analysis(&dead, &sweep()).unwrap().signal("V(out)").unwrap());
    let one = C64::new(1.0, 0.0);
    let mut worst = 0.0_f64;
    for (k, t) in lg.t.iter().enumerate() {
        let predicted = (h_inf * *t + h0[k]) / (one + *t);
        worst = worst.max(dist(h[k], predicted) / h_inf.abs());
    }
    let m = crate::measure::stability_margins(&lg.freq, &lg.db(), &lg.phase_deg());
    (worst, m.as_ref().and_then(|m| m.phase_margin), m.as_ref().and_then(|m| m.gain_crossover))
}

#[test]
fn the_loop_gain_agrees_with_the_closed_loop_response_on_every_bundled_op_amp() {
    let mut worst = 0.0_f64;
    for name in ["LM324", "LM358", "MCP6001", "MCP6002", "NE5532", "TL072"] {
        for topology in [Topology::Follower, Topology::NonInverting, Topology::Inverting] {
            for cload in [0.0, 10e-9] {
                let (err, pm, fc) = consistency(name, topology, cload);
                eprintln!("{name:8} {topology:?} cload {cload:e}: mismatch {err:.2e}  crossover {fc:?}  phase margin {pm:?}");
                worst = worst.max(err);
                assert!(err < 1e-6, "{name} {topology:?} cload {cload}: the closed loop and the loop gain disagree by {err:e}");
            }
        }
    }
    eprintln!("largest mismatch {worst:e}");
}

/// The LM358 model at small signal: A(s) = 1e5 / (1 + s Rp Cp) from the differential input (2 Meg between the inputs), through
/// the output resistor of 50 ohm. A follower with a feedback resistor `rf` from the output to the inverting input and a load
/// capacitor `cl` (and a 10 k load to the reference): the output node carries Ro, then cl || RL || (rf + Rin); the inverting input
/// sees Rin to the (AC grounded) non-inverting input.
fn lm358_follower_t(f: f64, rf: f64, cl: f64) -> C64 {
    let s = C64::new(0.0, 2.0 * PI * f);
    let one = C64::new(1.0, 0.0);
    let (rin, ro, rl) = (2e6, 50.0, 10e3);
    let a = C64::new(1e5, 0.0) / (one + s * C64::new(1e6 * 16e-9, 0.0));
    // load of the output: cl || rl || (rf + rin)
    let y_load = one / C64::new(rl, 0.0) + one / C64::new(rf + rin, 0.0) + s * C64::new(cl, 0.0);
    let z_load = one / y_load;
    let to_out = z_load / (z_load + C64::new(ro, 0.0));
    // from the output to the inverting input, a divider of rf and rin
    let to_inn = C64::new(rin, 0.0) / C64::new(rin + rf, 0.0);
    a * to_out * to_inn
}

#[test]
fn the_lm358_follower_loop_gain_is_exact() {
    let mut worst = 0.0_f64;
    for (rf, cl) in [(1e3, 0.0), (1e3, 10e-9), (10e3, 100e-9), (100.0, 1e-6), (47e3, 1e-9)] {
        let mut c = amplifier("LM358", Topology::Follower, cl);
        // the feedback resistor of this case
        let i = c.elements.iter().position(|e| e.name() == "RF").unwrap();
        if let Element::Resistor { r, .. } = &mut c.elements[i] {
            *r = rf;
        }
        let lg = loop_gain(&c, "RF", BreakDirection::Auto, &sweep()).unwrap();
        for (k, f) in lg.freq.iter().enumerate() {
            let want = lm358_follower_t(*f, rf, cl);
            worst = worst.max(dist(lg.t[k], want) / want.abs());
        }
    }
    eprintln!("LM358 follower: largest relative error of the loop gain {worst:e}");
    assert!(worst < 1e-4, "{worst:e}");
}

#[test]
fn the_bundled_op_amp_models_are_one_pole_amplifiers() {
    // All six models share one template: a gain stage (Eop), one pole (Rp, Cp) and an output resistor, so a follower with no load
    // has a phase margin of 90 degrees at its gain-bandwidth product. A real part has a second pole and a lower margin, which these
    // models cannot show; the margin they give comes from the load (the output resistor with a load capacitor) only.
    for name in ["LM324", "LM358", "MCP6001", "MCP6002", "NE5532", "TL072"] {
        let (_, pm, fc) = consistency(name, Topology::Follower, 0.0);
        let (pm, fc) = (pm.unwrap(), fc.unwrap());
        assert!((pm - 90.0).abs() < 0.01, "{name}: {pm}");
        assert!(fc > 5e5 && fc < 1.1e7, "{name}: {fc}");
        // a load capacitor puts a second pole in the loop (Ro and CL) and the margin falls
        let (_, pm_loaded, _) = consistency(name, Topology::Follower, 10e-9);
        assert!(pm_loaded.unwrap() < 35.0, "{name}: {pm_loaded:?}");
    }
}
