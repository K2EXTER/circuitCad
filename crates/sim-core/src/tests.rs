use crate::*;
use sch_core::{Kind, Schematic, Wire};

use crate::dc::VT;

fn r(c: &mut Circuit, name: &str, a: &str, b: &str, ohms: f64) {
    let (a, b) = (c.node(a), c.node(b));
    c.add(Element::Resistor { name: name.into(), a, b, r: ohms });
}

fn vs(c: &mut Circuit, name: &str, p: &str, n: &str, dc: f64) {
    let (p, n) = (c.node(p), c.node(n));
    c.add(Element::vdc(name, p, n, dc));
}

fn diode(c: &mut Circuit, name: &str, a: &str, k: &str, m: DiodeModel) {
    let (a, k) = (c.node(a), c.node(k));
    c.add(Element::Diode { name: name.into(), a, k, model: m });
}

fn close(a: f64, b: f64, tol: f64) {
    // gmin (1e-12 S per node) leaks a few nA, so values are exact only to ~1e-7 relative
    let tol = tol.max(2e-8 * b.abs().max(1.0));
    assert!((a - b).abs() <= tol, "{a} != {b} (tol {tol})");
}

/// Tellegen: absorbed powers of all elements sum to zero.
fn assert_power_balance(res: &DcResult) {
    let total: f64 = res.elements.iter().map(|e| e.power).sum();
    let scale: f64 = res.elements.iter().map(|e| e.power.abs()).sum::<f64>().max(1e-3);
    assert!(total.abs() < 1e-7 * scale.max(1.0), "power imbalance {total} (scale {scale})");
}

/// Series loop  V -> R -> junction(IS, nVt)  solved by bisection.
fn diode_oracle(v: f64, r_total: f64, is: f64, nvt: f64) -> (f64, f64) {
    let f = |vj: f64| is * ((vj / nvt).exp() - 1.0) * r_total + vj - v;
    let (mut lo, mut hi) = (-1.0_f64.max(-v.abs() - 1.0), 2.0);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if f(mid) > 0.0 { hi = mid } else { lo = mid }
    }
    let vj = 0.5 * (lo + hi);
    (vj, is * ((vj / nvt).exp() - 1.0))
}

#[test]
fn voltage_divider() {
    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", 10.0);
    r(&mut c, "R1", "in", "mid", 1000.0);
    r(&mut c, "R2", "mid", "GND", 1000.0);
    let res = dc_op(&c).unwrap();
    close(res.voltage("in").unwrap(), 10.0, 1e-9);
    close(res.voltage("mid").unwrap(), 5.0, 1e-9);
    close(res.element("R1").unwrap().current, 5e-3, 1e-12);
    // the source delivers: current into its + terminal is negative
    close(res.element("V1").unwrap().current, -5e-3, 1e-12);
    close(res.element("V1").unwrap().power, -0.05, 1e-9);
    assert_eq!(res.strategy, "direct");
    assert_power_balance(&res);
}

#[test]
fn capacitor_is_open_inductor_is_short() {
    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", 5.0);
    r(&mut c, "R1", "in", "a", 1000.0);
    let (a, g) = (c.node("a"), 0);
    c.add(Element::Capacitor { name: "C1".into(), a, b: g, c: 1e-6 });
    let res = dc_op(&c).unwrap();
    close(res.voltage("a").unwrap(), 5.0, 1e-9);
    close(res.element("R1").unwrap().current, 0.0, 1e-12);

    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", 5.0);
    r(&mut c, "R1", "in", "a", 1000.0);
    let a = c.node("a");
    c.add(Element::Inductor { name: "L1".into(), a, b: 0, l: 1e-3 });
    let res = dc_op(&c).unwrap();
    close(res.voltage("a").unwrap(), 0.0, 1e-9);
    close(res.element("L1").unwrap().current, 5e-3, 1e-12);
    assert_power_balance(&res);
}

#[test]
fn current_source_into_resistor() {
    let mut c = Circuit::new();
    let a = c.node("a");
    c.add(Element::ISource { name: "I1".into(), from: 0, to: a, wave: Waveform::Dc(2e-3), ac: None });
    r(&mut c, "R1", "a", "0", 1500.0);
    let res = dc_op(&c).unwrap();
    close(res.voltage("a").unwrap(), 3.0, 1e-9);
    assert_power_balance(&res);
}

