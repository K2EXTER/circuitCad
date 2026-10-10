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
    let c = Component { refdes: "R1".into(), kind: Kind::Resistor, value: "1k".into(), pos: (10, 10), rot: 1, footprint: String::new(), custom: None };
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

fn opamp() -> CustomSymbol {
    let pin = |n: &str, name: &str, side| SymPin { number: n.into(), name: name.into(), side };
    CustomSymbol {
        name: "LM358".into(),
        prefix: "U".into(),
        pins: vec![
            pin("3", "IN+", PinSide::Left),
            pin("2", "IN-", PinSide::Left),
            pin("1", "OUT", PinSide::Right),
            pin("8", "V+", PinSide::Top),
            pin("4", "V-", PinSide::Bottom),
            pin("5", "NC", PinSide::Bottom),
        ],
        sim: Some(SimModel {
            kind: SimKind::Subckt,
            element: String::new(),
            name: "LM358".into(),
            text: ".subckt LM358 1 2 3 4 5\n* body\n.ends LM358".into(),
            pins: vec!["3".into(), "2".into(), "1".into(), "8".into(), "4".into()],
            params: Default::default(),
            limits: vec![],
        }),
    }
}

#[test]
fn custom_symbol_layout_is_on_grid() {
    let l = opamp().layout();
    let tip = |n: &str| l.pins.iter().find(|p| p.number == n).unwrap().tip;
    assert_eq!((l.hw, l.hh), (2, 2));
    assert_eq!(tip("3"), (-4, -1)); // two left pins: y = -1, +1
    assert_eq!(tip("2"), (-4, 1));
    assert_eq!(tip("1"), (4, 0));
    assert_eq!(tip("8"), (0, -4));
    assert_eq!(tip("4"), (-1, 4)); // two bottom pins
    assert_eq!(tip("5"), (1, 4));
}

