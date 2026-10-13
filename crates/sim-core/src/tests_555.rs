//! The NE555 library part: a behavioural model made of voltage-controlled switches.

use crate::*;

/// The `.subckt` text of the bundled part.
fn ne555_text() -> String {
    let part: serde_json::Value = serde_json::from_str(include_str!("../../part-lib/examples/NE555_DIP-8.part.json")).unwrap();
    part["sim"]["text"].as_str().unwrap().to_string()
}

struct Ports {
    trig: usize,
    thres: usize,
    ctl: usize,
    rst: usize,
    out: usize,
    disch: usize,
    vcc: usize,
}

fn add_555(c: &mut Circuit) -> Ports {
    let names = ["trig", "thres", "ctl", "rst", "out", "disch", "vcc"];
    let n: Vec<usize> = names.iter().map(|n| c.node(n)).collect();
    let mut nodes = n.clone();
    nodes.push(0); // gnd
    add_subckt_instance(c, "U1", "NE555", &ne555_text(), &nodes).unwrap();
    Ports { trig: n[0], thres: n[1], ctl: n[2], rst: n[3], out: n[4], disch: n[5], vcc: n[6] }
}

fn r(c: &mut Circuit, name: &str, a: usize, b: usize, ohms: f64) {
    c.add(Element::Resistor { name: name.into(), a, b, r: ohms });
}

fn cap(c: &mut Circuit, name: &str, a: usize, b: usize, f: f64) {
    c.add(Element::Capacitor { name: name.into(), a, b, c: f });
}

/// Classic astable: R1 from Vcc to DISCH, R2 from DISCH to THRES = TRIG, C to ground.
fn astable(vcc: f64, r1: f64, r2: f64, cf: f64) -> Circuit {
    let mut c = Circuit::new();
    let p = add_555(&mut c);
    c.add(Element::vdc("Vcc", p.vcc, 0, vcc));
    r(&mut c, "R1", p.vcc, p.disch, r1);
    r(&mut c, "R2", p.disch, p.thres, r2);
    r(&mut c, "Rt", p.thres, p.trig, 1.0); // THRES and TRIG tied together
    cap(&mut c, "C1", p.thres, 0, cf);
    cap(&mut c, "Cc", p.ctl, 0, 10e-9);
    r(&mut c, "Rr", p.rst, p.vcc, 1.0);
    r(&mut c, "Rl", p.out, 0, 10e3);
    c
}

/// Times at which `name` crosses `level` upwards (linear interpolation) and downwards.
fn crossings(r: &TranResult, name: &str, level: f64) -> (Vec<f64>, Vec<f64>) {
    let s = r.signal(name).unwrap();
    let (mut up, mut down) = (vec![], vec![]);
    for k in 1..s.values.len() {
        let (a, b) = (s.values[k - 1], s.values[k]);
        if (a < level) != (b < level) {
            let t = r.time[k - 1] + (level - a) / (b - a) * (r.time[k] - r.time[k - 1]);
            if b > a { up.push(t) } else { down.push(t) }
        }
    }
    (up, down)
}

fn run(c: &Circuit, tstop: f64, tstep: f64) -> TranResult {
    transient(c, &TranSettings { tstop, tstep, uic: false }).expect("the 555 must simulate")
}