#[test]
fn diode_matches_scalar_solution() {
    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", 5.0);
    r(&mut c, "R1", "in", "a", 1000.0);
    diode(&mut c, "D1", "a", "0", DiodeModel::default());
    let res = dc_op(&c).unwrap();
    let (vj, i) = diode_oracle(5.0, 1000.0, 1e-14, VT);
    close(res.voltage("a").unwrap(), vj, 1e-6);
    close(res.element("D1").unwrap().current, i, 1e-9);
    assert_power_balance(&res);
}

#[test]
fn diode_with_series_resistance_and_emission_coefficient() {
    let m = DiodeModel { is: 1e-14, n: 2.0, rs: 5.0, ..DiodeModel::default() };
    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", 5.0);
    r(&mut c, "R1", "in", "a", 100.0);
    diode(&mut c, "D1", "a", "0", m);
    let res = dc_op(&c).unwrap();
    let (vj, i) = diode_oracle(5.0, 105.0, 1e-14, 2.0 * VT);
    close(res.element("D1").unwrap().current, i, 1e-8);
    close(res.voltage("a").unwrap(), vj + i * 5.0, 1e-6);
    assert_power_balance(&res);
}

#[test]
fn reverse_biased_diode_blocks() {
    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", -5.0);
    r(&mut c, "R1", "in", "a", 1000.0);
    diode(&mut c, "D1", "a", "0", DiodeModel::default());
    let res = dc_op(&c).unwrap();
    close(res.voltage("a").unwrap(), -5.0, 1e-6);
    assert!(res.element("D1").unwrap().current.abs() < 1e-12);
}

#[test]
fn high_voltage_into_diode_converges() {
    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", 100.0);
    r(&mut c, "R1", "in", "a", 10.0);
    diode(&mut c, "D1", "a", "0", DiodeModel::default());
    let res = dc_op(&c).unwrap();
    let vd = res.voltage("a").unwrap();
    assert!(vd > 0.6 && vd < 1.0, "{vd}");
    close(res.element("R1").unwrap().current, (100.0 - vd) / 10.0, 1e-9);
    assert_power_balance(&res);
}

#[test]
fn diode_chain_converges() {
    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", 20.0);
    r(&mut c, "R1", "in", "n0", 100.0);
    for i in 0..10 {
        let (a, k) = (format!("n{i}"), if i == 9 { "0".to_string() } else { format!("n{}", i + 1) });
        diode(&mut c, &format!("D{i}"), &a, &k, DiodeModel::default());
    }
    let res = dc_op(&c).unwrap();
    let i = res.element("R1").unwrap().current;
    assert!(i > 0.1 && i < 0.2, "{i}");
    for k in 0..10 {
        close(res.element(&format!("D{k}")).unwrap().current, i, 1e-9);
    }
    assert_power_balance(&res);
}

fn bjt_stage(rb: f64, pnp: bool) -> DcResult {
    let model = BjtModel { bf: 200.0, pnp, ..BjtModel::default() };
    let mut c = Circuit::new();
    if pnp {
        // emitter at +10 V, collector load and base resistor to ground
        vs(&mut c, "V1", "vcc", "0", 10.0);
        r(&mut c, "Rb", "b", "0", rb);
        r(&mut c, "Rc", "c", "0", 1000.0);
        let (cc, b, e) = (c.node("c"), c.node("b"), c.node("vcc"));
        c.add(Element::Bjt { name: "Q1".into(), c: cc, b, e, model });
    } else {
        vs(&mut c, "V1", "vcc", "0", 10.0);
        r(&mut c, "Rb", "vcc", "b", rb);
        r(&mut c, "Rc", "vcc", "c", 1000.0);
        let (cc, b) = (c.node("c"), c.node("b"));
        c.add(Element::Bjt { name: "Q1".into(), c: cc, b, e: 0, model });
    }
    dc_op(&c).unwrap()
}

