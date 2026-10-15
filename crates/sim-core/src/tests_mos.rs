use crate::spice_in::Model;
use crate::*;

use crate::dc::VT;

fn vs(c: &mut Circuit, name: &str, p: &str, n: &str, dc: f64) {
    let (p, n) = (c.node(p), c.node(n));
    c.add(Element::vdc(name, p, n, dc));
}

fn res(c: &mut Circuit, name: &str, a: &str, b: &str, ohms: f64) {
    let (a, b) = (c.node(a), c.node(b));
    c.add(Element::Resistor { name: name.into(), a, b, r: ohms });
}

fn mos(c: &mut Circuit, name: &str, d: &str, g: &str, s: &str, m: MosModel) {
    let (d, g, s) = (c.node(d), c.node(g), c.node(s));
    c.add_mosfet(name, d, g, s, m);
}

fn nmos(kp: f64, vto: f64, lambda: f64) -> MosModel {
    MosModel { kp, vto, lambda, ..MosModel::default() }
}

fn pmos(kp: f64, vto: f64, lambda: f64) -> MosModel {
    MosModel { kp, vto, lambda, pmos: true, ..MosModel::default() }
}

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol.max(2e-8 * b.abs()), "{a} != {b}");
}

fn assert_power_balance(res: &DcResult) {
    let total: f64 = res.elements.iter().map(|e| e.power).sum();
    let scale: f64 = res.elements.iter().map(|e| e.power.abs()).sum::<f64>().max(1e-3);
    assert!(total.abs() < 1e-7 * scale, "power imbalance {total} (scale {scale})");
}

/// Solve  kp*(vov*v - v^2/2) = (vdd - v)/rd  for the triode drain voltage.
fn triode_oracle(kp: f64, vov: f64, vdd: f64, rd: f64) -> f64 {
    let f = |v: f64| kp * (vov * v - v * v / 2.0) - (vdd - v) / rd;
    let (mut lo, mut hi) = (0.0, vov.min(vdd));
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if f(mid) < 0.0 { lo = mid } else { hi = mid }
    }
    0.5 * (lo + hi)
}

#[test]
fn nmos_as_a_switch_in_the_triode_region() {
    let (kp, vto) = (0.1, 2.0);
    let mut c = Circuit::new();
    vs(&mut c, "Vdd", "vdd", "0", 5.0);
    vs(&mut c, "Vg", "g", "0", 5.0);
    res(&mut c, "Rd", "vdd", "d", 1000.0);
    mos(&mut c, "M1", "d", "g", "0", nmos(kp, vto, 0.0));
    let r = dc_op(&c).unwrap();
    let want = triode_oracle(kp, 5.0 - vto, 5.0, 1000.0);
    close(r.voltage("d").unwrap(), want, 1e-6);
    let m = r.element("M1").unwrap();
    close(m.current, (5.0 - want) / 1000.0, 1e-9);
    assert!(m.voltage < 3.0 && m.voltage < 5.0 - vto, "triode: vds below the overdrive");
    assert_power_balance(&r);
}

#[test]
fn nmos_in_saturation_with_channel_length_modulation() {
    let (kp, vto, lambda) = (1e-3, 2.0, 0.02);
    let mut c = Circuit::new();
    vs(&mut c, "Vdd", "vdd", "0", 5.0);
    vs(&mut c, "Vg", "g", "0", 3.0);
    res(&mut c, "Rd", "vdd", "d", 1000.0);
    mos(&mut c, "M1", "d", "g", "0", nmos(kp, vto, lambda));
    let r = dc_op(&c).unwrap();
    // Id = kp/2 * vov^2 * (1 + lambda*vds) = (5 - vds)/1k  =>  linear in vds
    let id0 = 0.5 * kp;
    let vds = (5.0 - 1000.0 * id0) / (1.0 + 1000.0 * id0 * lambda);
    close(r.voltage("d").unwrap(), vds, 1e-6);
    close(r.element("M1").unwrap().current, id0 * (1.0 + lambda * vds), 1e-9);
    assert!(vds > 1.0, "saturation needs vds > vov");
    assert_power_balance(&r);
}