#[test]
fn astable_timing_follows_the_datasheet_formulas() {
    let (r1, r2, cf) = (1e3, 68e3, 10e-6);
    let c = astable(5.0, r1, r2, cf);
    let r = run(&c, 6.0, 2e-3);
    let (up, down) = crossings(&r, "V(out)", 1.5);
    assert!(up.len() >= 5 && down.len() >= 5, "{} rising / {} falling edges", up.len(), down.len());
    // period 0.693 (R1 + 2 R2) C, high time 0.693 (R1 + R2) C
    let period = (up[up.len() - 1] - up[1]) / (up.len() - 2) as f64;
    let want = std::f64::consts::LN_2 * (r1 + 2.0 * r2) * cf;
    assert!((period / want - 1.0).abs() < 0.03, "period {period} vs {want}");
    let high = down.iter().find(|d| **d > up[1]).unwrap() - up[1];
    let want_high = std::f64::consts::LN_2 * (r1 + r2) * cf;
    assert!((high / want_high - 1.0).abs() < 0.04, "high {high} vs {want_high}");
    // the timing capacitor swings between 1/3 and 2/3 of the supply
    let v = r.signal("V(thres)").unwrap();
    let t0 = up[1];
    let late: Vec<f64> = r.time.iter().zip(&v.values).filter(|(t, _)| **t > t0).map(|(_, v)| *v).collect();
    let (lo, hi) = late.iter().fold((9.0f64, 0.0f64), |(l, h), x| (l.min(*x), h.max(*x)));
    assert!((lo - 5.0 / 3.0).abs() < 0.08 && (hi - 10.0 / 3.0).abs() < 0.08, "{lo} {hi}");
}

#[test]
fn output_levels_and_the_supply_current() {
    let c = astable(5.0, 1e3, 68e3, 10e-6);
    let r = run(&c, 3.0, 2e-3);
    let out = r.signal("V(out)").unwrap();
    let (lo, hi) = out.values.iter().fold((9.0f64, -9.0f64), |(l, h), v| (l.min(*v), h.max(*v)));
    // high is a bipolar totem pole: about Vcc - 1.7 V; low is close to ground
    assert!((hi - 3.3).abs() < 0.1, "high {hi}");
    assert!(lo < 0.05, "low {lo}");
    // the supply delivers a few milliamps of quiescent current
    let i = r.signal("I(Vcc)").unwrap();
    assert!(i.values.iter().any(|x| x.abs() > 2e-3));
}

#[test]
fn works_at_other_supply_voltages_with_the_same_timing() {
    // the thresholds scale with the supply, so the period does not depend on it
    for vcc in [5.0, 9.0, 12.0] {
        let c = astable(vcc, 1e3, 68e3, 10e-6);
        let r = run(&c, 5.0, 2e-3);
        let (up, _) = crossings(&r, "V(out)", vcc / 2.0 - 0.5);
        assert!(up.len() >= 4, "vcc {vcc}: {} edges", up.len());
        let period = (up[up.len() - 1] - up[1]) / (up.len() - 2) as f64;
        assert!((period / 0.949 - 1.0).abs() < 0.05, "vcc {vcc}: period {period}");
    }
}

#[test]
fn monostable_makes_one_pulse_of_1_1_rc() {
    // R from Vcc to THRES/DISCH, C to ground; a low pulse on TRIG starts it
    let (rt, ct) = (100e3, 1e-6);
    let mut c = Circuit::new();
    let p = add_555(&mut c);
    c.add(Element::vdc("Vcc", p.vcc, 0, 5.0));
    r(&mut c, "Rt", p.vcc, p.thres, rt);
    r(&mut c, "Rd", p.thres, p.disch, 1.0);
    cap(&mut c, "Ct", p.thres, 0, ct);
    cap(&mut c, "Cc", p.ctl, 0, 10e-9);
    r(&mut c, "Rr", p.rst, p.vcc, 1.0);
    r(&mut c, "Rl", p.out, 0, 10e3);
    // trigger idles high through a resistor and gets one 10 ms low pulse at 0.2 s
    let pulse = parse_source_full("PULSE(5 0 0.2 1m 1m 10m 10)").unwrap();
    c.add(Element::VSource { name: "Vtrig".into(), p: p.trig, n: 0, wave: pulse.wave, ac: pulse.ac });
    let r = run(&c, 1.0, 1e-3);
    let (up, down) = crossings(&r, "V(out)", 1.5);
    assert_eq!(up.len(), 1, "one pulse: {up:?}");
    assert_eq!(down.len(), 1, "{down:?}");
    let width = down[0] - up[0];
    assert!((width / (1.0986 * rt * ct) - 1.0).abs() < 0.04, "width {width} vs {}", 1.0986 * rt * ct);
}

