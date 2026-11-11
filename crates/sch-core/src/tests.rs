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
    let c = Component { refdes: "R1".into(), kind: Kind::Resistor, value: "1k".into(), pos: (10, 10), rot: 1, footprint: String::new(), custom: None, sim_only: false, pin_names: vec![], theta: 0.0, tc_ppm: 0.0, cth: 0.0, sub: None, shops: Default::default() };
    let pins = c.pins();
    assert_eq!(pins[0].1, (10, 8));
    assert_eq!(pins[1].1, (10, 12));
}

#[test]
fn an_unnamed_net_never_takes_the_name_a_label_gave_to_another_net() {
    // three resistors, nothing wired: three unnamed nets for the pins on one side, and a label called "N$1" on R3's pin
    let mut s = Schematic::default();
    for k in 0..3 {
        s.add(Kind::Resistor, (10 * k, 0), 0);
    }
    let pin = |s: &Schematic, r: &str, n: &str| s.components.iter().find(|c| c.refdes == r).unwrap().pins().into_iter().find(|p| p.0 == n).unwrap().1;
    let at = pin(&s, "R3", "1");
    s.labels.push(Label { pos: at, text: "N$1".into() });
    let nets = extract(&s);
    let name_of = |r: &str, p: &str| nets.iter().find(|n| n.pins.iter().any(|x| x.refdes == r && x.pin == p)).unwrap().name.clone();
    assert_eq!(name_of("R3", "1"), "N$1");
    let mut names: Vec<String> = nets.iter().map(|n| n.name.clone()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), nets.len(), "every net has its own name: {names:?}");
}