#[test]
fn nmos_is_off_below_threshold() {
    let mut c = Circuit::new();
    vs(&mut c, "Vdd", "vdd", "0", 5.0);
    vs(&mut c, "Vg", "g", "0", 1.5);
    res(&mut c, "Rd", "vdd", "d", 1000.0);
    mos(&mut c, "M1", "d", "g", "0", nmos(0.1, 2.0, 0.0));
    let r = dc_op(&c).unwrap();
    close(r.voltage("d").unwrap(), 5.0, 1e-6);
    assert!(r.element("M1").unwrap().current.abs() < 1e-9);
}

#[test]
fn pmos_high_side_switch_mirrors_the_nmos() {
    // source at +5, gate at 0 (vgs = -5), load to ground
    let (kp, vto) = (0.1, 2.0);
    let mut c = Circuit::new();
    vs(&mut c, "Vdd", "vdd", "0", 5.0);
    vs(&mut c, "Vg", "g", "0", 0.0);
    res(&mut c, "Rl", "d", "0", 1000.0);
    mos(&mut c, "M1", "d", "g", "vdd", pmos(kp, -vto, 0.0));
    let r = dc_op(&c).unwrap();
    let vout = r.voltage("d").unwrap();
    close(vout, 5.0 - triode_oracle(kp, 5.0 - vto, 5.0, 1000.0), 1e-6);
    // the current flows out of the drain (negative "into the drain")
    assert!(r.element("M1").unwrap().current < 0.0);
    assert_power_balance(&r);
    // gate high: off, output at ground
    let mut off = c.clone();
    off.elements[1] = Element::vdc("Vg", off.node("g"), 0, 5.0);
    close(dc_op(&off).unwrap().voltage("d").unwrap(), 0.0, 1e-6);
}

#[test]
fn cmos_inverter_transfer() {
    let build = |vin: f64| {
        let mut c = Circuit::new();
        vs(&mut c, "Vdd", "vdd", "0", 5.0);
        vs(&mut c, "Vin", "in", "0", vin);
        mos(&mut c, "Mn", "out", "in", "0", nmos(1e-3, 1.0, 0.01));
        mos(&mut c, "Mp", "out", "in", "vdd", pmos(1e-3, -1.0, 0.01));
        dc_op(&c).unwrap().voltage("out").unwrap()
    };
    assert!((build(0.0) - 5.0).abs() < 1e-3);
    assert!(build(5.0).abs() < 1e-3);
    // symmetric devices: the switching point is half the supply
    assert!((build(2.5) - 2.5).abs() < 0.15, "{}", build(2.5));
    // monotonic falling transfer curve
    let curve: Vec<f64> = (0..=10).map(|i| build(i as f64 * 0.5)).collect();
    assert!(curve.windows(2).all(|w| w[1] <= w[0] + 1e-9), "{curve:?}");
}

#[test]
fn the_device_is_symmetric_source_and_drain_can_swap() {
    // "drain" at 0 V, "source" at 2 V, gate at 5 V: current flows from the source terminal
    let mut c = Circuit::new();
    vs(&mut c, "Vs", "s", "0", 2.0);
    vs(&mut c, "Vg", "g", "0", 5.0);
    res(&mut c, "Rl", "d", "0", 100.0);
    mos(&mut c, "M1", "d", "g", "s", nmos(0.01, 1.0, 0.0));
    let r = dc_op(&c).unwrap();
    assert!(r.element("M1").unwrap().current < 0.0, "reverse current");
    assert!(r.voltage("d").unwrap() > 0.0);
    assert_power_balance(&r);
}