#[test]
fn npn_forward_active() {
    let res = bjt_stage(1e6, false);
    let q = res.element("Q1").unwrap();
    let ib = q.extra.iter().find(|(n, _)| n == "Ib").unwrap().1;
    let ratio = q.current / ib;
    assert!(ratio > 190.0 && ratio < 200.5, "beta {ratio}");
    let vbe = res.voltage("b").unwrap();
    assert!(vbe > 0.5 && vbe < 0.8, "vbe {vbe}");
    close(res.voltage("c").unwrap(), 10.0 - q.current * 1000.0, 1e-9);
    // terminal currents sum to zero
    let ie = q.extra.iter().find(|(n, _)| n == "Ie").unwrap().1;
    close(q.current + ib + ie, 0.0, 1e-12);
    assert_power_balance(&res);
}

#[test]
fn npn_saturation() {
    let res = bjt_stage(10e3, false);
    let q = res.element("Q1").unwrap();
    assert!(q.voltage < 0.3, "vce {}", q.voltage);
    let ib = q.extra.iter().find(|(n, _)| n == "Ib").unwrap().1;
    assert!(q.current / ib < 100.0, "saturated: beta_forced {}", q.current / ib);
    assert_power_balance(&res);
}

#[test]
fn pnp_mirrors_npn() {
    let npn = bjt_stage(1e6, false);
    let pnp = bjt_stage(1e6, true);
    let icn = npn.element("Q1").unwrap().current;
    let icp = pnp.element("Q1").unwrap().current;
    close(icp, -icn, 1e-7); // collector current flows out of the PNP collector
    close(pnp.voltage("c").unwrap(), icn * 1000.0, 1e-4);
    assert_power_balance(&pnp);
}

#[test]
fn controlled_sources() {
    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", 2.0);
    let (o, i) = (c.node("o"), c.node("in"));
    c.add(Element::Vcvs { name: "E1".into(), p: o, n: 0, cp: i, cn: 0, gain: 3.0 });
    r(&mut c, "RL", "o", "0", 1000.0);
    let g = c.node("g");
    c.add(Element::Vccs { name: "G1".into(), p: 0, n: g, cp: i, cn: 0, gm: 1e-3 }); // 2 mA into node g
    r(&mut c, "RG", "g", "0", 1000.0);
    let res = dc_op(&c).unwrap();
    close(res.voltage("o").unwrap(), 6.0, 1e-9);
    close(res.element("E1").unwrap().current, -6e-3, 1e-9); // delivering: current enters the + terminal negatively
    close(res.voltage("g").unwrap(), 2.0, 1e-9);
    assert_power_balance(&res);
}

#[test]
fn error_cases() {
    // parallel voltage sources with different values
    let mut c = Circuit::new();
    vs(&mut c, "V1", "a", "0", 5.0);
    vs(&mut c, "V2", "a", "0", 3.0);
    assert_eq!(dc_op(&c).unwrap_err(), SimError::Singular);

    // nothing connected to ground
    let mut c = Circuit::new();
    r(&mut c, "R1", "a", "b", 1.0);
    assert_eq!(dc_op(&c).unwrap_err(), SimError::NoGround);
}

#[test]
fn floating_node_is_reported() {
    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", 5.0);
    r(&mut c, "R1", "in", "0", 1000.0);
    let (a, b) = (c.node("a"), c.node("b"));
    c.add(Element::Capacitor { name: "C1".into(), a, b, c: 1e-6 });
    let res = dc_op(&c).unwrap();
    assert!(res.warnings.iter().any(|w| w.contains('a')));
    assert_eq!(res.warnings.len(), 2);
}

// ---------------------------------------------------------------- schematic level

fn wire(s: &mut Schematic, a: (i32, i32), b: (i32, i32)) {
    s.wires.push(Wire { a, b });
}

fn lib_part(name: &str) -> part_lib::PartDef {
    let text = part_lib::EXAMPLES.iter().find(|(n, _)| n.starts_with(name)).unwrap().1;
    serde_json::from_str(text).unwrap()
}

