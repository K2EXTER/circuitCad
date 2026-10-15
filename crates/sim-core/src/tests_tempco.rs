//! Temperature coefficients (`TC=` / `TC1=` / `TC2=`) of resistors, capacitors and inductors in library models.

use crate::*;

fn divider(text: &str) -> Circuit {
    let mut c = Circuit::new();
    let (a, o) = (c.node("a"), c.node("o"));
    add_subckt_instance(&mut c, "U1", "T", text, &[a, o, 0]).unwrap_or_else(|e| panic!("{e}"));
    c.add(Element::vdc("Vin", a, 0, 1.0));
    c
}

fn vout(c: &Circuit, temp: f64) -> f64 {
    dc_op(&c.at_temperature(temp)).unwrap().voltage("o").unwrap()
}

#[test]
fn a_resistor_with_tc1_changes_with_temperature() {
    let c = divider(".subckt T a o gnd\nR1 a o 1k TC=1m\nR2 o gnd 1k\n.ends T");
    assert!((vout(&c, 27.0) - 0.5).abs() < 1e-9);
    // +100 degC: R1 = 1.1k, so Vo = 1k / 2.1k
    assert!((vout(&c, 127.0) - 1.0 / 2.1).abs() < 1e-9, "{}", vout(&c, 127.0));
    // the circuit itself is not changed by asking for a temperature
    assert!((vout(&c, 27.0) - 0.5).abs() < 1e-9);
}

#[test]
fn tc2_and_the_tc1_tc2_spelling_work() {
    let c = divider(".subckt T a o gnd\nR1 a o 1k TC1=0 TC2=100u\nR2 o gnd 1k\n.ends T");
    // dT = 100: 1 + 100u * 10000 = 2
    assert!((vout(&c, 127.0) - 1.0 / 3.0).abs() < 1e-9, "{}", vout(&c, 127.0));
}

#[test]
fn a_part_without_tc_stays_ideal() {
    let c = divider(".subckt T a o gnd\nR1 a o 1k\nR2 o gnd 1k\n.ends T");
    assert!((vout(&c, 127.0) - 0.5).abs() < 1e-9);
}

#[test]
fn the_temperature_sweep_sees_the_coefficient() {
    let c = divider(".subckt T a o gnd\nR1 a o 1k TC=1m\nR2 o gnd 1k\n.ends T");
    let r = temp_sweep(&c, 27.0, 127.0, 50.0).unwrap();
    let o = r.signals.iter().find(|s| s.name.eq_ignore_ascii_case("V(o)") || s.name.eq_ignore_ascii_case("o")).expect("signal o");
    assert!(o.values[0] > o.values[2], "{:?}", o.values);
}
