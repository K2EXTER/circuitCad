//! The battery charger library parts (TP4056, MCP73831): behavioural linear Li-ion chargers.

use crate::*;

fn part_text(json: &str) -> String {
    let part: serde_json::Value = serde_json::from_str(json).unwrap();
    part["sim"]["text"].as_str().unwrap().to_string()
}

fn tp4056_text() -> String {
    part_text(include_str!("../../part-lib/examples/TP4056_ESOP-8.part.json"))
}

fn r(c: &mut Circuit, name: &str, a: usize, b: usize, ohms: f64) {
    c.add(Element::Resistor { name: name.into(), a, b, r: ohms });
}

struct Rig {
    c: Circuit,
    bat: usize,
}

/// TP4056 with `vin` volts on VCC, R_PROG = `rprog`, and the cell replaced by a stiff source `vbat`.
fn tp4056_dc(vin: f64, rprog: f64, vbat: f64) -> Rig {
    let mut c = Circuit::new();
    let n: Vec<usize> = ["temp", "prog", "vcc", "bat", "stdby", "chrg", "ce"].iter().map(|s| c.node(s)).collect();
    let (temp, prog, vcc, bat, stdby, chrg, ce) = (n[0], n[1], n[2], n[3], n[4], n[5], n[6]);
    // ports: temp prog gnd vcc bat stdby chrg ce
    add_subckt_instance(&mut c, "U1", "TP4056", &tp4056_text(), &[temp, prog, 0, vcc, bat, stdby, chrg, ce]).unwrap();
    c.add(Element::vdc("Vin", vcc, 0, vin));
    c.add(Element::vdc("Vbat", bat, 0, vbat));
    c.add(Element::vdc("Vce", ce, 0, vin));
    r(&mut c, "Rprog", prog, 0, rprog);
    r(&mut c, "Rtemp", temp, 0, 1.0);
    // LED loads: pull-ups to VCC
    r(&mut c, "Rc", vcc, chrg, 1e3);
    r(&mut c, "Rs", vcc, stdby, 1e3);
    Rig { c, bat }
}

fn charge_current(rig: &Rig) -> f64 {
    // the cell source absorbs the charge current (it flows through the source from + to -)
    dc_op(&rig.c).unwrap().element("Vbat").unwrap().current
}

#[test]
fn constant_current_follows_1200_over_rprog() {
    for (rprog, want) in [(1.2e3, 1.0), (2.4e3, 0.5), (4.8e3, 0.25)] {
        let rig = tp4056_dc(5.0, rprog, 3.7);
        let i = charge_current(&rig);
        assert!((i / want - 1.0).abs() < 0.03, "Rprog {rprog}: {i} A, want {want}");
    }
}

#[test]
fn a_deeply_discharged_cell_gets_a_tenth_of_the_current() {
    let rig = tp4056_dc(5.0, 2.4e3, 2.5);
    let i = charge_current(&rig);
    assert!((i / 0.05 - 1.0).abs() < 0.1, "{i}");
}

#[test]
fn near_full_the_current_tapers_and_at_4v2_it_stops() {
    let i_cc = charge_current(&tp4056_dc(5.0, 2.4e3, 4.0));
    let i_mid = charge_current(&tp4056_dc(5.0, 2.4e3, 4.19));
    let i_end = charge_current(&tp4056_dc(5.0, 2.4e3, 4.198));
    let i_full = charge_current(&tp4056_dc(5.0, 2.4e3, 4.21));
    assert!((i_cc - 0.5).abs() < 0.02, "constant current up to the voltage limit: {i_cc}");
    assert!(i_mid < 0.3 && i_mid > i_end, "the current tapers: {i_mid} {i_end}");
    assert!(i_end < 0.06, "{i_end}");
    assert!(i_full.abs() < 0.01, "no current into a full cell, none back out: {i_full}");
}

#[test]
fn no_current_without_enough_supply_or_when_disabled() {
    // 3.4 V is below the under-voltage lockout
    assert!(charge_current(&tp4056_dc(3.4, 2.4e3, 3.0)).abs() < 5e-3);
    // supply barely above the cell: nothing flows
    assert!(charge_current(&tp4056_dc(3.8, 2.4e3, 3.75)).abs() < 5e-3);
}

#[test]
fn status_pins_show_charging_and_done() {
    let node = |rig: &Rig, n: &str| dc_op(&rig.c).unwrap().voltage(n).unwrap();
    let charging = tp4056_dc(5.0, 2.4e3, 3.7);
    assert!(node(&charging, "chrg") < 0.8 && node(&charging, "stdby") > 4.0, "CHRG low while charging");
    let done = tp4056_dc(5.0, 2.4e3, 4.2);
    assert!(node(&done, "stdby") < 0.8 && node(&done, "chrg") > 4.0, "STDBY low when terminated");
    let _ = (charging.bat, done.bat);
}

fn mcp73831_text() -> String {
    part_text(include_str!("../../part-lib/examples/MCP73831_SOT-23-5.part.json"))
}

