use super::*;
use super::lay::pair_up;
use super::sections::strip_of;

/// With `chips_back` on a stripboard, a chip goes across two strips one above the other that carry its two nets; the board
/// checks clean and beats the one with every part on top. The active filter's chips find their places too since the layout puts
/// each chip's two strips side by side (before, none did and it was laid out all on top).
#[test]
fn a_stripboard_chip_on_the_copper_side_sits_across_two_strips() {
    let load = |name: &str| -> Schematic {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    };
    for name in ["attiny_blinker.schjson", "active_filter.schjson"] {
        let s = load(name);
        let front = strip_layout(&s);
        let l = strip_layout_with(&s, StripOptions { chips_back: true, ..Default::default() });
        assert!(strip_check(&s, &l).is_empty(), "{name}: {:?}", strip_check(&s, &l));
        assert!(strip_unjoined(&s, &l).is_empty());
        let backs: Vec<&StripPart> = l.parts.iter().filter(|p| matches!(p.mount, Mount::Back { .. })).collect();
        assert!(!backs.is_empty(), "{name}");
        let nets = extract(&s.flat());
        let net = |r: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|q| q.refdes == r && q.pin == pin)).map(|n| n.name.clone()).unwrap();
        for p in &backs {
            let (a, b) = (p.leads[0].1, p.leads[1].1);
            assert!(a.1 == b.1 && a.0.abs_diff(b.0) == 1, "{name} {}: one above the other", p.refdes);
            assert_ne!(net(&p.refdes, &p.leads[0].0), net(&p.refdes, &p.leads[1].0));
            assert!(!l.cuts.contains(&a) && !l.cuts.contains(&b));
        }
        // holes + 10 per wire on top - 5 per chip on the back
        let score = |l: &StripLayout| (l.rows as usize * l.cols as usize + 10 * l.links.len()) as i64 - 5 * l.parts.iter().filter(|p| matches!(p.mount, Mount::Back { .. })).count() as i64;
        assert!(score(&l) < score(&front), "{name}: {} < {}", score(&l), score(&front));
    }
}

/// A band's strips are reordered so that each pair (a chip on the copper side) sits side by side, in chains, ground kept last.
#[test]
fn the_pairs_of_a_band_sit_side_by_side() {
    let names: Vec<String> = ["VCC", "A", "B", "C", "GND"].iter().map(|s| s.to_string()).collect();
    let nets: Vec<&String> = names.iter().collect();
    let pairs = vec![("C".to_string(), "VCC".to_string()), ("A".to_string(), "GND".to_string())];
    let out: Vec<&str> = pair_up(nets, &pairs, &|n: &str| n == "GND").into_iter().map(|s| s.as_str()).collect();
    assert_eq!(out, ["VCC", "C", "B", "A", "GND"]);
}

/// A section's ground wire that starts on its ground strip just past the section's box (the strip runs on a hole past the last
/// part) still counts as the wire back to the one ground point (the 555 in two sections on a stripboard).
#[test]
fn a_ground_wire_from_the_strip_past_a_sections_box_is_counted() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/ne555_blinker.schjson");
    let mut s: Schematic = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let group = |name: &str, parts: &[&str]| design_spec::PlaceGroup { name: name.into(), parts: parts.iter().map(|p| p.to_string()).collect(), section: true, ..Default::default() };
    s.requirements.groups = vec![group("タイマ", &["U1", "R1", "R2", "C1", "C2"]), group("表示", &["R3", "D1"])];
    for l in [strip_layout(&s), crate::perfboard::perf_layout_with(&s, Default::default())] {
        assert!(strip_check(&s, &l).is_empty());
        let rule = strip_rules(&s, &l).into_iter().find(|r| r.message.starts_with("GND:")).expect("the ground rule");
        assert!(rule.ok, "perf {}: {}", l.perf, rule.message);
    }
}

