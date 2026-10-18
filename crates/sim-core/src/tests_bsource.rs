//! Behavioural sources (`B` elements): expressions of node voltages, branch currents and time.

use crate::*;

fn sub(text: &str, ports: &[&str]) -> Circuit {
    let mut c = Circuit::new();
    let nodes: Vec<usize> = ports.iter().map(|p| c.node(p)).collect();
    add_subckt_instance(&mut c, "X1", "T", text, &nodes).unwrap_or_else(|e| panic!("{e}"));
    c
}

fn sub_err(text: &str, ports: &[&str]) -> String {
    let mut c = Circuit::new();
    let nodes: Vec<usize> = ports.iter().map(|p| c.node(p)).collect();
    add_subckt_instance(&mut c, "X1", "T", text, &nodes).unwrap_err()
}

fn vdc(c: &mut Circuit, name: &str, node: &str, v: f64) {
    let n = c.node(node);
    c.add(Element::vdc(name, n, 0, v));
}

fn res(c: &mut Circuit, name: &str, a: &str, b: &str, r: f64) {
    let (a, b) = (c.node(a), c.node(b));
    c.add(Element::Resistor { name: name.into(), a, b, r });
}

#[test]
fn a_current_source_expression_acts_like_a_transconductance() {
    let mut c = sub(".subckt T a o\nB1 0 o I=2m*V(a)\n.ends T", &["a", "o"]);
    vdc(&mut c, "Va", "a", 1.5);
    res(&mut c, "Rl", "o", "0", 1000.0);
    // 3 mA into 1k from node 0 to o: 3 V
    let r = dc_op(&c).unwrap();
    assert!((r.voltage("o").unwrap() - 3.0).abs() < 1e-6, "{}", r.voltage("o").unwrap());
}

#[test]
fn a_voltage_source_expression_is_an_amplifier_with_an_offset() {
    let mut c = sub(".subckt T a o\nB1 o 0 V=3*V(a)+1\n.ends T", &["a", "o"]);
    vdc(&mut c, "Va", "a", 0.5);
    res(&mut c, "Rl", "o", "0", 1000.0);
    let r = dc_op(&c).unwrap();
    assert!((r.voltage("o").unwrap() - 2.5).abs() < 1e-9);
    // 2.5 mA flows out of the source's + terminal into the load: the branch current is the other sign
    assert!((r.element("X1.B1").unwrap().current + 2.5e-3).abs() < 1e-9, "{}", r.element("X1.B1").unwrap().current);
}

#[test]
fn nonlinear_expressions_converge() {
    // o = a*a: the output follows the square of the input
    let mut c = sub(".subckt T a o\nB1 o 0 V=V(a)*V(a)\n.ends T", &["a", "o"]);
    vdc(&mut c, "Va", "a", 3.0);
    res(&mut c, "Rl", "o", "0", 1000.0);
    assert!((dc_op(&c).unwrap().voltage("o").unwrap() - 9.0).abs() < 1e-6);
    // a feedback loop: o = limit(10*(1 - o), 0, 5)  =>  o = 10/11
    let mut c = sub(".subckt T o\nB1 o 0 V=limit(10*(1-V(o)), 0, 5)\n.ends T", &["o"]);
    res(&mut c, "Rl", "o", "0", 1000.0);
    let v = dc_op(&c).unwrap().voltage("o").unwrap();
    assert!((v - 10.0 / 11.0).abs() < 1e-6, "{v}");
}

#[test]
fn a_current_through_a_source_can_be_read_and_used() {
    // Vs measures the current into R; B1 makes a voltage 100 x that current
    let mut c = sub(".subckt T a o\nVs a m 0\nR1 m 0 200\nB1 o 0 V=100*I(Vs)\n.ends T", &["a", "o"]);
    vdc(&mut c, "Va", "a", 2.0);
    res(&mut c, "Rl", "o", "0", 1e4);
    // 2 V / 200 ohm = 10 mA -> 1.0 V
    let v = dc_op(&c).unwrap().voltage("o").unwrap();
    assert!((v - 1.0).abs() < 1e-6, "{v}");
}

#[test]
fn time_is_available_in_the_transient_analysis() {
    let mut c = sub(".subckt T o\nB1 o 0 V=2000*time\n.ends T", &["o"]);
    res(&mut c, "Rl", "o", "0", 1000.0);
    let r = transient(&c, &TranSettings { tstop: 1e-3, tstep: 1e-5, uic: false }).unwrap();
    let v = &r.signal("V(o)").unwrap().values;
    assert!((v.last().unwrap() - 2.0).abs() < 1e-3, "{}", v.last().unwrap());
    assert!(v[0].abs() < 1e-9);
}