#[test]
fn mcp73831_uses_1000_over_rprog_and_pulls_stat_low_while_charging() {
    let mut c = Circuit::new();
    let n: Vec<usize> = ["stat", "vbat", "vdd", "prog"].iter().map(|s| c.node(s)).collect();
    let (stat, vbat, vdd, prog) = (n[0], n[1], n[2], n[3]);
    // ports: stat vss vbat vdd prog
    add_subckt_instance(&mut c, "U1", "MCP73831", &mcp73831_text(), &[stat, 0, vbat, vdd, prog]).unwrap();
    c.add(Element::vdc("Vin", vdd, 0, 5.0));
    c.add(Element::vdc("Vbat", vbat, 0, 3.7));
    r(&mut c, "Rprog", prog, 0, 2e3);
    r(&mut c, "Rled", vdd, stat, 1e3);
    let res = dc_op(&c).unwrap();
    let i = res.element("Vbat").unwrap().current;
    assert!((i / 0.5 - 1.0).abs() < 0.03, "{i}");
    assert!(res.voltage("stat").unwrap() < 0.8);
}

/// A 100 uF "cell" charged from empty: pre-charge, then constant current, then the taper.
#[test]
fn a_capacitor_cell_goes_through_all_three_charge_phases() {
    let mut c = Circuit::new();
    let n: Vec<usize> = ["temp", "prog", "vcc", "bat", "stdby", "chrg", "ce"].iter().map(|s| c.node(s)).collect();
    let (temp, prog, vcc, bat, stdby, chrg, ce) = (n[0], n[1], n[2], n[3], n[4], n[5], n[6]);
    add_subckt_instance(&mut c, "U1", "TP4056", &tp4056_text(), &[temp, prog, 0, vcc, bat, stdby, chrg, ce]).unwrap();
    c.add(Element::vdc("Vin", vcc, 0, 5.0));
    c.add(Element::vdc("Vce", ce, 0, 5.0));
    r(&mut c, "Rprog", prog, 0, 1.2e3); // 1 A
    r(&mut c, "Rtemp", temp, 0, 1.0);
    r(&mut c, "Rc", vcc, chrg, 1e3);
    r(&mut c, "Rs", vcc, stdby, 1e3);
    let mid = c.node("cell");
    r(&mut c, "Resr", bat, mid, 0.2);
    c.add(Element::Capacitor { name: "Cell".into(), a: mid, b: 0, c: 100e-6 });
    let res = transient(&c, &TranSettings { tstop: 6e-3, tstep: 10e-6, uic: true }).expect("the charger must simulate");
    let v = &res.signal("V(bat)").unwrap().values;
    let at = |t: f64| v[res.time.iter().position(|x| *x >= t).unwrap_or(v.len() - 1)];
    // pre-charge: 100 mA into 100 uF is 1 V/ms (2.9 V takes 2.9 ms); CC: 1 A is 10 V/ms
    assert!(at(1e-3) > 0.7 && at(1e-3) < 1.3, "slow start while the cell is empty: {}", at(1e-3));
    let final_v = at(6e-3);
    assert!((final_v - 4.2).abs() < 0.05, "ends at the regulation voltage: {final_v}");
    // once past the pre-charge threshold the cell rises at about 10 V/ms
    let cell = &res.signal("V(cell)").unwrap().values;
    let steep = (1..cell.len()).filter(|&k| cell[k] > 3.2 && cell[k] < 3.9).map(|k| (cell[k] - cell[k - 1]) / (res.time[k] - res.time[k - 1])).fold(0.0, f64::max);
    assert!(steep > 8e3 && steep < 13e3, "constant-current slope {steep} V/s");
    assert!(*v.last().unwrap() < 4.25, "no overshoot past the limit");
    // /STDBY ends up low, /CHRG released
    let last = |n: &str| *res.signal(n).unwrap().values.last().unwrap();
    assert!(last("V(chrg)") > 3.5, "{}", last("V(chrg)"));
}

#[test]
fn a_cell_that_starts_at_3v2_skips_the_pre_charge() {
    let mut c = Circuit::new();
    let n: Vec<usize> = ["temp", "prog", "vcc", "bat", "stdby", "chrg", "ce"].iter().map(|s| c.node(s)).collect();
    let (temp, prog, vcc, bat, stdby, chrg, ce) = (n[0], n[1], n[2], n[3], n[4], n[5], n[6]);
    add_subckt_instance(&mut c, "U1", "TP4056", &tp4056_text(), &[temp, prog, 0, vcc, bat, stdby, chrg, ce]).unwrap();
    c.add(Element::vdc("Vin", vcc, 0, 5.0));
    c.add(Element::vdc("Vce", ce, 0, 5.0));
    r(&mut c, "Rprog", prog, 0, 1.2e3);
    r(&mut c, "Rtemp", temp, 0, 1.0);
    r(&mut c, "Rc", vcc, chrg, 1e3);
    r(&mut c, "Rs", vcc, stdby, 1e3);
    c.add(Element::Capacitor { name: "Cell".into(), a: bat, b: 0, c: 100e-6 });
    c.set_initial_voltage("Cell", 3.2);
    let res = transient(&c, &TranSettings { tstop: 1e-3, tstep: 10e-6, uic: false }).unwrap();
    let v = &res.signal("V(bat)").unwrap().values;
    assert!((v[0] - 3.2).abs() < 1e-6, "starts at the initial voltage: {}", v[0]);
    // 1 A into 100 uF: 10 V/ms, so it reaches the 4.2 V limit within 0.15 ms
    let k = res.time.iter().position(|t| *t >= 0.1e-3).unwrap();
    assert!(v[k] > 3.9, "no slow pre-charge: {}", v[k]);
}
