//! The first power-on guide (`poweron.rs`): the expected supply current, its range and the limit, and the verdict on a reading.

use crate::circuit::{Circuit, DiodeModel, Element};
use crate::poweron::{nice_up, power_on, PowerOnSettings, Reading, SupplyCurrent, LIMIT_FLOOR};
use crate::selfheat::Heater;

fn settings(tol_r: f64) -> PowerOnSettings {
    PowerOnSettings { tol_r, tol_c: 0.0, tol_l: 0.0, ..Default::default() }
}

/// 5 V into two resistors in series to ground (10 mA at their printed values).
fn two_resistors(r1: f64, r2: f64) -> Circuit {
    let mut c = Circuit::new();
    let (vin, mid) = (c.node("VIN"), c.node("MID"));
    c.elements.push(Element::vdc("V1", vin, 0, 5.0));
    c.elements.push(Element::Resistor { name: "R1".into(), a: vin, b: mid, r: r1 });
    c.elements.push(Element::Resistor { name: "R2".into(), a: mid, b: 0, r: r2 });
    c
}

#[test]
fn resistors_give_the_exact_corners() {
    let p = power_on(&two_resistors(250.0, 250.0), &[], &settings(0.05)).unwrap();
    let s = &p.supplies[0];
    assert_eq!((s.name.as_str(), s.volts), ("V1", 5.0));
    assert!((s.nominal - 0.01).abs() < 1e-9, "{}", s.nominal);
    // both resistors at +5 % / -5 %
    assert!((s.min - 5.0 / 525.0).abs() < 1e-9 && (s.max - 5.0 / 475.0).abs() < 1e-9, "{} {}", s.min, s.max);
    // 10.53 mA × 1.5 = 15.8 mA → 20 mA
    assert!((s.limit - 0.02).abs() < 1e-12, "{}", s.limit);
    assert_eq!(s.parts.len(), 2);
    assert!(!p.heated && p.unsolved.is_empty());
}

#[test]
fn self_heating_widens_the_side_it_moves() {
    let c = two_resistors(250.0, 250.0);
    // R1 dissipates 25 mW; at 2000 K/W it warms by about 50 K and its +1000 ppm/K raises it by about 5 %: less current
    let h = Heater { name: "R1".into(), elements: vec!["R1".into()], theta: 2000.0, tc1: 1e-3, tc2: 0.0, through_pins: false };
    let cold = power_on(&c, &[], &settings(0.0)).unwrap();
    let warm = power_on(&c, &[h], &settings(0.0)).unwrap();
    let (a, b) = (&cold.supplies[0], &warm.supplies[0]);
    assert!(warm.heated);
    assert!((a.min - a.max).abs() < 1e-15, "no tolerance: no range");
    let w = b.warm.unwrap();
    assert!(w < b.nominal * 0.99, "warm {w}");
    assert!((b.min - w).abs() < 1e-12 && (b.max - b.nominal).abs() < 1e-12, "{} {} {}", b.min, b.max, w);
}

#[test]
fn a_diode_load_and_a_charged_cell_are_told_apart() {
    // 5 V → 330 Ω → LED-like diode; a 3 V source that takes current in (a cell being charged through 100 Ω) is not a supply
    let mut c = Circuit::new();
    let (vin, a, cell) = (c.node("VIN"), c.node("A"), c.node("CELL"));
    c.elements.push(Element::vdc("V1", vin, 0, 5.0));
    c.elements.push(Element::Resistor { name: "R1".into(), a: vin, b: a, r: 330.0 });
    c.elements.push(Element::Diode { name: "D1".into(), a, k: 0, model: DiodeModel::default() });
    c.elements.push(Element::Resistor { name: "R2".into(), a: vin, b: cell, r: 100.0 });
    c.elements.push(Element::vdc("V2", cell, 0, 3.0));
    let p = power_on(&c, &[], &settings(0.05)).unwrap();
    assert_eq!(p.supplies.len(), 1, "{:?}", p.supplies);
    let s = &p.supplies[0];
    assert!(s.min < s.nominal && s.nominal < s.max);
    // R1 and R2 both move it; the diode has no tolerance
    assert!(s.parts.iter().all(|(n, _, _)| n == "R1" || n == "R2"));
}

#[test]
fn no_supply_is_an_error_with_a_way_out() {
    let mut c = Circuit::new();
    let a = c.node("A");
    c.elements.push(Element::Resistor { name: "R1".into(), a, b: 0, r: 1e3 });
    let e = power_on(&c, &[], &settings(0.05)).unwrap_err().to_string();
    assert!(e.contains("電源"), "{e}");
    assert!(power_on(&two_resistors(1.0, 1.0), &[], &PowerOnSettings { margin: 0.5, ..settings(0.05) }).is_err());
}

#[test]
fn nice_values_round_up() {
    for (x, want) in [(0.0158, 0.02), (0.012, 0.012), (0.0121, 0.015), (0.31, 0.4), (0.9, 1.0), (1.0, 1.0), (7.0, 8.0), (81.0, 100.0)] {
        assert!((nice_up(x) - want).abs() < want * 1e-9, "{x} → {} (want {want})", nice_up(x));
    }
    assert_eq!(nice_up(0.0), 0.0);
}

#[test]
fn readings_are_judged_against_the_range_and_the_limit() {
    let s = SupplyCurrent { name: "V1".into(), volts: 5.0, nominal: 0.010, warm: None, min: 0.009, max: 0.011, limit: 0.02, parts: vec![] };
    for (ma, want) in [(10.0, Reading::Normal), (9.0, Reading::Normal), (8.0, Reading::Low), (4.0, Reading::MuchLow), (0.5, Reading::Nothing), (12.0, Reading::High), (19.5, Reading::MuchHigh), (25.0, Reading::MuchHigh)] {
        assert_eq!(s.judge(ma * 1e-3), want, "{ma} mA");
    }
    // a small circuit whose limit sits at the floor: clearly too much is twice the high end, before the limit
    let small = SupplyCurrent { min: 0.0009, max: 0.0011, limit: LIMIT_FLOOR, ..s };
    assert_eq!(small.judge(0.003), Reading::MuchHigh);
    for r in [Reading::Nothing, Reading::MuchLow, Reading::Low, Reading::Normal, Reading::High, Reading::MuchHigh] {
        assert!(!r.causes().is_empty() && !r.title().is_empty());
    }
    assert!(Reading::MuchHigh.cut_now() && !Reading::High.cut_now());
}