/// The section marked as the one ground point in the table is the one every other section's ground goes back to (not the
/// first in the table), and it is laid out first.
#[test]
fn the_ground_point_is_the_section_chosen_in_the_table() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/seg7_counter.schjson");
    let mut s: Schematic = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let group = |name: &str, parts: &[&str]| design_spec::PlaceGroup { name: name.into(), parts: parts.iter().map(|p| p.to_string()).collect(), section: true, ..Default::default() };
    s.requirements.groups = vec![group("カウンタ", &["U1", "U3"]), group("表示", &["U2", "DS1", "R1", "R2", "R3", "R4", "R5", "R6", "R7"])];
    let gnd_rule = |s: &Schematic| {
        let l = strip_layout(s);
        assert!(strip_check(s, &l).is_empty(), "{:?}", strip_check(s, &l));
        (l.sections[0].0.clone(), strip_rules(s, &l).into_iter().find(|r| r.message.starts_with("GND:")).expect("the ground rule"))
    };
    let (first, rule) = gnd_rule(&s);
    assert_eq!(first, "カウンタ");
    assert!(rule.ok && rule.message.contains("「表示」の GND は") && rule.message.contains("「カウンタ」の GND の帯"), "{}", rule.message);
    s.requirements.groups[1].ground_point = true;
    let (first, rule) = gnd_rule(&s);
    assert_eq!(first, "表示", "the ground point's section first");
    assert!(rule.ok && rule.message.contains("「カウンタ」の GND は") && rule.message.contains("「表示」の GND の帯"), "{}", rule.message);
}

/// A board built as laid out carries the schematic's nets; a cut left out shorts two nets into one.
#[test]
fn the_board_nets_are_the_schematics_until_a_cut_is_left_out() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/ne555_blinker.schjson");
    let s: Schematic = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let l = strip_layout(&s);
    let b = strip_board_nets(&s, &l);
    let nets = extract(&s.flat());
    for p in &l.parts {
        for (pin, _) in &p.leads {
            let want = nets.iter().find(|n| n.pins.iter().any(|q| q.refdes == p.refdes && q.pin == *pin)).map(|n| n.name.clone()).unwrap();
            assert_eq!(b.pins[&(p.refdes.clone(), pin.clone())], want, "{}.{pin}", p.refdes);
        }
    }
    assert!(!b.off_pins.is_empty(), "the supply off the board");
    let distinct = |b: &crate::breadboard::BoardNets| b.pins.values().collect::<BTreeSet<_>>().len();
    // some cut left out joins two strips of different nets (each cut separates something; no cut adds a net)
    let joined: Vec<usize> = l
        .cuts
        .iter()
        .map(|c| {
            let mut shorted = l.clone();
            shorted.cuts.remove(c);
            distinct(&strip_board_nets(&s, &shorted))
        })
        .collect();
    assert!(joined.iter().all(|n| *n <= distinct(&b)), "{joined:?}");
    assert!(joined.contains(&(distinct(&b) - 1)), "a cut left out shorts two nets: {joined:?}");
}

/// The holes joined to a lead are its net's leads and no other net's (the board as laid out checks clean).
#[test]
fn the_holes_joined_to_a_lead_are_its_net() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/ne555_blinker.schjson");
    let s: Schematic = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    for perf in [false, true] {
        let l = if perf { crate::perfboard::perf_layout_with(&s, StripOptions::default()) } else { strip_layout(&s) };
        assert!(strip_check(&s, &l).is_empty());
        let leads: Vec<(At, String)> = l.parts.iter().flat_map(|p| p.leads.iter().map(|(_, a)| (*a, net_at(&s, &l, *a).unwrap()))).collect();
        for (a, net) in &leads {
            let joined = l.joined_to(*a);
            for (b, other) in &leads {
                assert_eq!(joined.contains(b), net == other, "perf {perf}: {a:?} ({net}) and {b:?} ({other})");
            }
        }
        assert_eq!(l.joined_to((l.rows + 3, 0)), BTreeSet::from([(l.rows + 3, 0)]), "off the board");
    }
}