#[test]
fn custom_part_pins_follow_rotation_and_nets() {
    let mut s = Schematic::default();
    let u = s.add_custom(opamp(), "LM358", "SOIC-8", (10, 10), 0);
    assert_eq!(s.components[u].refdes, "U1");
    assert_eq!(s.components[u].pins().iter().find(|p| p.0 == "1").unwrap().1, (14, 10));
    s.components[u].rot = 1;
    assert_eq!(s.components[u].pins().iter().find(|p| p.0 == "1").unwrap().1, (10, 14));
    s.components[u].rot = 0;
    let r = s.add(Kind::Resistor, (20, 10), 0); // pin 1 at (18,10)
    let _ = r;
    wire(&mut s, (14, 10), (18, 10));
    let nets = extract(&s);
    assert!(nets.iter().any(|n| n.pins.contains(&PinRef { refdes: "U1".into(), pin: "1".into() })
        && n.pins.contains(&PinRef { refdes: "R1".into(), pin: "1".into() })));
    // NC pin is not reported, other floating pins are
    let msgs: Vec<_> = erc(&s).into_iter().map(|p| p.message).collect();
    assert!(!msgs.iter().any(|m| m.contains("U1 ピン5")), "{msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("U1 ピン3")));
}

#[test]
fn spice_uses_part_model_once() {
    let mut s = Schematic::default();
    s.add_custom(opamp(), "LM358", "SOIC-8", (10, 10), 0);
    s.add_custom(opamp(), "LM358", "SOIC-8", (30, 10), 0);
    let d = to_spice(&s, "t");
    assert_eq!(d.matches(".subckt LM358").count(), 1);
    assert!(d.contains("XU1 ") && d.contains("XU2 "));
    // node order follows sim.pins: 3 2 1 8 4
    let line = d.lines().find(|l| l.starts_with("XU1 ")).unwrap();
    assert!(line.ends_with(" LM358") && line.split_whitespace().count() == 7, "{line}");
}

#[test]
fn spice_without_model_leaves_a_comment() {
    let mut s = Schematic::default();
    let mut sym = opamp();
    sym.sim = None;
    s.add_custom(sym, "LM358", "SOIC-8", (10, 10), 0);
    assert!(to_spice(&s, "t").contains("シミュレーションモデルなし"));
}

#[test]
fn erc_warns_when_a_power_net_has_no_source() {
    // VCC symbol on a resistor only: nothing drives the net
    let mut s = Schematic::default();
    s.add(Kind::Resistor, (0, 0), 0); // pins (-2,0) (2,0)
    s.add(Kind::Power, (-2, 0), 0);
    s.add(Kind::Ground, (2, 0), 0);
    let msgs: Vec<_> = erc(&s).into_iter().map(|p| p.message).collect();
    assert!(msgs.iter().any(|m| m.contains("VCC") && m.contains("電圧源")), "{msgs:?}");

    // add a voltage source whose + terminal reaches the VCC net: the warning goes away
    let v = s.add(Kind::VSource, (-10, 4), 0); // + at (-10,2), - at (-10,6)
    let _ = v;
    wire(&mut s, (-10, 2), (-10, 0));
    wire(&mut s, (-10, 0), (-2, 0));
    let msgs: Vec<_> = erc(&s).into_iter().map(|p| p.message).collect();
    assert!(!msgs.iter().any(|m| m.contains("電圧源")), "{msgs:?}");
}

#[test]
fn spice_expands_capacitor_and_inductor_parasitics() {
    let mut s = Schematic::default();
    s.add(Kind::Capacitor, (0, 0), 0);
    s.components[0].value = "100n ESR=20m ESL=5n".into();
    s.add(Kind::Inductor, (10, 0), 0);
    s.components[1].value = "10u DCR=0.1".into();
    s.add(Kind::Resistor, (20, 0), 0);
    let d = to_spice(&s, "t");
    assert!(d.contains("RC1_ESR "), "{d}");
    assert!(d.contains("LC1_ESL "), "{d}");
    assert!(d.lines().any(|l| l.starts_with("C1 C1_esl ") && l.ends_with(" 100n")), "{d}");
    assert!(d.lines().any(|l| l.starts_with("L1 ") && l.contains(" L1_dcr 10u")), "{d}");
    assert!(d.contains("RL1_DCR L1_dcr "), "{d}");
    // a plain value is untouched
    assert!(d.lines().any(|l| l.starts_with("R1 ") && l.ends_with(" 10k")), "{d}");
}

#[test]
fn net_at_finds_the_net_under_a_point() {
    let mut s = divider();
    let r2 = s.components[2].pins();
    wire(&mut s, (0, -2), (0, -6));
    wire(&mut s, (0, -6), (4, -6));
    wire(&mut s, (8, -6), (r2[0].1 .0, r2[0].1 .1));
    wire(&mut s, (r2[1].1 .0, r2[1].1 .1), (8, 4));
    wire(&mut s, (8, 4), (0, 4));
    wire(&mut s, (0, 2), (0, 6));
    let nets = extract(&s);
    let name_of = |r: &str, p: &str| nets.iter().find(|n| n.pins.iter().any(|x| x.refdes == r && x.pin == p)).unwrap().name.clone();
    // a pin, a point in the middle of a wire, and a ground symbol all resolve to their net
    assert_eq!(net_at(&s, (4, -6)).unwrap(), name_of("R1", "1"));
    assert_eq!(net_at(&s, (0, -4)).unwrap(), name_of("V1", "+"));
    assert_eq!(net_at(&s, (4, 4)).unwrap(), "GND");
    // an empty spot, and probing must not change the schematic or the other nets
    assert_eq!(net_at(&s, (20, 20)), None);
    assert_eq!(extract(&s).len(), nets.len());
}

#[test]
fn net_at_keeps_label_names() {
    let mut s = Schematic::default();
    s.add(Kind::Resistor, (0, 0), 0);
    s.labels.push(Label { pos: (-2, 0), text: "SIG".into() });
    assert_eq!(net_at(&s, (-2, 0)).unwrap(), "SIG");
}

#[test]
fn every_erc_finding_explains_itself() {
    let mut s = divider();
    s.components[1].refdes = "V1".into();
    for p in erc(&s) {
        assert!(p.hint.chars().count() > 20, "no explanation for: {}", p.message);
    }
    assert!(!erc(&s).is_empty());
}

#[test]
fn a_wire_that_stops_short_of_a_pin_is_found_and_pointed_out() {
    let mut s = divider();
    let r1 = s.components[1].pins();
    // the wire from the source ends one grid step before R1's pin 1
    wire(&mut s, (0, -2), (0, -6));
    wire(&mut s, (0, -6), (r1[0].1.0 - 1, r1[0].1.1));
    let found = erc(&s);
    let dangling = found.iter().find(|p| p.message.contains("線の端")).expect("dangling end reported");
    assert_eq!(dangling.severity, Severity::Warning);
    assert_eq!(dangling.at, Some((r1[0].1.0 - 1, r1[0].1.1)));
    let pin = found.iter().find(|p| p.message.contains("R1 ピン1")).expect("pin reported");
    assert_eq!(pin.severity, Severity::Error);
    assert!(pin.hint.contains("近く"), "the hint names the nearby wire end: {}", pin.hint);
}

#[test]
fn a_source_with_both_terminals_on_one_net_is_a_short() {
    let mut s = divider();
    wire(&mut s, (0, -2), (0, 2));
    let found = erc(&s);
    let short = found.iter().find(|p| p.message.contains("電源 V1") && p.message.contains("短絡")).expect("source short");
    assert_eq!(short.severity, Severity::Error);
    assert_eq!(short.at, Some((0, 0)));
}

#[test]
fn a_clean_circuit_has_no_findings() {
    let mut s = divider();
    let r2 = s.components[2].pins();
    for (a, b) in [((0, -2), (0, -6)), ((0, -6), (4, -6)), ((8, -6), r2[0].1), (r2[1].1, (8, 4)), ((8, 4), (0, 4)), ((0, 2), (0, 6))] {
        wire(&mut s, a, b);
    }
    assert_eq!(erc(&s), vec![]);
}

#[test]
fn a_missing_ground_is_explained() {
    let mut s = Schematic::default();
    s.add(Kind::Resistor, (0, 0), 0);
    let p = erc(&s).into_iter().find(|p| p.message.contains("GND")).unwrap();
    assert!(p.hint.contains("基準"));
    assert_eq!(p.at, None);
}

#[test]
fn a_label_named_gnd_counts_as_the_reference() {
    let mut s = Schematic::default();
    s.add(Kind::Resistor, (0, 0), 0);
    assert!(erc(&s).iter().any(|p| p.message.contains("GNDシンボル")));
    s.labels.push(Label { pos: (-2, 0), text: "GND".into() });
    assert!(!erc(&s).iter().any(|p| p.message.contains("GNDシンボル")));
}

fn wire_all(s: &mut Schematic, pairs: &[(Pt, Pt)]) {
    for &(a, b) in pairs {
        wire(s, a, b);
    }
}

fn has(found: &[Problem], needle: &str) -> bool {
    found.iter().any(|p| p.message.contains(needle))
}

#[test]
fn an_led_straight_across_the_supply_is_flagged_until_a_resistor_is_added() {
    let mut s = Schematic::default();
    s.add(Kind::VSource, (0, 0), 0); // + (0,-2), - (0,2)
    let d = s.add(Kind::Led, (8, 0), 1); // vertical: A at top, K at bottom
    s.add(Kind::Ground, (0, 6), 0);
    let dp = s.components[d].pins();
    wire_all(&mut s, &[((0, -2), (0, -6)), ((0, -6), (8, -6)), ((8, -6), dp[0].1), (dp[1].1, (8, 4)), ((8, 4), (0, 4)), ((0, 2), (0, 4)), ((0, 4), (0, 6))]);
    assert!(has(&erc(&s), "電流を制限する抵抗"), "{:?}", erc(&s));

    // with a series resistor in the top wire the warning goes away
    s.wires.retain(|w| !(w.a == (0, -6) && w.b == (8, -6)));
    let r = s.add(Kind::Resistor, (4, -6), 0); // pins at (2,-6) and (6,-6)
    let rp = s.components[r].pins();
    wire_all(&mut s, &[((0, -6), rp[0].1), (rp[1].1, (8, -6))]);
    assert!(!has(&erc(&s), "電流を制限する抵抗"), "{:?}", erc(&s));
}

#[test]
fn a_part_with_no_path_to_ground_is_reported_once() {
    let mut s = divider();
    let r2 = s.components[2].pins();
    wire_all(&mut s, &[((0, -2), (0, -6)), ((0, -6), (4, -6)), ((8, -6), r2[0].1), (r2[1].1, (8, 4)), ((8, 4), (0, 4)), ((0, 2), (0, 6))]);
    assert!(!has(&erc(&s), "GND とつながっていません"));
    // an island: two resistors wired to each other only
    let a = s.add(Kind::Resistor, (20, 0), 0);
    let b = s.add(Kind::Resistor, (30, 0), 0);
    let (pa, pb) = (s.components[a].pins(), s.components[b].pins());
    wire_all(&mut s, &[(pa[1].1, pb[0].1), (pa[0].1, (16, 0)), (pb[1].1, (34, 0))]);
    s.labels.push(Label { pos: (16, 0), text: "X".into() });
    s.labels.push(Label { pos: (34, 0), text: "X".into() });
    let found = erc(&s);
    assert_eq!(found.iter().filter(|p| p.message.contains("GND とつながっていません")).count(), 1, "{found:?}");
}

#[test]
fn labels_that_differ_only_in_case_are_a_warning() {
    let mut s = divider();
    s.labels.push(Label { pos: (0, -2), text: "VCC".into() });
    s.labels.push(Label { pos: (0, -6), text: "Vcc".into() });
    let found = erc(&s);
    let p = found.iter().find(|p| p.message.contains("大文字・小文字")).expect("case warning");
    assert_eq!(p.severity, Severity::Warning);
    s.labels[1].text = "VCC".into();
    assert!(!has(&erc(&s), "大文字・小文字"));
}

#[test]
fn ic_power_pins_must_reach_a_supply_but_a_regulator_output_counts() {
    let mut s = Schematic::default();
    let u = s.add_custom(opamp(), "LM358", "SOIC-8", (20, 0), 0);
    let r = s.add(Kind::Resistor, (20, -14), 1);
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.add(Kind::Ground, (0, 8), 0);
    let pin = |s: &Schematic, i: usize, name: &str| s.components[i].pins().into_iter().find(|p| p.0 == name).unwrap().1;
    let (vplus, rpin) = (pin(&s, u, "8"), pin(&s, r, "2"));
    // V+ only goes to a resistor: nothing powers it
    wire(&mut s, vplus, rpin);
    assert!(has(&erc(&s), "電源ピン 8"), "{:?}", erc(&s));
    // wiring V+ to the source's + pin fixes it
    s.wires.clear();
    let src = pin(&s, v, "+");
    wire(&mut s, vplus, (vplus.0, src.1));
    wire(&mut s, (vplus.0, src.1), src);
    assert!(!has(&erc(&s), "電源ピン 8"), "{:?}", erc(&s));
}