#[test]
fn reset_forces_the_output_low() {
    let mut c = astable(5.0, 1e3, 68e3, 10e-6);
    // replace the reset tie by a source that pulls RESET low from 1.0 s on
    let rst = c.node("rst");
    c.elements.retain(|e| e.name() != "Rr");
    let s = parse_source_full("PULSE(5 0 1.0 1m 1m 100 200)").unwrap();
    c.add(Element::VSource { name: "Vrst".into(), p: rst, n: 0, wave: s.wave, ac: s.ac });
    let r = run(&c, 3.0, 2e-3);
    let out = r.signal("V(out)").unwrap();
    let after: Vec<f64> = r.time.iter().zip(&out.values).filter(|(t, _)| **t > 1.05).map(|(_, v)| *v).collect();
    assert!(after.iter().all(|v| *v < 0.1), "stays low while RESET is low");
    let before_high = r.time.iter().zip(&out.values).any(|(t, v)| *t < 1.0 && *v > 3.0);
    assert!(before_high);
}

#[test]
fn the_control_pin_moves_the_thresholds() {
    // forcing CONT to 4 V makes the capacitor swing 4/2 .. 4 V: a longer period than at 3.33 V
    let mut c = astable(6.0, 1e3, 68e3, 10e-6);
    let ctl = c.node("ctl");
    c.add(Element::vdc("Vctl", ctl, 0, 4.0));
    let r = run(&c, 6.0, 2e-3);
    let (up, _) = crossings(&r, "V(out)", 1.5);
    assert!(up.len() >= 3);
    let period = (up[up.len() - 1] - up[1]) / (up.len() - 2) as f64;
    // charge 2 -> 4 V and discharge 4 -> 2 V towards 0 / 6 V:  t = RC ln((6-2)/(6-4)) etc.
    let (r1, r2, cf) = (1e3, 68e3, 10e-6);
    let t_high = (r1 + r2) * cf * ((6.0f64 - 2.0) / (6.0 - 4.0)).ln();
    let t_low = r2 * cf * (4.0f64 / 2.0).ln();
    assert!((period / (t_high + t_low) - 1.0).abs() < 0.05, "{period} vs {}", t_high + t_low);
}

#[test]
fn ne555_blinker_example_blinks_about_once_a_second() {
    let sch: sch_core::Schematic = serde_json::from_str(include_str!("../../../docs/examples/ne555_blinker.schjson")).unwrap();
    let c = from_schematic(&sch).unwrap();
    let r = run(&c, 4.0, 2e-3);
    let (up, _) = crossings(&r, "V(OUT)", 1.5);
    assert!(up.len() >= 4, "{} rising edges", up.len());
    // the first cycle starts from an empty capacitor and is longer: measure from the second edge
    let period = (up[up.len() - 1] - up[1]) / (up.len() - 2) as f64;
    assert!((period - 0.95).abs() < 0.05, "period {period}");
    // the LED lights for about half of the period with a few milliamps
    let i = r.signal("I(D1)").unwrap();
    let peak = i.values.iter().fold(0.0f64, |a, b| a.max(*b));
    assert!(peak > 2e-3 && peak < 8e-3, "LED current {peak}");
    assert!(r.warnings.iter().any(|w| w.contains("ゼロ")), "start-from-zero notice: {:?}", r.warnings);
}

#[test]
fn the_dc_panel_finds_a_point_for_the_latch() {
    // the 555 is bistable: plain Newton, gmin and source stepping all cycle. `dc_op` must still
    // return a point (the transient keeps starting from zero charge, tested above).
    let c = astable(5.0, 1e3, 68e3, 10e-6);
    let r = dc_op(&c).expect("the DC panel must find an operating point");
    assert_eq!(r.strategy, "switch widening");
    assert!((r.voltage("vcc").unwrap() - 5.0).abs() < 1e-6);
    assert!(r.voltages.iter().all(|(_, v)| v.is_finite() && v.abs() < 5.5));
}