fn examples() -> Vec<(String, Schematic)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let mut v: Vec<(String, Schematic)> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "schjson"))
        .filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?)))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// Every sample laid out on a stripboard joins what its schematic joins and nothing more, with no two things in one hole.
#[test]
fn every_sample_lays_out_on_a_stripboard_as_drawn() {
    let mut bad = vec![];
    let mut n = 0;
    for (name, s) in examples() {
        n += 1;
        // one row of chips, and wrapped to a board 40 holes wide (about 10 cm)
        for cols in [None, Some(40)] {
            let l = strip_layout_with(&s, StripOptions { cols, ..Default::default() });
            let p = strip_check(&s, &l);
            if !p.is_empty() {
                bad.push(format!("{name} ({cols:?}): {}", p.iter().take(3).cloned().collect::<Vec<_>>().join(" | ")));
            }
        }
    }
    assert!(n > 50, "{n}");
    assert!(bad.is_empty(), "{} of {n}:\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn a_chip_sits_on_cut_pieces_joined_to_its_nets() {
    let s: Schematic = serde_json::from_str(include_str!("../../../../docs/examples/ne555_blinker.schjson")).unwrap();
    let l = strip_layout(&s);
    let chip = l.parts.iter().find(|p| p.look == Look::Dip).expect("the 555");
    assert_eq!(chip.leads.len(), 8);
    // pin 1 top left, pin 8 top right, three columns apart
    let at = |pin: &str| chip.leads.iter().find(|(p, _)| p == pin).unwrap().1;
    assert_eq!(at("1").0, at("8").0);
    assert_eq!(at("8").1 - at("1").1, 3);
    assert!(l.cuts.contains(&(at("1").0, at("1").1 + 1)), "cut between the chip's columns");
    assert!(strip_check(&s, &l).is_empty());
    // the check finds a link left out (a pin cut off from its net) and a link between two home strips (a short)
    let mut open = l.clone();
    let gone = open.links.remove(0);
    assert!(strip_check(&s, &open).iter().any(|p| p.contains("分かれています")), "{:?}", strip_check(&s, &open));
    // the ratsnest: one line for that net, none on the board as laid out
    assert!(strip_unjoined(&s, &l).is_empty());
    let rats = strip_unjoined(&s, &open);
    assert_eq!(rats.len(), 1, "{rats:?}");
    assert_eq!(rats[0].0, gone.net);
    let mut short = l.clone();
    short.links.push(Link { a: (0, l.cols - 1), b: (1, l.cols - 1), net: String::new() });
    assert!(strip_check(&s, &short).iter().any(|p| p.starts_with("ショート")), "{:?}", strip_check(&s, &short));
    assert_eq!(hole_name((2, 11)), "C12");
    assert_eq!(hole_name((27, 0)), "AB1");
}

mod mount_tests {
    use super::*;

    fn sample(f: &str) -> Schematic {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../docs/examples/{f}.schjson"));
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    /// A surface-mount chip sits on an adapter board: its pins where the adapter's pins are (by number, also when the symbol leaves
    /// some out), the rows as far apart as the adapter's.
    #[test]
    fn chips_on_adapters_sit_by_pin_number() {
        let at = |l: &StripLayout, r: &str, pin: &str| l.parts.iter().find(|p| p.refdes == r).unwrap().leads.iter().find(|(p, _)| p == pin).unwrap().1;
        // QFN-16 with 12 pins drawn (no 3, 4, 5, 16): a DIP-16 adapter, pin 15 across from pin 2, pin 13 a row below the last pin drawn on the left (2)
        let s = sample("tp5100_charger");
        let l = strip_layout(&s);
        let u1 = l.parts.iter().find(|p| p.refdes == "U1").unwrap();
        assert_eq!(u1.mount, Mount::Adapter { package: "QFN-16".into(), pins: 16, width: 3 });
        assert_eq!(u1.look, Look::Dip);
        assert_eq!(at(&l, "U1", "15").0, at(&l, "U1", "2").0);
        // (either way round: the layout may turn a chip half a turn)
        assert_eq!(at(&l, "U1", "13").0.abs_diff(at(&l, "U1", "2").0), 2);
        assert_eq!(at(&l, "U1", "15").1.abs_diff(at(&l, "U1", "2").1), 3);
        assert!(strip_check(&s, &l).is_empty(), "{:?}", strip_check(&s, &l));
        // TQFP-32 on a 0.6 inch adapter
        let l = strip_layout(&sample("arduino_minimal"));
        assert_eq!((at(&l, "U1", "1").0, at(&l, "U1", "32").1.abs_diff(at(&l, "U1", "1").1)), (at(&l, "U1", "32").0, 6));
        // a SOT-223 regulator on a one-row adapter
        let l = strip_layout(&sample("ldo_compare"));
        let u1 = l.parts.iter().find(|p| p.refdes == "U1").unwrap();
        assert_eq!(u1.look, Look::Row);
        assert!(matches!(u1.mount, Mount::Adapter { width: 0, .. }));
    }
}

mod size_tests {
    /// The parts' leads stand in for wires (a decoder's resistor from its pin's piece to the display's strip), nets of chip pins only
    /// or of one lead get no strip, and only the pins going up to a strip take a column: the 7-segment counter took 100 x 37 holes with
    /// a strip and a column for everything, then 78 x 21. With bands of strips above and below the chips (a pin's wire up or down,
    /// a wire up and a wire down sharing a column), the two-lead parts standing in the bands' free columns and the wires between bands
    /// in free columns: 60 x 24; the traffic light 36 x 19 → 19 x 20, the 555 blinker 25 x 12 → 13 x 13; wrapped to 40 holes 35 x 34
    /// (2026-11-19). The two-lead parts standing near their nets' wires and the strips sharing rows (`share_rows`): 60 x 22, 19 x 19,
    /// 13 x 11, wrapped to 40 holes 35 x 30; the DMM's B stage at 59 holes 60 x 87 → 60 x 78 (2026-11-20).
    #[test]
    fn leads_as_wires_keep_the_board_small() {
        let size = |f: &str, cols: Option<u16>| {
            let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../docs/examples/{f}.schjson"));
            let s: crate::Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
            let l = super::strip_layout_with(&s, super::StripOptions { cols, ..Default::default() });
            (l.cols, l.rows)
        };
        let (cols, rows) = size("seg7_counter", None);
        assert!(rows <= 22 && cols <= 60, "{cols} x {rows}");
        let (cols, rows) = size("traffic_light", None);
        assert!(cols <= 19 && rows <= 19, "{cols} x {rows}");
        let (cols, rows) = size("ne555_blinker", None);
        assert!(cols <= 13 && rows <= 11, "{cols} x {rows}");
        // wrapped to 40 holes: the chips go in rows, the board as wide as asked
        let (cols, rows) = size("seg7_counter", Some(40));
        assert!(cols <= 40 && rows <= 30, "{cols} x {rows}");
    }

    /// A chip turned half a turn (pin 1 at the bottom right) when that sends its wires to bands that already carry their nets: the
    /// shift register's board 29 x 22 → 24 x 19 (2026-11-22), still joined as drawn.
    #[test]
    fn a_chip_turned_half_a_turn_makes_the_board_smaller() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/shift_register_leds.schjson");
        let s: crate::Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let l = super::strip_layout(&s);
        assert!(l.cols <= 24 && l.rows <= 19, "{} x {}", l.cols, l.rows);
        assert!(super::strip_check(&s, &l).is_empty(), "{:?}", super::strip_check(&s, &l));
        // a turned chip: pin 1 at the bottom of its row, below the last pin of that row (pin 8 of 16)
        let turned = l.parts.iter().filter(|p| p.leads.len() > 4).any(|p| {
            let at = |pin: &str| p.leads.iter().find(|x| x.0 == pin).map(|x| x.1);
            let half = (p.leads.len() / 2).to_string();
            matches!((at("1"), at(&half)), (Some(a), Some(b)) if a.0 > b.0 && a.1 == b.1)
        });
        assert!(turned, "{:?}", l.parts.iter().map(|p| (&p.refdes, p.leads.first())).collect::<Vec<_>>());
    }

    /// More than two short parts stacked in the same columns (the ones above with their wires all up, each in columns of its own):
    /// the Arduino Mega's board 83 x 62 → 65 x 61 (2026-11-22), headers and crystals under the USB chip, still joined as drawn.
    #[test]
    fn several_short_parts_stack_in_the_same_columns() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/arduino_mega_usb.schjson");
        let s: crate::Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let l = super::strip_layout(&s);
        assert!(l.cols <= 65 && l.rows <= 61, "{} x {}", l.cols, l.rows);
        assert!(super::strip_check(&s, &l).is_empty(), "{:?}", super::strip_check(&s, &l));
        let span = |p: &super::StripPart, f: fn(&super::At) -> u16| {
            let v: Vec<u16> = p.leads.iter().map(|x| f(&x.1)).collect();
            (*v.iter().min().unwrap(), *v.iter().max().unwrap())
        };
        let chips: Vec<&super::StripPart> = l.parts.iter().filter(|p| p.leads.len() > 2).collect();
        let under = |a: &super::StripPart| {
            let (ac, ar) = (span(a, |x| x.1), span(a, |x| x.0));
            chips.iter().filter(|b| b.refdes != a.refdes && span(b, |x| x.1).0 <= ac.1 && ac.0 <= span(b, |x| x.1).1 && span(b, |x| x.0).0 > ar.1).count()
        };
        assert!(chips.iter().any(|a| under(a) >= 2), "no stack of three");
    }

    /// Two chips the requirements table keeps apart and laid side by side: one swapped along the board with another chip as tall,
    /// the board as big (the bus transceiver's U1 and U2, 22.9 mm → 71.1 mm, 68 x 17 holes).
    #[test]
    fn chips_kept_apart_swap_places() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/bus_transceiver.schjson");
        let mut s: crate::Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let near = |l: &super::StripLayout| {
            let leads = |r: &str| l.parts.iter().filter(|p| p.refdes == r).flat_map(|p| p.leads.iter().map(|x| x.1)).collect::<Vec<_>>();
            let (a, b) = (leads("U1"), leads("U2"));
            a.iter().flat_map(|x| b.iter().map(move |y| (x.0 as f64 - y.0 as f64).hypot(x.1 as f64 - y.1 as f64) * 2.54)).fold(f64::INFINITY, f64::min)
        };
        let before = super::strip_layout(&s);
        assert!(near(&before) < 33.0, "{}", near(&before));
        s.requirements.apart = vec![design_spec::KeepApart { name: "離す".into(), a: vec!["U1".into()], b: vec!["U2".into()], min_dist: 33.0, ..Default::default() }];
        let l = super::strip_layout(&s);
        assert!(near(&l) >= 33.0, "{}", near(&l));
        assert!(l.cols as usize * l.rows as usize <= before.cols as usize * before.rows as usize, "{} x {}", l.cols, l.rows);
        assert!(super::strip_check(&s, &l).is_empty(), "{:?}", super::strip_check(&s, &l));
        assert!(super::strip_rules(&s, &l).iter().all(|r| r.ok), "{:?}", super::strip_rules(&s, &l));
    }

    /// A two-lead part the requirements table keeps apart from a chip stands in a column that far from it, where its band has one:
    /// the blinker's R1 next to the 555 laid out as usual, 15 mm from it when told to.
    #[test]
    fn a_two_lead_part_stands_away_from_a_chip_kept_apart() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/ne555_blinker.schjson");
        let mut s: crate::Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let closest = |l: &super::StripLayout| {
            let leads = |r: &str| l.parts.iter().filter(|p| p.refdes == r).flat_map(|p| p.leads.iter().map(|x| x.1)).collect::<Vec<_>>();
            let (a, b) = (leads("R1"), leads("U1"));
            a.iter().flat_map(|x| b.iter().map(move |y| (x.0 as f64 - y.0 as f64).hypot(x.1 as f64 - y.1 as f64) * 2.54)).fold(f64::INFINITY, f64::min)
        };
        let l = super::strip_layout(&s);
        assert!(closest(&l) < 15.0, "{}", closest(&l));
        s.requirements.apart = vec![design_spec::KeepApart { name: "離す".into(), a: vec!["R1".into()], b: vec!["U1".into()], min_dist: 15.0, ..Default::default() }];
        let l = super::strip_layout(&s);
        assert!(closest(&l) >= 15.0, "{}", closest(&l));
        assert!(super::strip_check(&s, &l).is_empty(), "{:?}", super::strip_check(&s, &l));
        assert!(super::strip_rules(&s, &l).iter().all(|r| r.ok), "{:?}", super::strip_rules(&s, &l));
    }

    /// Two short parts of the row of chips (chips, transistors) stacked in the same columns (the upper one's wires all up, the lower
    /// one's all down) when the row is as tall as the two: the half adder's board 41 x 13 → 37 x 13 (2026-11-22), still joined as drawn.
    #[test]
    fn short_chips_stack_in_the_same_columns() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/half_adder.schjson");
        let s: crate::Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let l = super::strip_layout(&s);
        assert!(l.cols <= 37 && l.rows <= 13, "{} x {}", l.cols, l.rows);
        assert!(super::strip_check(&s, &l).is_empty(), "{:?}", super::strip_check(&s, &l));
        let span = |p: &super::StripPart, f: fn(&super::At) -> u16| {
            let v: Vec<u16> = p.leads.iter().map(|x| f(&x.1)).collect();
            (*v.iter().min().unwrap(), *v.iter().max().unwrap())
        };
        let chips: Vec<&super::StripPart> = l.parts.iter().filter(|p| p.leads.len() > 2).collect();
        let stacked = chips.iter().any(|a| {
            chips.iter().any(|b| {
                let (ac, bc, ar, br) = (span(a, |x| x.1), span(b, |x| x.1), span(a, |x| x.0), span(b, |x| x.0));
                a.refdes != b.refdes && ac.0 <= bc.1 && bc.0 <= ac.1 && ar.1 < br.0
            })
        });
        assert!(stacked, "{:?}", chips.iter().map(|p| (&p.refdes, span(p, |x| x.1), span(p, |x| x.0))).collect::<Vec<_>>());
    }
}