#[test]
fn schematic_divider_end_to_end() {
    let mut s = Schematic::default();
    s.add(Kind::VSource, (0, 0), 0);
    s.components[0].value = "12".into();
    s.add(Kind::Resistor, (6, -6), 0); // R1 10k default
    s.add(Kind::Resistor, (12, 0), 1); // R2
    s.components[2].value = "2.2k".into();
    s.components[1].value = "4.7k".into();
    s.add(Kind::Ground, (0, 6), 0);
    let r2 = s.components[2].pins();
    wire(&mut s, (0, -2), (0, -6));
    wire(&mut s, (0, -6), (4, -6));
    wire(&mut s, (8, -6), (12, -6));
    wire(&mut s, (12, -6), r2[0].1);
    wire(&mut s, r2[1].1, (12, 6));
    wire(&mut s, (12, 6), (0, 6));
    wire(&mut s, (0, 2), (0, 6));
    let c = from_schematic(&s).unwrap();
    let res = dc_op(&c).unwrap();
    let mid = res.element("R2").unwrap().voltage;
    close(mid, 12.0 * 2200.0 / (4700.0 + 2200.0), 1e-9);
    assert_power_balance(&res);
}

#[test]
fn wiring_resistance_sits_in_series_with_the_pin() {
    let mut s = Schematic::default();
    s.add(Kind::VSource, (0, 0), 0);
    s.components[0].value = "12".into();
    s.add(Kind::Resistor, (6, -6), 0);
    s.add(Kind::Resistor, (12, 0), 1);
    s.components[1].value = "4.7k".into();
    s.components[2].value = "2.2k".into();
    s.add(Kind::Ground, (0, 6), 0);
    let r2 = s.components[2].pins();
    wire(&mut s, (0, -2), (0, -6));
    wire(&mut s, (0, -6), (4, -6));
    wire(&mut s, (8, -6), (12, -6));
    wire(&mut s, (12, -6), r2[0].1);
    wire(&mut s, r2[1].1, (12, 6));
    wire(&mut s, (12, 6), (0, 6));
    wire(&mut s, (0, 2), (0, 6));
    let w = [
        WiringR { refdes: "R2".into(), pin: "1".into(), ohm: 2200.0, henry: 0.0 },
        WiringR { refdes: "R9".into(), pin: "1".into(), ohm: 1.0, henry: 0.0 }, // not in the schematic: ignored
    ];
    let res = dc_op(&from_schematic_with_wiring(&s, &w).unwrap()).unwrap();
    close(res.element("R2").unwrap().voltage, 12.0 * 2200.0 / (4700.0 + 2200.0 + 2200.0), 1e-9);
    // no wiring list: the same as from_schematic
    let plain = dc_op(&from_schematic_with_wiring(&s, &[]).unwrap()).unwrap();
    close(plain.element("R2").unwrap().voltage, 12.0 * 2200.0 / 6900.0, 1e-9);
}

#[test]
fn wiring_inductance_matters_for_ac_and_not_for_dc() {
    let mut s = Schematic::default();
    s.add(Kind::VSource, (0, 0), 0);
    s.components[0].value = "0 AC 1".into();
    s.add(Kind::Resistor, (6, -6), 0);
    s.add(Kind::Resistor, (12, 0), 1);
    s.add(Kind::Ground, (0, 6), 0);
    let r2 = s.components[2].pins();
    wire(&mut s, (0, -2), (0, -6));
    wire(&mut s, (0, -6), (4, -6));
    wire(&mut s, (8, -6), (12, -6));
    wire(&mut s, (12, -6), r2[0].1);
    wire(&mut s, r2[1].1, (12, 6));
    wire(&mut s, (12, 6), (0, 6));
    wire(&mut s, (0, 2), (0, 6));
    let w = |henry: f64| [WiringR { refdes: "R2".into(), pin: "1".into(), ohm: 0.1, henry }];
    let (flat, coil) = (from_schematic_with_wiring(&s, &w(0.0)).unwrap(), from_schematic_with_wiring(&s, &w(0.1)).unwrap());
    assert!(coil.elements.iter().any(|e| e.name() == "R2.1.LW") && !flat.elements.iter().any(|e| e.name() == "R2.1.LW"));
    let (d0, d1) = (dc_op(&flat).unwrap(), dc_op(&coil).unwrap());
    close(d0.element("R2").unwrap().voltage, d1.element("R2").unwrap().voltage, 1e-9);
    let at = |c: &Circuit| {
        let r = ac_analysis(c, &AcSettings { fstart: 100e3, fstop: 100e3, sweep: AcSweep::Linear(2) }).unwrap();
        r.signal("I(R2)").unwrap().mag()[0]
    };
    // R2 is 10 kohm; 0.1 H is 63 kohm at 100 kHz, so the current through R2 falls to about a third
    let (m0, m1) = (at(&flat), at(&coil));
    assert!(m1 < 0.5 * m0, "0.1 H in front of R2 cuts 100 kHz: {m0} -> {m1}");
}