#[test]
fn current_mirror_copies_the_reference() {
    let mut c = Circuit::new();
    vs(&mut c, "Vdd", "vdd", "0", 5.0);
    res(&mut c, "Rref", "vdd", "g", 10e3);
    mos(&mut c, "M1", "g", "g", "0", nmos(1e-3, 1.0, 0.0)); // diode-connected
    res(&mut c, "Rl", "vdd", "d", 5e3);
    mos(&mut c, "M2", "d", "g", "0", nmos(1e-3, 1.0, 0.0));
    let r = dc_op(&c).unwrap();
    let (i1, i2) = (r.element("M1").unwrap().current, r.element("M2").unwrap().current);
    close(i2, i1, 1e-7);
    assert_power_balance(&r);
}

#[test]
fn common_source_amplifier_gain_is_minus_gm_rd() {
    let (kp, vto, rd) = (2e-3, 1.0, 2000.0);
    let mut c = Circuit::new();
    vs(&mut c, "Vdd", "vdd", "0", 10.0);
    // gate bias 2 V plus a 1 V AC drive
    let src = parse_source_full("2 AC 1").unwrap();
    let (g, gnd) = (c.node("g"), 0);
    c.add(Element::VSource { name: "Vg".into(), p: g, n: gnd, wave: src.wave, ac: src.ac });
    res(&mut c, "Rd", "vdd", "d", rd);
    mos(&mut c, "M1", "d", "g", "0", nmos(kp, vto, 0.0));
    let dc = dc_op(&c).unwrap();
    // saturated: gm = kp * vov
    let gm = kp * (2.0 - vto);
    close(dc.element("M1").unwrap().current, 0.5 * kp * (2.0 - vto).powi(2), 1e-9);
    let ac = ac_analysis(&c, &AcSettings { fstart: 1e3, fstop: 1e3, sweep: AcSweep::Decade(1) }).unwrap();
    let s = ac.signal("V(d)").unwrap();
    let mag = s.re[0].hypot(s.im[0]);
    assert!((mag - gm * rd).abs() / (gm * rd) < 1e-4, "{mag} vs {}", gm * rd);
    assert!(s.re[0] < 0.0, "inverting");
}