mod section_tests {
    use super::*;

    /// Two sections from the requirements table: each laid out by itself side by side, every strip cut between them, joined as the
    /// schematic joins them; ground goes back from the second section to the first's ground strip by a wire of its own.
    #[test]
    fn sections_are_cut_apart_and_ground_meets_at_one_point() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/seg7_counter.schjson");
        let mut s: Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let group = |name: &str, parts: &[&str]| design_spec::PlaceGroup { name: name.into(), parts: parts.iter().map(|p| p.to_string()).collect(), section: true, ..Default::default() };
        s.requirements.groups = vec![group("カウンタ", &["U1", "U3"]), group("表示", &["U2", "DS1", "R1", "R2", "R3", "R4", "R5", "R6", "R7"])];
        for cols in [None, Some(40)] {
            let l = strip_layout_with(&s, StripOptions { cols, ..Default::default() });
            assert!(strip_check(&s, &l).is_empty(), "{cols:?}: {:?}", strip_check(&s, &l));
            assert_eq!(l.sections.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["カウンタ", "表示"]);
            let inside = |k: usize, a: At| {
                let (_, p, q) = &l.sections[k];
                a.0 >= p.0 && a.0 <= q.0 && a.1 >= p.1 && a.1 <= q.1
            };
            // every part inside its own section
            for p in &l.parts {
                let k = if ["U1", "U3"].contains(&p.refdes.as_str()) { 0 } else { 1 };
                assert!(p.leads.iter().all(|(_, a)| inside(k, *a)), "{} outside {}", p.refdes, l.sections[k].0);
            }
            // the ground wires between sections all end on the first section's ground strip
            let gnd: Vec<&Link> = l.links.iter().filter(|k| is_ground_name(&k.net) && (inside(0, k.a) != inside(0, k.b))).collect();
            assert_eq!(gnd.len(), 1, "{cols:?}: one ground wire back to the first section");
            // side by side: every row cut between them
            let (_, _, q0) = l.sections[0];
            let (_, p1, _) = l.sections[1];
            if p1.0 == l.sections[0].1 .0 {
                assert!((p1.0..=q0.0).all(|r| l.cuts.contains(&(r, q0.1 + 1))), "the strips cut between the sections");
            }
        }
        // the one ground point holds; a distance to keep between the two sections' chips is measured
        s.requirements.apart = vec![design_spec::KeepApart { name: "離す".into(), a: vec!["U1".into()], b: vec!["DS1".into()], min_dist: 500.0, ..Default::default() }];
        let l = strip_layout(&s);
        let rules = strip_rules(&s, &l);
        assert!(rules.iter().any(|r| r.ok && r.message.starts_with("GND")), "{rules:?}");
        assert!(rules.iter().any(|r| !r.ok && r.message.contains("離す")), "500 mm is not kept: {rules:?}");
        // without sections marked the board is laid out as one
        s.requirements.groups.iter_mut().for_each(|g| g.section = false);
        assert!(strip_layout(&s).sections.is_empty());
    }

    /// With sections and a narrow board, each section wraps its chips within the width and the sections stack down the board: the
    /// board keeps to the width and every net between sections is joined (a strip shared along a row keeps a free hole for the wire
    /// from the other section).
    #[test]
    fn sections_keep_to_a_narrow_board() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/seg7_counter.schjson");
        let mut s: Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let group = |name: &str, parts: &[&str]| design_spec::PlaceGroup { name: name.into(), parts: parts.iter().map(|p| p.to_string()).collect(), section: true, ..Default::default() };
        s.requirements.groups = vec![group("カウンタ", &["U1", "U3"]), group("表示", &["U2", "DS1", "R1", "R2", "R3", "R4", "R5", "R6", "R7"])];
        for w in [30u16, 34, 40] {
            let l = strip_layout_with(&s, StripOptions { cols: Some(w), ..Default::default() });
            assert!(l.cols <= w, "{w}: {} columns", l.cols);
            assert!(strip_check(&s, &l).is_empty(), "{w}: {:?}", strip_check(&s, &l));
        }
    }
}

