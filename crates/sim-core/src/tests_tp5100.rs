//! The TP5100 library part: an averaged buck charger for one (4.2 V) or two (8.4 V) Li-ion cells.

use crate::*;

fn model() -> String {
    let part: serde_json::Value = serde_json::from_str(include_str!("../../part-lib/examples/TP5100_QFN-16.part.json")).unwrap();
    part["sim"]["text"].as_str().unwrap().to_string()
}

#[derive(Clone, Copy)]
struct Setup {
    vin: f64,
    rs: f64,
    vbat: f64,
    /// CS pin: None = open (single cell), Some(true) = VREG (two cells), Some(false) = GND.
    cs: Option<bool>,
    /// RTRICK resistor in ohms (None = open).
    rtrick: Option<f64>,
    ts_ground: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Setup { vin: 9.0, rs: 0.1, vbat: 3.7, cs: None, rtrick: None, ts_ground: true }
    }
}

fn build(s: Setup) -> Circuit {
    let mut c = Circuit::new();
    let names = ["vin", "lx", "pwr", "vs", "bat", "vreg", "ts", "rtrick", "cs", "stdby", "chrg"];
    let n: Vec<usize> = names.iter().map(|x| c.node(x)).collect();
    let (vin, lx, pwr, vs, bat, vreg, ts, rtrick, cs, stdby, chrg) = (n[0], n[1], n[2], n[3], n[4], n[5], n[6], n[7], n[8], n[9], n[10]);
    // ports: vin lx pwr gnd vs bat vreg ts rtrick cs stdby chrg
    add_subckt_instance(&mut c, "U1", "TP5100", &model(), &[vin, lx, pwr, 0, vs, bat, vreg, ts, rtrick, cs, stdby, chrg]).unwrap();
    c.add(Element::vdc("Vin", vin, 0, s.vin));
    c.add(Element::vdc("Vbat", bat, 0, s.vbat));
    c.add(Element::Inductor { name: "L1".into(), a: lx, b: vs, l: 10e-6 });
    c.add(Element::Resistor { name: "Rs".into(), a: vs, b: bat, r: s.rs });
    c.add(Element::Resistor { name: "Rc".into(), a: vin, b: chrg, r: 1e3 });
    c.add(Element::Resistor { name: "Rd".into(), a: vin, b: stdby, r: 1e3 });
    c.add(Element::Resistor { name: "Rpw".into(), a: vin, b: pwr, r: 10e3 });
    match s.cs {
        Some(true) => c.add(Element::Resistor { name: "Rcs".into(), a: vreg, b: cs, r: 1.0 }),
        Some(false) => c.add(Element::Resistor { name: "Rcs".into(), a: cs, b: 0, r: 1.0 }),
        None => {}
    }
    if let Some(r) = s.rtrick {
        c.add(Element::Resistor { name: "Rtr".into(), a: rtrick, b: 0, r });
    }
    if s.ts_ground {
        c.add(Element::Resistor { name: "Rts".into(), a: ts, b: 0, r: 1.0 });
    }
    c
}

fn charge_current(s: Setup) -> f64 {
    dc_op(&build(s)).unwrap().element("Vbat").unwrap().current
}

#[test]
fn the_current_is_a_tenth_of_a_volt_over_rs() {
    for (rs, want) in [(0.1, 1.0), (0.067, 1.49), (0.05, 2.0), (0.2, 0.5)] {
        let i = charge_current(Setup { rs, vbat: 3.7, ..Default::default() });
        assert!((i / want - 1.0).abs() < 0.03, "Rs {rs}: {i} A, want {want}");
    }
}

#[test]
fn a_deeply_discharged_cell_is_precharged_at_the_rtrick_fraction() {
    // 2.5 V is below the 2.9 V threshold; 50k gives 20 %, open gives 100 %
    let i50 = charge_current(Setup { vbat: 2.5, rtrick: Some(50e3), ..Default::default() });
    assert!((i50 / 0.2 - 1.0).abs() < 0.1, "{i50}");
    let i114 = charge_current(Setup { vbat: 2.5, rtrick: Some(114e3), ..Default::default() });
    assert!((i114 / 0.3 - 1.0).abs() < 0.1, "{i114}");
    let iopen = charge_current(Setup { vbat: 2.5, ..Default::default() });
    assert!((iopen / 1.0 - 1.0).abs() < 0.05, "{iopen}");
}