#[test]
fn gate_capacitance_makes_a_pole() {
    let mut c = Circuit::new();
    let src = parse_source_full("0 AC 1").unwrap();
    let a = c.node("in");
    c.add(Element::VSource { name: "V1".into(), p: a, n: 0, wave: src.wave, ac: src.ac });
    res(&mut c, "Rg", "in", "g", 1000.0);
    mos(&mut c, "M1", "d", "g", "0", MosModel { kp: 1e-3, vto: 1.0, cgs: 1e-9, ..MosModel::default() });
    res(&mut c, "Rd", "d", "0", 1e6); // keep the drain defined
    assert!(c.elements.iter().any(|e| e.name() == "M1.CGS"));
    let r = ac_analysis(&c, &AcSettings { fstart: 159_155.0, fstop: 159_155.0, sweep: AcSweep::Decade(1) }).unwrap();
    let s = r.signal("V(g)").unwrap();
    let mag = s.re[0].hypot(s.im[0]);
    assert!((mag - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.002, "-3 dB at 1/(2 pi R Cgs): {mag}");
}

// ---------------------------------------------------------------- model cards and library parts

#[test]
fn model_cards_for_mosfets_and_zeners() {
    let m = parse_models(".model NM NMOS(VTO=2.1 KP=0.12 LAMBDA=0.01 CGS=20p CGD=5p)\n.model PM PMOS(VTO=-1.5 KP=0.05)\n.model DZ D(IS=1n BV=5.1 IBV=10m)").unwrap();
    match &m["nm"] {
        Model::Mos(mm) => assert_eq!((mm.kp, mm.vto, mm.lambda, mm.cgs, mm.cgd, mm.pmos), (0.12, 2.1, 0.01, 20e-12, 5e-12, false)),
        _ => panic!(),
    }
    match &m["pm"] {
        Model::Mos(mm) => assert!(mm.pmos && mm.vto == -1.5),
        _ => panic!(),
    }
    match &m["dz"] {
        Model::Diode(d) => assert!(d.bv == 5.1 && d.ibv == 0.01),
        _ => panic!(),
    }
}

/// Reverse current of the diode model at `v` volts reverse bias.
fn zener_current(m: &DiodeModel, vd: f64) -> f64 {
    let nvt = m.n * VT;
    m.is * ((vd / nvt).exp() - 1.0) - m.ibv * (-(vd + m.bv) / nvt).exp()
}

#[test]
fn zener_regulates_a_reverse_biased_supply() {
    let m = DiodeModel { is: 1e-9, n: 1.0, rs: 0.0, bv: 5.1, ibv: 0.02, ..DiodeModel::default() };
    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", 12.0);
    res(&mut c, "R1", "in", "z", 470.0);
    // cathode at z, anode at ground: reverse biased
    let z = c.node("z");
    c.add(Element::Diode { name: "DZ".into(), a: 0, k: z, model: m.clone() });
    let r = dc_op(&c).unwrap();
    let vz = r.voltage("z").unwrap();
    assert!(vz > 5.0 && vz < 5.3, "clamped near BV: {vz}");
    // KCL against the diode equation
    let iz = (12.0 - vz) / 470.0;
    close(-zener_current(&m, -vz), iz, 1e-6);
    assert_power_balance(&r);
    // the clamp holds when the supply moves: line regulation
    let vz2 = {
        let mut c2 = c.clone();
        c2.elements[0] = Element::vdc("V1", c2.node("in"), 0, 15.0);
        dc_op(&c2).unwrap().voltage("z").unwrap()
    };
    assert!((vz2 - vz).abs() < 0.03, "{vz} -> {vz2}");
    // below the breakdown voltage it blocks
    let mut low = c.clone();
    low.elements[0] = Element::vdc("V1", low.node("in"), 0, 3.0);
    close(dc_op(&low).unwrap().voltage("z").unwrap(), 3.0, 1e-4);
}

#[test]
fn zener_small_signal_resistance() {
    let m = DiodeModel { is: 1e-9, n: 1.0, rs: 2.0, bv: 5.1, ibv: 0.02, ..DiodeModel::default() };
    let mut c = Circuit::new();
    let s = parse_source_full("12 AC 1").unwrap();
    let a = c.node("in");
    c.add(Element::VSource { name: "V1".into(), p: a, n: 0, wave: s.wave, ac: s.ac });
    res(&mut c, "R1", "in", "z", 470.0);
    let z = c.node("z");
    c.add(Element::Diode { name: "DZ".into(), a: 0, k: z, model: m.clone() });
    let dc = dc_op(&c).unwrap();
    let iz = dc.element("DZ").unwrap().current.abs();
    let rz = VT / (iz + 1e-30) + 2.0; // dynamic resistance in breakdown + series resistance
    let ac = ac_analysis(&c, &AcSettings { fstart: 1e3, fstop: 1e3, sweep: AcSweep::Decade(1) }).unwrap();
    let h = ac.signal("V(z)").unwrap();
    let got = h.re[0].hypot(h.im[0]);
    let want = rz / (470.0 + rz);
    assert!((got - want).abs() / want < 0.05, "{got} vs {want}");
}

#[test]
fn zener_in_a_clipping_circuit_converges_from_a_cold_start() {
    // a hard case: large overdrive into a low-voltage zener through a small resistor
    let m = DiodeModel { is: 1e-12, n: 1.0, rs: 0.5, bv: 3.3, ibv: 0.005, ..DiodeModel::default() };
    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", 24.0);
    res(&mut c, "R1", "in", "z", 100.0);
    let z = c.node("z");
    c.add(Element::Diode { name: "DZ".into(), a: 0, k: z, model: m });
    let r = dc_op(&c).unwrap();
    let vz = r.voltage("z").unwrap();
    assert!(vz > 3.2 && vz < 4.5, "{vz}");
    assert_power_balance(&r);
}