#[test]
fn coupled_supply_and_return_wiring_lowers_the_loop_inductance() {
    let mut s = Schematic::default();
    s.add(Kind::VSource, (0, 0), 0);
    s.components[0].value = "0 AC 1".into();
    s.add(Kind::Resistor, (6, -6), 0);
    s.add(Kind::Resistor, (12, 0), 1);
    s.add(Kind::Ground, (0, 6), 0);
    let r2 = s.components[2].pins();
    wire(&mut s, (0, -2), (0, -6));
    wire(&mut s, (0, -6), (4, -6));
    wire(&mut s, (8, -6), (12, -6));
    wire(&mut s, (12, -6), r2[0].1);
    wire(&mut s, r2[1].1, (12, 6));
    wire(&mut s, (12, 6), (0, 6));
    wire(&mut s, (0, 2), (0, 6));
    // the supply stretch in front of R1 and the return stretch behind R2 are in one loop, 1 H each
    let w = [
        WiringR { refdes: "R1".into(), pin: "1".into(), ohm: 0.0, henry: 1.0 },
        WiringR { refdes: "R2".into(), pin: "2".into(), ohm: 0.0, henry: 1.0 },
    ];
    let k = |k: f64| [WiringK { a: ("R1".into(), "1".into()), b: ("R2".into(), "2".into()), k }];
    let at = |ks: &[WiringK]| {
        let c = from_schematic_with_layout(&s, &w, ks).unwrap();
        let r = ac_analysis(&c, &AcSettings { fstart: 2e3, fstop: 2e3, sweep: AcSweep::Linear(2) }).unwrap();
        r.signal("I(R1)").unwrap().mag()[0]
    };
    // |Z| = 20k + j25k alone; with k = 0.9 the loop L is 0.2 H: 20k + j2.5k
    let (alone, coupled) = (at(&[]), at(&k(0.9)));
    assert!(coupled > 1.4 * alone, "{alone} -> {coupled}");
    assert!((at(&k(0.0)) - alone).abs() < 1e-12, "k = 0 changes nothing");
    let c = from_schematic_with_layout(&s, &w, &[WiringK { a: ("R1".into(), "1".into()), b: ("R9".into(), "1".into()), k: 0.5 }]).unwrap();
    assert!(!c.elements.iter().any(|e| matches!(e, Element::Mutual { .. })), "a coupling to a missing inductor is ignored");
}

#[test]
fn schematic_reports_bad_values() {
    let mut s = Schematic::default();
    s.add(Kind::Resistor, (0, 0), 0);
    s.components[0].value = "abc".into();
    let errs = from_schematic(&s).unwrap_err();
    assert_eq!(errs[0].refdes, "R1");
    assert!(errs[0].message.contains("abc"));
}

#[test]
fn library_diode_model_is_used() {
    let d = lib_part("1N4148");
    let mut s = Schematic::default();
    s.add(Kind::VSource, (0, 0), 0); // 5 V default
    s.add(Kind::Resistor, (6, -6), 0);
    s.components[1].value = "1k".into();
    s.add_custom(d.symbol(), &d.default_value(), &d.footprint_name(), (12, -6), 0); // A tip (10,-6), K tip (14,-6)
    s.add(Kind::Ground, (16, 6), 0);
    wire(&mut s, (0, -2), (0, -6));
    wire(&mut s, (0, -6), (4, -6));
    wire(&mut s, (8, -6), (10, -6));
    wire(&mut s, (14, -6), (16, -6));
    wire(&mut s, (16, -6), (16, 6));
    wire(&mut s, (0, 2), (0, 6));
    wire(&mut s, (0, 6), (16, 6));
    let c = from_schematic(&s).unwrap();
    let res = dc_op(&c).unwrap();
    // IS=2.52n RS=0.568 N=1.752 from the part file
    let (vj, i) = diode_oracle(5.0, 1000.0 + 0.568, 2.52e-9, 1.752 * VT);
    let d1 = res.element("D1").unwrap();
    close(d1.current, i, 1e-8);
    close(d1.voltage, vj + i * 0.568, 1e-5);
    assert_power_balance(&res);
}