#[test]
fn single_cell_tapers_into_4v2_and_stops() {
    let i_cc = charge_current(Setup { vbat: 4.0, ..Default::default() });
    let i_mid = charge_current(Setup { vbat: 4.17, ..Default::default() });
    let i_full = charge_current(Setup { vbat: 4.21, ..Default::default() });
    assert!((i_cc - 1.0).abs() < 0.04, "{i_cc}");
    assert!(i_mid < 0.8 && i_mid > 0.05, "{i_mid}");
    assert!(i_full.abs() < 0.02, "no current into a full cell, none back out: {i_full}");
}

#[test]
fn cs_to_vreg_selects_two_cells_and_cs_low_stops_charging() {
    // 7.4 V is a half-full two-cell pack: it gets the full current only in the two-cell mode
    let double = charge_current(Setup { vbat: 7.4, cs: Some(true), vin: 12.0, ..Default::default() });
    assert!((double - 1.0).abs() < 0.05, "{double}");
    let single = charge_current(Setup { vbat: 7.4, vin: 12.0, ..Default::default() });
    assert!(single.abs() < 0.02, "a single-cell charger does not push a 7.4 V pack: {single}");
    let full = charge_current(Setup { vbat: 8.45, cs: Some(true), vin: 12.0, ..Default::default() });
    assert!(full.abs() < 0.02, "{full}");
    let off = charge_current(Setup { vbat: 3.7, cs: Some(false), ..Default::default() });
    assert!(off.abs() < 0.01, "{off}");
    // pre-charge threshold is 5.8 V in the two-cell mode
    let pre = charge_current(Setup { vbat: 5.0, cs: Some(true), vin: 12.0, rtrick: Some(50e3), ..Default::default() });
    assert!((pre / 0.2 - 1.0).abs() < 0.15, "{pre}");
}

#[test]
fn no_current_without_enough_input_or_with_a_bad_temperature() {
    assert!(charge_current(Setup { vin: 3.4, vbat: 3.0, ..Default::default() }).abs() < 5e-3);
    assert!(charge_current(Setup { vin: 3.9, vbat: 3.85, ..Default::default() }).abs() < 5e-3);
    // TS left open means no temperature check, and the charge runs
    assert!((charge_current(Setup { ts_ground: false, ..Default::default() }) - 1.0).abs() < 0.04);
}

#[test]
fn status_pins_show_charging_and_done_and_pwr_on_follows_vin() {
    let node = |s: Setup, n: &str| dc_op(&build(s)).unwrap().voltage(n).unwrap();
    let charging = Setup::default();
    assert!(node(charging, "chrg") < 1.0 && node(charging, "stdby") > 7.0, "CHRG low while charging");
    assert!(node(charging, "pwr") < 0.5, "PWR_ON- low with a supply");
    let done = Setup { vbat: 4.2, ..Default::default() };
    assert!(node(done, "stdby") < 1.0 && node(done, "chrg") > 7.0, "STDBY low when terminated");
    assert!((node(charging, "vreg") - 5.0).abs() < 0.1);
}

/// A small capacitor as the cell, charged from empty: the voltage rises and settles at 4.2 V.
#[test]
fn a_capacitor_cell_charges_up_to_4v2_without_overshoot() {
    let mut c = build(Setup { vbat: 0.0, ..Default::default() });
    c.elements.retain(|e| e.name() != "Vbat");
    let bat = c.node("bat");
    let mid = c.node("cell");
    c.add(Element::Resistor { name: "Resr".into(), a: bat, b: mid, r: 0.2 });
    c.add(Element::Capacitor { name: "Cell".into(), a: mid, b: 0, c: 470e-6 });
    let res = transient(&c, &TranSettings { tstop: 12e-3, tstep: 10e-6, uic: true }).expect("the charger must simulate");
    let v = &res.signal("V(cell)").unwrap().values;
    let last = *v.last().unwrap();
    assert!((last - 4.2).abs() < 0.08, "ends at {last}");
    let peak = v.iter().cloned().fold(0.0, f64::max);
    assert!(peak < 4.3, "no overshoot past the float voltage: {peak}");
}
