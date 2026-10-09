//! The IP2332 library part: an averaged synchronous buck charger for one Li-ion cell.

use crate::*;

fn model() -> String {
    let part: serde_json::Value = serde_json::from_str(include_str!("../../part-lib/examples/IP2332_ESOP-8.part.json")).unwrap();
    part["sim"]["text"].as_str().unwrap().to_string()
}

#[derive(Clone, Copy)]
struct Setup {
    vin: f64,
    vbat: f64,
    /// RISET in ohms (None = ISET open).
    riset: Option<f64>,
    /// NTC resistor in ohms.
    rntc: f64,
}

impl Default for Setup {
    fn default() -> Self {
        Setup { vin: 5.0, vbat: 3.7, riset: Some(4.3e3), rntc: 10e3 }
    }
}

fn build(s: Setup) -> Circuit {
    let mut c = Circuit::new();
    let names = ["ntc", "iset", "led2", "led1", "bat", "lx", "vin"];
    let n: Vec<usize> = names.iter().map(|x| c.node(x)).collect();
    let (ntc, iset, led2, led1, bat, lx, vin) = (n[0], n[1], n[2], n[3], n[4], n[5], n[6]);
    // ports: ntc iset led2 led1 bat lx gnd vin
    add_subckt_instance(&mut c, "U1", "IP2332", &model(), &[ntc, iset, led2, led1, bat, lx, 0, vin]).unwrap();
    c.add(Element::vdc("Vin", vin, 0, s.vin));
    c.add(Element::vdc("Vbat", bat, 0, s.vbat));
    c.add(Element::Inductor { name: "L1".into(), a: lx, b: bat, l: 2.2e-6 });
    c.add(Element::Resistor { name: "Rntc".into(), a: ntc, b: 0, r: s.rntc });
    if let Some(r) = s.riset {
        c.add(Element::Resistor { name: "Riset".into(), a: iset, b: 0, r });
    }
    // LEDs as plain loads: 2k to ground
    c.add(Element::Resistor { name: "Rl1".into(), a: led1, b: 0, r: 600.0 });
    c.add(Element::Resistor { name: "Rl2".into(), a: led2, b: 0, r: 2e3 });
    c
}

fn charge_current(s: Setup) -> f64 {
    dc_op(&build(s)).unwrap().element("Vbat").unwrap().current
}

#[test]
fn the_current_follows_the_riset_table() {
    for (r, want) in [(1.4e3, 2.31), (2.4e3, 1.5), (4.3e3, 0.99), (10e3, 0.625)] {
        let i = charge_current(Setup { riset: Some(r), ..Default::default() });
        assert!((i / want - 1.0).abs() < 0.04, "RISET {r}: {i} A, want {want}");
    }
    // ISET open (RISET >= 120k) is 2 A
    let open = charge_current(Setup { riset: None, ..Default::default() });
    assert!((open / 2.0 - 1.0).abs() < 0.04, "{open}");
}

#[test]
fn below_3v_the_cell_gets_a_fifth_and_near_full_the_current_tapers() {
    let pre = charge_current(Setup { vbat: 2.6, riset: None, ..Default::default() });
    assert!((pre / 0.4 - 1.0).abs() < 0.1, "{pre}");
    let cc = charge_current(Setup { vbat: 3.9, ..Default::default() });
    assert!((cc / 0.99 - 1.0).abs() < 0.04, "{cc}");
    // the voltage loop takes over 17 mV below the float voltage and the current falls linearly
    let mid = charge_current(Setup { vbat: 4.19, ..Default::default() });
    let end = charge_current(Setup { vbat: 4.198, ..Default::default() });
    assert!(mid < 0.7 && mid > 0.4 && end < 0.2 && end > 0.0, "{mid} {end}");
    let full = charge_current(Setup { vbat: 4.21, ..Default::default() });
    assert!(full.abs() < 0.02, "no current into a full cell, none back out: {full}");
}

#[test]
fn it_stops_for_a_bad_input_or_a_bad_cell_temperature() {
    assert!(charge_current(Setup { vin: 4.0, ..Default::default() }).abs() < 5e-3, "under-voltage");
    assert!(charge_current(Setup { vin: 6.5, vbat: 3.0, ..Default::default() }).abs() < 5e-3, "over-voltage");
    // 100 uA in the NTC: < 3k is too hot, > 27.3k too cold, in between it charges
    assert!(charge_current(Setup { rntc: 2e3, ..Default::default() }).abs() < 5e-3, "too hot");
    assert!(charge_current(Setup { rntc: 30e3, ..Default::default() }).abs() < 5e-3, "too cold");
    let cool = charge_current(Setup { rntc: 22e3, ..Default::default() });
    assert!((cool / 0.5 - 1.0).abs() < 0.1, "cool cell: half the current, got {cool}");
    let warm = charge_current(Setup { rntc: 4e3, vbat: 4.12, ..Default::default() });
    let normal = charge_current(Setup { rntc: 10e3, vbat: 4.12, ..Default::default() });
    assert!(warm < normal, "a warm cell is charged to a lower voltage: {warm} vs {normal}");
}

#[test]
fn the_leds_show_charging_and_full() {
    let node = |s: Setup, n: &str| dc_op(&build(s)).unwrap().voltage(n).unwrap();
    let charging = Setup::default();
    // LED1 is a 5 mA source into the 600 ohm load
    assert!((node(charging, "led1") - 3.0).abs() < 0.3, "{}", node(charging, "led1"));
    assert!(node(charging, "led2") < 0.3);
    let full = Setup { vbat: 4.2, ..Default::default() };
    assert!(node(full, "led1") < 0.3 && node(full, "led2") > 2.0, "{} {}", node(full, "led1"), node(full, "led2"));
}

/// A capacitor as the cell, charged from empty: up to 4.2 V, no overshoot.
#[test]
fn a_capacitor_cell_charges_up_to_4v2_without_overshoot() {
    let mut c = build(Setup { vbat: 0.0, riset: Some(4.3e3), ..Default::default() });
    c.elements.retain(|e| e.name() != "Vbat");
    let bat = c.node("bat");
    let mid = c.node("cell");
    c.add(Element::Resistor { name: "Resr".into(), a: bat, b: mid, r: 0.1 });
    c.add(Element::Capacitor { name: "Cell".into(), a: mid, b: 0, c: 470e-6 });
    let res = transient(&c, &TranSettings { tstop: 15e-3, tstep: 10e-6, uic: true }).expect("the charger must simulate");
    let v = &res.signal("V(cell)").unwrap().values;
    let last = *v.last().unwrap();
    assert!((last - 4.2).abs() < 0.08, "ends at {last}");
    assert!(v.iter().cloned().fold(0.0, f64::max) < 4.3, "no overshoot");
}