#[test]
fn library_opamp_subckt_follower() {
    let u = lib_part("LM358");
    let mut s = Schematic::default();
    s.add(Kind::VSource, (0, 0), 0);
    s.components[0].value = "1".into();
    s.add_custom(u.symbol(), &u.default_value(), &u.footprint_name(), (20, 0), 0);
    s.add(Kind::Ground, (0, 6), 0);
    // 5 V single supply for the op-amp
    s.add(Kind::VSource, (28, 0), 0);
    s.add(Kind::Ground, (20, 6), 0);
    wire(&mut s, (0, -2), (0, -4));
    wire(&mut s, (0, -4), (16, -4));
    wire(&mut s, (16, -4), (16, -1)); // IN+
    wire(&mut s, (24, 0), (24, 8)); // OUT -> IN- (the route crosses the supply wire without touching it)
    wire(&mut s, (24, 8), (16, 8));
    wire(&mut s, (16, 8), (16, 1));
    wire(&mut s, (0, 2), (0, 6));
    wire(&mut s, (28, -2), (28, -4)); // supply +  -> V+
    wire(&mut s, (28, -4), (20, -4));
    wire(&mut s, (20, 4), (20, 6)); // V- -> ground
    wire(&mut s, (28, 2), (28, 6)); // supply - -> ground
    wire(&mut s, (28, 6), (20, 6));
    let c = from_schematic(&s).unwrap();
    let res = dc_op(&c).unwrap();
    // closed loop gain A/(1+A) with A = 1e5
    let follower = res.element("U1.Eo").expect("subcircuit elements are prefixed with the instance name");
    close(follower.voltage, 1e5 / (1.0 + 1e5), 1e-7);
    assert!(res.warnings.is_empty(), "{:?}", res.warnings);
}

#[test]
fn unsupported_subckt_element_is_an_error_not_silence() {
    let mut c = Circuit::new();
    let (a, b) = (c.node("a"), c.node("b"));
    let text = ".subckt X 1 2
W1 1 0 V1 SW1
.ends X";
    let e = add_subckt_instance(&mut c, "U1", "X", text, &[a, b]).unwrap_err();
    assert!(e.contains("未対応"), "{e}");
}

#[test]
fn model_cards_parse() {
    let text = "* comment\n.model DX D(IS=1e-12 N=1.5\n+ RS=2) ; trailing\n.model QX NPN(BF=250)";
    let m = parse_models(text).unwrap();
    match &m["dx"] {
        spice_in::Model::Diode(d) => assert_eq!((d.is, d.n, d.rs), (1e-12, 1.5, 2.0)),
        _ => panic!(),
    }
    // a card of an unknown kind is skipped; an element that uses it is reported when it is read
    assert!(parse_models(".model Z JFET(X=1)").unwrap().is_empty());
}

#[test]
fn fallback_strategies_reach_the_same_solution() {
    use crate::dc::{dc_op_with, Strategy};
    let mut c = Circuit::new();
    vs(&mut c, "V1", "in", "0", 20.0);
    r(&mut c, "R1", "in", "n0", 100.0);
    for i in 0..6 {
        let (a, k) = (format!("n{i}"), if i == 5 { "0".to_string() } else { format!("n{}", i + 1) });
        diode(&mut c, &format!("D{i}"), &a, &k, DiodeModel { is: 1e-14, n: 1.0, rs: 1.0, ..DiodeModel::default() });
    }
    let q = c.node("n0");
    let b = c.node("base");
    r(&mut c, "Rb", "n0", "base", 47e3);
    c.add(Element::Bjt { name: "Q1".into(), c: q, b, e: 0, model: BjtModel { bf: 150.0, ..BjtModel::default() } });

    let direct = dc_op_with(&c, &[Strategy::Direct]).unwrap();
    let gmin = dc_op_with(&c, &[Strategy::Gmin]).unwrap();
    let source = dc_op_with(&c, &[Strategy::Source]).unwrap();
    assert_eq!((gmin.strategy, source.strategy), ("gmin stepping", "source stepping"));
    for other in [&gmin, &source] {
        for (n, v) in &direct.voltages {
            close(other.voltage(n).unwrap(), *v, 1e-5);
        }
    }
    assert_power_balance(&gmin);
    assert_power_balance(&source);
}