#[test]
fn the_pin_names_of_a_header_are_looked_up_by_pin_number_and_saved_only_when_there_are_some() {
    let mut s = Schematic::default();
    let i = s.add(Kind::Header(3), (0, 0), 0);
    assert!(!serde_json::to_string(&s.components[i]).unwrap().contains("pin_names"), "nothing is written when there are no names");
    s.components[i].pin_names = vec!["F+".into(), String::new(), "GND".into()];
    let c = &s.components[i];
    assert_eq!((c.pin_name("1"), c.pin_name("2"), c.pin_name("3"), c.pin_name("4"), c.pin_name("0")), (Some("F+"), None, Some("GND"), None, None));
    let back: Component = serde_json::from_str(&serde_json::to_string(c).unwrap()).unwrap();
    assert_eq!(back.pin_names, c.pin_names);
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
    let pin = |n: &str, name: &str, side| SymPin { number: n.into(), name: name.into(), side, sim_only: false, gap: 0 };
    CustomSymbol {
        ratings: None,
        shops: Default::default(),
        programming: vec![],
        glyph: String::new(),
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
            origin: None,
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

/// A gap leaves empty places before its pin: the side gets longer and the pins after it move down.
#[test]
fn a_gap_leaves_empty_places_on_its_side() {
    let mut s = opamp();
    // IN- with one empty place before it: the left side has three places (IN+, gap, IN-)
    s.pins[1].gap = 1;
    let l = s.layout();
    let tip = |n: &str| l.pins.iter().find(|p| p.number == n).unwrap().tip;
    assert_eq!((l.hw, l.hh), (2, 3));
    assert_eq!(tip("3"), (-4, -2));
    assert_eq!(tip("2"), (-4, 2));
    assert_eq!(tip("1"), (4, 0), "the other sides keep their places");
    // a gap is left out of the saved symbol when it is 0
    assert!(!serde_json::to_string(&opamp()).unwrap().contains("gap"));
    assert!(serde_json::to_string(&s).unwrap().contains("\"gap\":1"));
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
fn spice_writes_the_capacitance_left_after_the_dc_bias() {
    let mut s = Schematic::default();
    s.add(Kind::Capacitor, (0, 0), 0);
    s.components[0].value = "22u BIAS=61".into();
    let d = to_spice(&s, "t");
    let line = d.lines().find(|l| l.starts_with("C1 ")).unwrap_or_else(|| panic!("{d}"));
    let v: f64 = line.split_whitespace().last().unwrap().parse().unwrap();
    assert!((v - 22e-6 * 0.39).abs() < 1e-12, "{line}");
    assert!(d.contains("61% lost to the DC bias"), "{d}");
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
fn ic_power_pin_behind_a_series_mosfet_counts_as_supplied() {
    let pin = |n: &str, name: &str, side| SymPin { number: n.into(), name: name.into(), side, sim_only: false, gap: 0 };
    let fet = CustomSymbol {
        ratings: None,
        shops: Default::default(),
        programming: vec![],
        glyph: "pmos".into(),
        name: "AO3401A".into(),
        prefix: "Q".into(),
        pins: vec![pin("1", "G", PinSide::Left), pin("2", "S", PinSide::Top), pin("3", "D", PinSide::Bottom)],
        sim: Some(SimModel {
            kind: SimKind::Model,
            element: "M".into(),
            name: "AO3401A".into(),
            text: ".model AO3401A PMOS(VTO=-1)".into(),
            pins: vec!["3".into(), "1".into(), "2".into()],
            params: Default::default(),
            limits: vec![],
            origin: None,
        }),
    };
    let mut s = Schematic::default();
    let u = s.add_custom(opamp(), "LM358", "SOIC-8", (40, 0), 0);
    let q = s.add_custom(fet, "AO3401A", "SOT-23_MOS", (20, 0), 0);
    let v = s.add(Kind::VSource, (0, 0), 0);
    s.add(Kind::Ground, (0, 8), 0);
    let pin = |s: &Schematic, i: usize, name: &str| s.components[i].pins().into_iter().find(|p| p.0 == name).unwrap().1;
    // battery + -> drain, source -> the IC's V+ (by labels)
    s.labels.push(Label { pos: pin(&s, v, "+"), text: "BATJ".into() });
    s.labels.push(Label { pos: pin(&s, q, "3"), text: "BATJ".into() });
    s.labels.push(Label { pos: pin(&s, q, "2"), text: "BATP".into() });
    s.labels.push(Label { pos: pin(&s, u, "8"), text: "BATP".into() });
    assert!(!has(&erc(&s), "電源ピン 8"), "{:?}", erc(&s));
    // a switch whose other side is not supplied does not feed anything
    s.labels[0].text = "OTHER".into();
    assert!(has(&erc(&s), "電源ピン 8"), "{:?}", erc(&s));
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

#[test]
fn old_two_lead_glyph_sheet_gets_short_leads() {
    // a version-0 sheet: electrolytic cap at (10,10) whose pins used to sit at +-4, wires on them
    let pin = |n: &str, side| SymPin { number: n.into(), name: n.into(), side, sim_only: false, gap: 0 };
    let sym = CustomSymbol {
        name: "CE".into(),
        prefix: "C".into(),
        pins: vec![pin("1", PinSide::Left), pin("2", PinSide::Right)],
        sim: None,
        ratings: None,
        shops: Default::default(),
        programming: vec![],
        glyph: "electrolytic".into(),
    };
    let c = Component { refdes: "C1".into(), kind: Kind::Custom, value: "100u".into(), pos: (10, 10), rot: 0, footprint: String::new(), custom: Some(sym), sim_only: false, pin_names: vec![], theta: 0.0, tc_ppm: 0.0, cth: 0.0, sub: None, shops: Default::default() };
    let mut s = Schematic::default();
    s.components.push(c);
    assert_eq!(s.components[0].pins()[0].1, (8, 10)); // new leads are 2 long
    s.layout_version = 0;
    s.wires = vec![Wire { a: (6, 10), b: (0, 10) }, Wire { a: (14, 10), b: (14, 20) }];
    s.migrate();
    assert_eq!(s.layout_version, LAYOUT_VERSION);
    assert!(s.wires.contains(&Wire { a: (8, 10), b: (0, 10) })); // straight wire: end moved
    assert!(s.wires.contains(&Wire { a: (14, 10), b: (14, 20) })); // bent: kept ...
    assert!(s.wires.contains(&Wire { a: (12, 10), b: (14, 10) })); // ... with a stub
}

#[test]
fn a_pasted_fragment_is_moved_and_its_parts_get_new_reference_designators() {
    let mut s = Schematic::default();
    let r1 = s.add(Kind::Resistor, (10, 10), 0);
    let r2 = s.add(Kind::Resistor, (10, 20), 0);
    s.wires.push(Wire { a: (12, 10), b: (12, 14) });
    s.labels.push(Label { pos: (8, 10), text: "IN".into() });
    let f = s.copy_fragment(&[r1], &[0], &[0]);
    assert_eq!((f.components.len(), f.wires.len(), f.labels.len()), (1, 1, 1));
    let p = s.paste_fragment(&f, 20, 0);
    // R1 and R2 exist: the copy of R1 is R3, and it is where it was plus the move
    let c = &s.components[p.components[0]];
    assert_eq!(c.refdes, "R3");
    assert_eq!(c.pos, (30, 10));
    assert_eq!(s.wires[p.wires[0]], Wire { a: (32, 10), b: (32, 14) });
    assert_eq!(s.labels[p.labels[0]], Label { pos: (28, 10), text: "IN".into() });
    // the original is untouched, and a second paste goes on counting
    assert_eq!(s.components[r1].refdes, "R1");
    assert_eq!(s.components[r2].refdes, "R2");
    let q = s.paste_fragment(&f, 40, 0);
    assert_eq!(s.components[q.components[0]].refdes, "R4");
}

#[test]
fn a_fragment_of_library_parts_and_power_symbols_pastes_with_their_own_prefixes() {
    let mut s = Schematic::default();
    let u = s.add_custom(opamp(), "LM358", "SOIC_8", (10, 10), 1);
    let g = s.add(Kind::Ground, (30, 10), 0);
    let f = s.copy_fragment(&[u, g], &[], &[]);
    let p = s.paste_fragment(&f, 0, 30);
    let (cu, cg) = (&s.components[p.components[0]], &s.components[p.components[1]]);
    assert_eq!((cu.refdes.as_str(), cu.rot, cu.pos), ("U2", 1, (10, 40)));
    assert!(cu.custom.is_some() && cu.value == "LM358" && cu.footprint == "SOIC_8");
    assert!(cg.refdes.starts_with("#PWR"));
    assert_ne!(cg.refdes, s.components[g].refdes);
}

#[test]
fn copying_nothing_gives_an_empty_fragment_and_bad_indices_are_ignored() {
    let s = Schematic::default();
    assert!(s.copy_fragment(&[], &[], &[]).is_empty());
    assert!(s.copy_fragment(&[3], &[2], &[1]).is_empty());
}

#[test]
fn the_simulation_only_mark_is_saved_only_when_set() {
    let mut s = Schematic::default();
    let i = s.add(Kind::Resistor, (10, 10), 0);
    let plain = serde_json::to_string(&s).unwrap();
    assert!(!plain.contains("sim_only"), "an unmarked part writes nothing: {plain}");
    s.components[i].sim_only = true;
    let marked = serde_json::to_string(&s).unwrap();
    assert!(marked.contains("\"sim_only\":true"), "{marked}");
    let back: Schematic = serde_json::from_str(&marked).unwrap();
    assert!(back.components[0].sim_only);
    // a file from before the mark existed reads as not marked
    let old: Schematic = serde_json::from_str(&plain).unwrap();
    assert!(!old.components[0].sim_only);
}

#[test]
fn thermal_links_are_kept_in_the_file_and_replaced_in_either_order() {
    let mut s = Schematic::default();
    assert!(!serde_json::to_string(&s).unwrap().contains("thermal_links"), "an empty list is not written");
    s.set_thermal_link("Q1", "RS1", 150.0);
    s.set_thermal_link("rs1", "q1", 200.0);
    assert_eq!(s.thermal_links.len(), 1);
    assert_eq!(s.thermal_links[0].2, 200.0);
    let back: Schematic = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.thermal_links, s.thermal_links);
    s.set_thermal_link("Q1", "RS1", 0.0);
    assert!(s.thermal_links.is_empty());
}

/// A wire joins every point that lies on it, along a row, along a column and slanted; an end that rests on the middle of
/// another wire is not dangling.
#[test]
fn a_wire_joins_the_points_on_it_in_every_direction() {
    let mut s = Schematic::default();
    s.add(Kind::Resistor, (0, 0), 0); // R1: pins (-2,0) and (2,0)
    s.add(Kind::Resistor, (-4, 4), 1); // R2: vertical, pins (-4,2) and (-4,6)
    s.add(Kind::Resistor, (20, 0), 0); // R3: pins (18,0) and (22,0)
    s.add(Kind::Resistor, (30, 10), 1); // R4: pins (30,8) and (30,12)
    s.add(Kind::Resistor, (40, 10), 0); // R5: pins (38,10) and (42,10)
    // slanted from (-4,2) to (0,-2): passes through R1's pin (-2,0)
    wire(&mut s, (-4, 2), (0, -2));
    // along a row from R3's pin (18,0) to (26,0), then along a column down to (26,10)
    wire(&mut s, (18, 0), (26, 0));
    wire(&mut s, (26, 0), (26, 10));
    // a wire from (22,6) to (26,6): one end rests on the middle of the column, the other touches nothing
    wire(&mut s, (22, 6), (26, 6));
    let net = |s: &Schematic, p: Pt| net_at(s, p).expect("a net");
    assert_eq!(net(&s, (-2, 0)), net(&s, (-4, 2)), "the slanted wire joins R1's pin and R2's pin");
    assert_ne!(net(&s, (-2, 0)), net(&s, (2, 0)));
    // the row and the column are one net; the point (26,10) at the end of the column is on it
    assert_eq!(net(&s, (18, 0)), net(&s, (26, 10)));
    let problems = erc(&s);
    let dangling: Vec<Pt> = problems.iter().filter(|p| p.message.contains("線の端")).filter_map(|p| p.at).collect();
    assert!(dangling.contains(&(0, -2)), "the far end of the slanted wire dangles: {dangling:?}");
    assert!(!dangling.contains(&(-4, 2)), "an end on R2's pin does not: {dangling:?}");
    assert!(!dangling.contains(&(26, 6)), "an end on the middle of the column does not: {dangling:?}");
    assert!(dangling.contains(&(22, 6)), "the other end of the T wire dangles: {dangling:?}");
}

#[test]
fn bus_labels_are_read_in_every_notation() {
    assert_eq!(bus_members("D[0..3]").unwrap(), ["D0", "D1", "D2", "D3"]);
    assert_eq!(bus_members("D[3:0]").unwrap(), ["D3", "D2", "D1", "D0"]);
    assert_eq!(bus_members("ADDR_[1-2]").unwrap(), ["ADDR_1", "ADDR_2"]);
    assert_eq!(bus_member("PB[0..7]", 5).as_deref(), Some("PB5"));
    assert_eq!(bus_member("PB[0..7]", 8), None);
    // a single bracketed number or any other text is an ordinary net name
    for t in ["D[3]", "D", "[0..3]", "D[a..b]", "D[0..999]", "D[0..3"] {
        assert!(!is_bus_label(t), "{t}");
    }
}

/// Two headers joined by labels D0..D3, a bus line with the bus label on it.
fn bus_board(range: &str, with_d3: bool) -> Schematic {
    let mut s = Schematic::default();
    let j1 = s.add(Kind::Header(4), (0, 0), 0);
    let j2 = s.add(Kind::Header(4), (30, 0), 0);
    for k in 0..4 {
        for (j, name) in [(j1, "D"), (j2, "D")] {
            if k == 3 && !with_d3 && j == j2 {
                continue;
            }
            let at = s.components[j].pins()[k].1;
            s.labels.push(Label { pos: at, text: format!("{name}{k}") });
        }
    }
    let g = s.add(Kind::Ground, (15, 12), 0);
    let _ = g;
    s.buses.push(Wire { a: (10, 20), b: (20, 20) });
    s.labels.push(Label { pos: (12, 20), text: range.into() });
    s
}

fn bus_messages(s: &Schematic) -> Vec<String> {
    erc(s).into_iter().map(|p| p.message).filter(|m| m.contains("バス") || m.contains("範囲")).collect()
}

#[test]
fn a_bus_label_names_no_net_and_a_complete_bus_is_quiet() {
    let s = bus_board("D[0..3]", true);
    assert!(extract(&s).iter().all(|n| !n.name.contains('[')), "{:?}", extract(&s).iter().map(|n| &n.name).collect::<Vec<_>>());
    assert!(extract(&s).iter().any(|n| n.name == "D2" && n.pins.len() == 2));
    assert_eq!(bus_messages(&s), Vec::<String>::new());
}

#[test]
fn the_bus_checks_find_a_missing_member_a_half_wired_one_a_stray_label_and_a_range_that_is_too_small() {
    // the bus says D0..D4 but D4 is nowhere
    let m = bus_messages(&bus_board("D[0..4]", true));
    assert!(m.iter().any(|x| x.contains("D4") && x.contains("どこにも")), "{m:?}");
    // D3 has a pin on one header only
    let m = bus_messages(&bus_board("D[0..3]", false));
    assert!(m.iter().any(|x| x.contains("D3") && x.contains("1つ")), "{m:?}");
    // D3 exists but the bus stops at D2
    let m = bus_messages(&bus_board("D[0..2]", true));
    assert!(m.iter().any(|x| x.contains("D3") && x.contains("範囲の外")), "{m:?}");
    // the bus label is off the line
    let mut s = bus_board("D[0..3]", true);
    s.labels.last_mut().unwrap().pos = (12, 30);
    assert!(bus_messages(&s).iter().any(|x| x.contains("バスの線の上にありません")));
    // two widths of one bus
    let mut s = bus_board("D[0..3]", true);
    s.labels.push(Label { pos: (14, 20), text: "D[0..1]".into() });
    assert!(bus_messages(&s).iter().any(|x| x.contains("幅が食い違")));
}

#[test]
fn buses_follow_their_sheet_and_survive_a_save() {
    let mut s = bus_board("D[0..3]", true);
    s.add_sheet();
    assert!(s.buses.is_empty(), "the new sheet has none");
    s.switch_sheet(0);
    assert_eq!(s.buses.len(), 1);
    assert_eq!(s.flat().buses.len(), 1);
    let back: Schematic = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back.flat().buses.len(), 1);
    assert_eq!(back, s);
}

#[test]
fn a_part_numbers_of_its_own_win_over_those_of_its_library_part_and_are_not_written_when_empty() {
    let mut c = Component { refdes: "R1".into(), kind: Kind::Resistor, value: "1k".into(), pos: (10, 10), rot: 0, footprint: String::new(), custom: None, sim_only: false, pin_names: vec![], theta: 0.0, tc_ppm: 0.0, cth: 0.0, sub: None, shops: Default::default() };
    assert!(c.shop_numbers().is_empty());
    assert!(!serde_json::to_string(&c).unwrap().contains("shops"), "an empty map must not be written");
    c.shops.insert("digikey".into(), " RC0805FR-071KL ".into());
    assert_eq!(c.shop_numbers()["digikey"], "RC0805FR-071KL");
    let back: Component = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
    assert_eq!(back.shops, c.shops);
    // an old file without the field still reads
    let old = serde_json::to_string(&Component { shops: Default::default(), ..c.clone() }).unwrap();
    assert!(serde_json::from_str::<Component>(&old).is_ok());
    // a blank number takes the library part's number away
    let mut sym = CustomSymbol { name: "X".into(), prefix: "R".into(), pins: vec![], sim: None, ratings: None, shops: Default::default(), programming: vec![], glyph: String::new() };
    sym.shops.insert("lcsc".into(), "C1".into());
    sym.shops.insert("digikey".into(), "X".into());
    c.custom = Some(sym);
    c.shops.insert("lcsc".into(), "  ".into());
    let m = c.shop_numbers();
    assert_eq!(m.get("lcsc"), None);
    assert_eq!(m["digikey"], "RC0805FR-071KL");
}