mod share_tests {
    use super::*;

    /// Two nets used along different stretches of a band share one row, cut between them and named where the second starts; the
    /// sections find a net on its stretch of a shared row.
    #[test]
    fn nets_apart_along_a_band_share_a_row() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/seg7_counter.schjson");
        let s: Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let l = strip_layout(&s);
        assert!(strip_check(&s, &l).is_empty(), "{:?}", strip_check(&s, &l));
        let ((row, col), name) = l.names.first().cloned().expect("a shared row");
        assert!(l.cuts.contains(&(row, col - 1)), "cut where the second net starts");
        assert_ne!(l.homes[row as usize], name);
        let (r, a, _) = strip_of(&l, &name).unwrap();
        assert_eq!((r, a), (row, col));
        let (r, a, _) = strip_of(&l, &l.homes[row as usize]).unwrap();
        assert_eq!((r, a), (row, 0));
        // every end on that row lies on the stretch of its own net
        let nets = extract(&s.flat());
        let net_of = |r: &str, pin: &str| nets.iter().find(|n| n.pins.iter().any(|p| p.refdes == r && p.pin == pin)).map(|n| n.name.clone()).unwrap_or_default();
        // (a row can be shared by more than two nets: the stretch an end is on is named at the last name before it)
        let stretch = |c: u16| l.names.iter().filter(|(a, _)| a.0 == row && a.1 <= c).max_by_key(|(a, _)| a.1).map(|x| x.1.clone()).unwrap_or_else(|| l.homes[row as usize].clone());
        for p in &l.parts {
            for (pin, at) in p.leads.iter().filter(|x| x.1 .0 == row) {
                assert_eq!(stretch(at.1), net_of(&p.refdes, pin), "{}.{pin}", p.refdes);
            }
        }
    }
}