#[test]
fn a_behavioural_integrator_and_the_small_signal_analysis() {
    // gain stage: o = -10 a, followed by an RC low-pass: |H| = 10 / sqrt(1 + (f/fc)^2), fc = 1/(2 pi R C)
    let mut c = sub(".subckt T a o\nB1 g 0 V=-10*V(a)\nR1 g o 1k\nC1 o 0 159n\n.ends T", &["a", "o"]);
    let a = c.node("a");
    c.add(Element::VSource { name: "Va".into(), p: a, n: 0, wave: Waveform::Dc(0.2), ac: Some((1.0, 0.0)) });
    res(&mut c, "Rl", "o", "0", 1e9);
    let r = ac_analysis(&c, &AcSettings { fstart: 100.0, fstop: 100e3, sweep: AcSweep::Decade(10) }).unwrap();
    let s = r.signals.iter().find(|s| s.name == "V(o)").unwrap();
    let mag = |k: usize| (s.re[k].powi(2) + s.im[k].powi(2)).sqrt();
    // |H| = 10 / sqrt(1 + (f / fc)^2) with fc = 1 / (2 pi R C) at every frequency of the sweep
    let fc = 1.0 / (2.0 * std::f64::consts::PI * 1e3 * 159e-9);
    for (k, f) in r.freq.iter().enumerate() {
        let want = 10.0 / (1.0 + (f / fc).powi(2)).sqrt();
        assert!((mag(k) - want).abs() < 0.02 * want + 1e-3, "{f} Hz: {} vs {want}", mag(k));
    }
}

#[test]
fn a_comparator_with_if_and_a_smooth_step() {
    let mut c = sub(".subckt T a o p\nB1 o 0 V=if(V(a) > 1.5, 5, 0)\nB2 p 0 V=5*step(V(a)-1.5, 0.05)\n.ends T", &["a", "o", "p"]);
    vdc(&mut c, "Va", "a", 2.0);
    res(&mut c, "Rl", "o", "0", 1e3);
    res(&mut c, "Rp", "p", "0", 1e3);
    let r = dc_op(&c).unwrap();
    assert!((r.voltage("o").unwrap() - 5.0).abs() < 1e-9);
    assert!((r.voltage("p").unwrap() - 5.0).abs() < 1e-9);
}

#[test]
fn mistakes_are_reported_in_plain_japanese() {
    let e = sub_err(".subckt T a o\nB1 o 0 V=3*foo(a)\n.ends T", &["a", "o"]);
    assert!(e.contains("X1.B1") && e.contains("foo"), "{e}");
    let e = sub_err(".subckt T a o\nB1 o 0 3*V(a)\n.ends T", &["a", "o"]);
    assert!(e.contains("V=式"), "{e}");
    let e = sub_err(".subckt T a o\nB1 o 0 X=3\n.ends T", &["a", "o"]);
    assert!(e.contains("V=式 (電圧) か I=式"), "{e}");
    // reading the current of a resistor is not possible
    let e = sub_err(".subckt T a o\nR1 a 0 1k\nB1 o 0 V=I(R1)\n.ends T", &["a", "o"]);
    assert!(e.contains("電流を読める素子ではありません") && e.contains("0V の電圧源"), "{e}");
}

#[test]
fn a_zero_division_does_not_break_the_solver() {
    let mut c = sub(".subckt T a o\nB1 o 0 V=1/V(a)\n.ends T", &["a", "o"]);
    vdc(&mut c, "Va", "a", 0.0);
    res(&mut c, "Rl", "o", "0", 1e3);
    // 1/0 is a mistake in the circuit; whatever comes back must not be NaN or a panic
    if let Ok(r) = dc_op(&c) {
        assert!(r.voltage("o").unwrap().is_finite());
    }
}

#[test]
fn the_current_of_a_diode_can_be_read() {
    // 5 V -> 1k -> diode -> ground; B1 copies the diode current (about 4.3 mA) as a voltage
    let mut c = sub(".subckt T a o\nR1 a m 1k\nD1 m 0 DX\nB1 o 0 V=I(D1)*1000\n.model DX D(IS=1e-14)\n.ends T", &["a", "o"]);
    vdc(&mut c, "Va", "a", 5.0);
    res(&mut c, "Rl", "o", "0", 1e6);
    let r = dc_op(&c).unwrap();
    let (vo, vm) = (r.voltage("o").unwrap(), r.voltage("X1.m").unwrap());
    assert!((vo / 1000.0 - (5.0 - vm) / 1000.0).abs() < 1e-6, "I(D1) {} vs resistor {}", vo / 1000.0, (5.0 - vm) / 1000.0);
    assert!(vo > 3.5 && vo < 4.8, "{vo}");
}
