use crate::*;

/// V1(5V) -> R1 -> R2 -> GND, with the divider midpoint tapped by a wire.
fn divider() -> Schematic {
    let mut s = Schematic::default();
    let v = s.add(Kind::VSource, (0, 0), 0); // + at (0,-2), - at (0,2)
    let r1 = s.add(Kind::Resistor, (6, -6), 0); // 1 at (4,-6), 2 at (8,-6)
    let r2 = s.add(Kind::Resistor, (8, 0), 1); // rotated: pins vertical
    let g = s.add(Kind::Ground, (0, 6), 0);
    let _ = (v, r1, r2, g);
    s
}

fn wire(s: &mut Schematic, a: Pt, b: Pt) {
    s.wires.push(Wire { a, b });
}

#[test]
fn refdes_are_numbered_per_prefix() {
    let s = divider();
    let names: Vec<_> = s.components.iter().map(|c| c.refdes.as_str()).collect();
    assert_eq!(names, ["V1", "R1", "R2", "#PWR01"]);
}

#[test]
fn rotation_moves_pins() {
    let c = Component { refdes: "R1".into(), kind: Kind::Resistor, value: "1k".into(), pos: (10, 10), rot: 1, footprint: String::new() };
    let pins = c.pins();
    assert_eq!(pins[0].1, (10, 8));
    assert_eq!(pins[1].1, (10, 12));
}

#[test]
fn voltage_divider_netlist() {
    let mut s = divider();
    let r2 = s.components[2].pins();
    // V+ -> R1.1
    wire(&mut s, (0, -2), (0, -6));
    wire(&mut s, (0, -6), (4, -6));
    // R1.2 -> R2.top
    wire(&mut s, (8, -6), (r2[0].1 .0, r2[0].1 .1));
    // R2.bottom -> V- and GND
    wire(&mut s, (r2[1].1 .0, r2[1].1 .1), (8, 4));
    wire(&mut s, (8, 4), (0, 4));
    wire(&mut s, (0, 2), (0, 6));
    wire(&mut s, (0, 4), (0, 4)); // degenerate wire must be harmless
    let nets = extract(&s);
    let find = |r: &str, p: &str| nets.iter().find(|n| n.pins.iter().any(|x| x.refdes == r && x.pin == p)).unwrap().name.clone();
    assert_eq!(find("V1", "-"), "GND");
    assert_eq!(find("R2", "2"), "GND");
    assert_eq!(find("V1", "+"), find("R1", "1"));
    assert_eq!(find("R1", "2"), find("R2", "1"));
    assert_ne!(find("R1", "1"), find("R1", "2"));
    assert_eq!(nets.len(), 3);
    assert!(erc(&s).is_empty(), "{:?}", erc(&s));
}

#[test]
fn t_junction_connects_but_crossing_does_not() {
    let mut s = Schematic::default();
    s.add(Kind::Resistor, (0, 0), 0); // pins (-2,0) (2,0)
    s.add(Kind::Resistor, (0, 6), 0); // pins (-2,6) (2,6)
    s.add(Kind::Resistor, (10, 3), 1); // pins (10,1) (10,5)
    // vertical wire x=-2 from R1.1 down to R2.1 : endpoint-to-pin connection
    wire(&mut s, (-2, 0), (-2, 6));
    // horizontal wire from R1.2 right; vertical wire from R3.1 passes through its interior => T junction
    wire(&mut s, (2, 0), (12, 0));
    wire(&mut s, (10, 0), (10, 1));
    // R2.2 wire crossing the R3 vertical wire without touching a pin/endpoint
    wire(&mut s, (2, 6), (14, 6));
    wire(&mut s, (12, 3), (12, 8)); // crosses y=6 at (12,6), but its endpoints are not on that wire... except it passes through
    let nets = extract(&s);
    let net_of = |r: &str, p: &str| nets.iter().position(|n| n.pins.iter().any(|x| x.refdes == r && x.pin == p)).unwrap();
    assert_eq!(net_of("R1", "1"), net_of("R2", "1"));
    assert_eq!(net_of("R1", "2"), net_of("R3", "1")); // T junction
    assert_ne!(net_of("R2", "2"), net_of("R3", "2")); // crossing is not a connection
}

#[test]
fn labels_merge_nets_by_name() {
    let mut s = Schematic::default();
    s.add(Kind::Resistor, (0, 0), 0);
    s.add(Kind::Resistor, (0, 10), 0);
    s.labels.push(Label { pos: (2, 0), text: "SIG".into() });
    s.labels.push(Label { pos: (-2, 10), text: "SIG".into() });
    let nets = extract(&s);
    let sig = nets.iter().find(|n| n.name == "SIG").unwrap();
    assert_eq!(sig.pins.len(), 2);
}

#[test]
fn erc_reports_floating_pins_duplicates_and_short() {
    let mut s = Schematic::default();
    s.add(Kind::Resistor, (0, 0), 0);
    let r = s.add(Kind::Resistor, (0, 10), 0);
    s.components[r].refdes = "R1".into(); // duplicate
    let g = s.add(Kind::Ground, (-2, 0), 0);
    let _ = g;
    let p = s.add(Kind::Power, (-2, 0), 0); // same spot as GND => short
    let _ = p;
    let msgs: Vec<_> = erc(&s).into_iter().map(|p| p.message).collect();
    assert!(msgs.iter().any(|m| m.contains("重複")));
    assert!(msgs.iter().any(|m| m.contains("未接続")));
    assert!(msgs.iter().any(|m| m.contains("短絡")));
}

#[test]
fn spice_deck() {
    let mut s = divider();
    let r2 = s.components[2].pins();
    s.components[0].value = "9".into();
    wire(&mut s, (0, -2), (0, -6));
    wire(&mut s, (0, -6), (4, -6));
    wire(&mut s, (8, -6), r2[0].1);
    wire(&mut s, r2[1].1, (8, 4));
    wire(&mut s, (8, 4), (0, 4));
    wire(&mut s, (0, 2), (0, 6));
    let d = to_spice(&s, "divider");
    assert!(d.contains("V1 N$1 0 DC 9") || d.contains("V1 N_1 0 DC 9"), "{d}");
    assert!(d.contains("R2 ") && d.trim_end().ends_with(".end"));
}