mod fork_tests {
    use super::*;

    fn load(name: &str) -> Schematic {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// A Kelvin connection from the requirements table (a fork: the pad, and the two pads its ways go to): on the active filter's
    /// strips, the ways from R3.2 to R4.1 and to C3.1 share 15 mm of strip; told to keep them apart, the layout moves the part or a
    /// wire's end so the two part at the lead, still joined as the schematic joins them, and the rules say so. One it cannot keep
    /// (R4.1's, both ways going off the same side) is told with how far they run together.
    #[test]
    fn a_fork_in_the_table_parts_at_the_lead() {
        let mut s = load("active_filter.schjson");
        let pads = ["R3.2", "R4.1", "C3.1"];
        let at = |l: &StripLayout| pads.map(|p| pad_at(l, p).unwrap());
        let l = strip_layout(&s);
        let [a, b, c] = at(&l);
        assert!(strip_fork(&l, a, b, c).unwrap() > 10.0, "without the rule the ways share the strip");
        s.requirements.measures.push(design_spec::Measure { name: "検出".into(), what: "fork".into(), pads: pads.map(String::from).to_vec(), op: "max".into(), limit: 0.5, ..Default::default() });
        let l = strip_layout(&s);
        let [a, b, c] = at(&l);
        assert_eq!(strip_fork(&l, a, b, c), Some(0.0));
        assert!(strip_check(&s, &l).is_empty(), "{:?}", strip_check(&s, &l));
        let rules = strip_rules(&s, &l);
        assert!(rules.iter().any(|r| r.ok && r.message.starts_with("ケルビン「検出」")), "{rules:?}");
        // the ways from the other lead run together on the board laid out for this one (laid out for it, a chip is turned half a turn and they part at the lead)
        s.requirements.measures[0].pads = ["R4.1", "R3.2", "C3.1"].map(String::from).to_vec();
        let rules = strip_rules(&s, &l);
        assert!(rules.iter().any(|r| !r.ok && r.message.contains("一緒に走り")), "{rules:?}");
        let l = strip_layout(&s);
        assert!(strip_check(&s, &l).is_empty());
        let rules = strip_rules(&s, &l);
        assert!(rules.iter().any(|r| r.ok && r.message.starts_with("ケルビン「検出」")), "{rules:?}");
    }

    /// The current sensors' sample keeps both of its shunt's Kelvin connections on a stripboard and on a perfboard.
    #[test]
    fn the_shunt_sample_keeps_its_kelvin_connections() {
        let s = load("current_sensors.schjson");
        for l in [strip_layout(&s), strip_layout_with(&s, StripOptions { cols: Some(20), ..Default::default() })] {
            let kelvin: Vec<RuleCheck> = strip_rules(&s, &l).into_iter().filter(|r| r.message.starts_with("ケルビン")).collect();
            assert_eq!(kelvin.len(), 2);
            assert!(kelvin.iter().all(|r| r.ok), "{kelvin:?}");
        }
    }
}
