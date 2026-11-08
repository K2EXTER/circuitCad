use super::*;
use crate::netlist::*;

fn divider() -> Board {
    let part = |r: &str| NetlistPart { refdes: r.into(), value: "1k".into(), footprint: "R_0805".into() };
    let net = |n: &str, pins: &[(&str, &str)]| NetlistNet { name: n.into(), pins: pins.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };
    let mut b = Board::new("t", 10.0, 10.0);
    import(
        &mut b,
        &(1..=6).map(|i| part(&format!("R{i}"))).collect::<Vec<_>>(),
        &[net("A", &[("R1", "1"), ("R2", "1")]), net("B", &[("R2", "2"), ("R3", "1")]), net("C", &[("R3", "2"), ("R4", "1"), ("R5", "1")]), net("GND", &[("R4", "2"), ("R5", "2"), ("R6", "1"), ("R6", "2"), ("R1", "2")])],
    );
    b
}

/// Two sheets of eight resistors each, a chain on each sheet; one net runs between the sheets.
fn two_sheets() -> Board {
    let part = |r: &str| NetlistPart { refdes: r.into(), value: "1k".into(), footprint: "R_0805".into() };
    let net = |n: &str, pins: &[(&str, &str)]| NetlistNet { name: n.into(), pins: pins.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };
    let mut b = Board::new("t", 10.0, 10.0);
    let mut nets = vec![];
    for (sheet, base) in [("電源", 0), ("制御", 8)] {
        for k in 1..8 {
            nets.push(net(&format!("{sheet}{k}"), &[(&format!("R{}", base + k), "2"), (&format!("R{}", base + k + 1), "1")]));
        }
        for k in 1..=8 {
            b.part_sheets.insert(format!("R{}", base + k), sheet.to_string());
        }
    }
    // every part of a sheet has a partner on the other sheet (shared net on pin 1 and pin 2): without the sheets the parts mix
    for k in 1..=8 {
        nets.push(net(&format!("PAIR{k}"), &[(&format!("R{k}"), "1"), (&format!("R{}", 8 + k), "2")]));
    }
    import(&mut b, &(1..=16).map(|i| part(&format!("R{i}"))).collect::<Vec<_>>(), &nets);
    for (r, s) in b.part_sheets.clone() {
        b.part_sheets.insert(r, s);
    }
    b
}

fn sheet_box(b: &Board, sheet: &str) -> (Point, Point) {
    let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
    for (r, _) in b.part_sheets.iter().filter(|(_, s)| s.as_str() == sheet) {
        let (a, c) = extent(b, r).unwrap();
        lo = Point::new(lo.x.min(a.x), lo.y.min(a.y));
        hi = Point::new(hi.x.max(c.x), hi.y.max(c.y));
    }
    (lo, hi)
}

/// Eight copies of "a resistor and a capacitor in series", with a ground that touches all of them and an input and an output each.
fn eight_stages() -> Board {
    let part = |r: &str, f: &str| NetlistPart { refdes: r.into(), value: "1k".into(), footprint: f.into() };
    let net = |n: &str, pins: &[(&str, &str)]| NetlistNet { name: n.into(), pins: pins.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };
    let mut b = Board::new("t", 10.0, 10.0);
    let (mut parts, mut nets, mut gnd) = (vec![], vec![], vec![]);
    for k in 1..=8 {
        parts.push(part(&format!("R{k}"), "R_0805"));
        parts.push(part(&format!("C{k}"), "C_0805"));
        nets.push(net(&format!("MID{k}"), &[(&format!("R{k}"), "2"), (&format!("C{k}"), "1")]));
        gnd.push((format!("C{k}"), "2".to_string()));
    }
    nets.push(NetlistNet { name: "GND".into(), pins: gnd });
    import(&mut b, &parts, &nets);
    b
}

#[test]
fn copies_of_a_structure_are_laid_out_alike_in_a_regular_grid() {
    let mut b = eight_stages();
    autoplace(&mut b).unwrap();
    let pos = |r: &str| b.parts.iter().find(|p| p.refdes == r).unwrap().pos;
    // the capacitor of each stage stands the same way from its resistor
    let off = |k: usize| (pos(&format!("C{k}")).x - pos(&format!("R{k}")).x, pos(&format!("C{k}")).y - pos(&format!("R{k}")).y);
    for k in 2..=8 {
        assert!((off(k).0 - off(1).0).abs() < 1e-6 && (off(k).1 - off(1).1).abs() < 1e-6, "stage {k}: {:?} against {:?}", off(k), off(1));
    }
    // the resistors stand on a grid: few different columns and rows, even steps
    let mut xs: Vec<f64> = (1..=8).map(|k| pos(&format!("R{k}")).x).collect();
    let mut ys: Vec<f64> = (1..=8).map(|k| pos(&format!("R{k}")).y).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    ys.sort_by(f64::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    assert!(xs.len() * ys.len() >= 8 && xs.len() <= 5 && ys.len() <= 3, "{xs:?} {ys:?}");
    let even = |v: &[f64]| v.windows(2).all(|w| ((w[1] - w[0]) - (v[1] - v[0])).abs() < 1e-6);
    assert!(even(&xs) && even(&ys), "{xs:?} {ys:?}");
    // they come in the order of their numbers, row by row
    let order: Vec<(f64, f64)> = (1..=8).map(|k| (pos(&format!("R{k}")).y, pos(&format!("R{k}")).x)).collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    // without the grid the stages are not alike
    let mut plain = eight_stages();
    let t = Tuning::for_board(&plain);
    autoplace_with(&mut plain, &Tuning { arrays: false, ..t }).unwrap();
    let pp = |r: &str| plain.parts.iter().find(|p| p.refdes == r).unwrap().pos;
    let alike = (2..=8).all(|k| ((pp(&format!("C{k}")).x - pp(&format!("R{k}")).x) - (pp("C1").x - pp("R1").x)).abs() < 1e-6 && ((pp(&format!("C{k}")).y - pp(&format!("R{k}")).y) - (pp("C1").y - pp("R1").y)).abs() < 1e-6);
    assert!(!alike || plain.parts.len() < 16, "the test means something only if the plain layout is not already a grid");
}

#[test]
fn a_bypass_capacitor_left_far_from_its_ic_is_moved_to_the_supply_pin() {
    let mut b = ics_with_caps();
    b.set_rect_outline(0.0, 0.0, 100.0, 60.0);
    // U1 at the middle, its capacitor C1 far away
    b.place_part("U1", Point::new(30.0, 20.0), 0);
    b.place_part("U2", Point::new(60.0, 20.0), 0);
    b.place_part("C1", Point::new(5.0, 5.0), 0);
    b.place_part("C2", Point::new(80.0, 5.0), 0);
    for k in 0..6 {
        b.place_part(&format!("R{k}"), Point::new(30.0 + 5.0 * k as f64, 45.0), 0);
    }
    let near = |b: &Board, cap: &str, ic: &str| gap_to_pin(b, cap, ic, "VCC");
    assert!(near(&b, "C1", "U1") > 20.0 && near(&b, "C2", "U2") > 15.0);
    attach_bypass_caps(&mut b, (Point::new(0.0, 0.0), Point::new(100.0, 60.0)), &Default::default());
    assert!(near(&b, "C1", "U1") <= BYPASS_NEAR + 1.5, "{}", near(&b, "C1", "U1"));
    assert!(near(&b, "C2", "U2") <= BYPASS_NEAR + 1.5, "{}", near(&b, "C2", "U2"));
    // not on top of anything
    assert!(!crowded(&b, "C1") && !crowded(&b, "C2"));
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    // a capacitor already near its pin stays
    let again = b.clone();
    attach_bypass_caps(&mut b, (Point::new(0.0, 0.0), Point::new(100.0, 60.0)), &Default::default());
    assert!(b.parts.iter().zip(&again.parts).all(|(a, c)| a.pos == c.pos && a.rot == c.rot));
}

#[test]
fn blocks_are_nested_as_the_shapes_they_are() {
    // two L-shaped blocks (an arm along the bottom, an arm up the side) and a small square that fits in the bay of an L
    let l = |len: f64, arm: f64| -> Cells { vec![(0.0, 0.0, len, arm), (0.0, 0.0, arm, len)] };
    let shapes = vec![l(24.0, 4.0), l(24.0, 4.0), vec![(0.0, 0.0, 6.0, 6.0)]];
    let (put, w, h) = nest_blocks(&shapes, &[]);
    assert_eq!(put.len(), 3);
    let boxes: f64 = shapes.iter().map(|c| c.iter().map(|q| q.0 + q.2).fold(0.0, f64::max) * c.iter().map(|q| q.1 + q.3).fold(0.0, f64::max)).sum();
    // the box of the whole is smaller than the boxes of the blocks added up (they lie in each other's bays)
    assert!(w * h < boxes * 0.9, "{w} x {h} against {boxes}");
    // and nothing covers anything: the squares of two blocks do not overlap
    for (a, pa) in put.iter().enumerate() {
        for (b, pb) in put.iter().enumerate().skip(a + 1) {
            for ca in &shapes[a] {
                for cb in &shapes[b] {
                    let (ax, ay) = (pa.1 + ca.0, pa.2 + ca.1);
                    let (bx, by) = (pb.1 + cb.0, pb.2 + cb.1);
                    assert!(!(ax < bx + cb.2 && bx < ax + ca.2 && ay < by + cb.3 && by < ay + ca.3), "blocks {a} and {b} overlap");
                }
            }
        }
    }
}

#[test]
fn the_role_of_a_part_in_every_copy_is_turned_to_face_what_it_is_joined_to() {
    // a header on the left with a pin for each stage; three stages (R, C) to the right: each R's pin 1 is joined to its header pin,
    // and the resistors stand turned the wrong way (their pin 1 faces away from the header)
    let part = |r: &str, f: &str| NetlistPart { refdes: r.into(), value: "1k".into(), footprint: f.into() };
    let net = |n: &str, pins: &[(&str, &str)]| NetlistNet { name: n.into(), pins: pins.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };
    let mut b = Board::new("t", 60.0, 40.0);
    b.add_footprint(package::PackageSpec::Dual { pins: 6, pitch: 2.54, row_spacing: 5.08, pad_len: 1.6, pad_width: 1.6, drill: Some(0.8) }.build("DIP-6").unwrap());
    let mut parts = vec![part("J1", "DIP-6")];
    let mut nets = vec![];
    for k in 1..=3 {
        parts.push(part(&format!("R{k}"), "R_0805"));
        parts.push(part(&format!("C{k}"), "C_0805"));
        nets.push(net(&format!("PORT{k}"), &[("J1", &k.to_string()), (&format!("R{k}"), "1")]));
        nets.push(net(&format!("MID{k}"), &[(&format!("R{k}"), "2"), (&format!("C{k}"), "1")]));
    }
    import(&mut b, &parts, &nets);
    b.place_part("J1", Point::new(5.0, 20.0), 0);
    for k in 1..=3 {
        b.place_part(&format!("R{k}"), Point::new(30.0, 8.0 + 4.0 * k as f64), 2);
        b.place_part(&format!("C{k}"), Point::new(40.0, 8.0 + 4.0 * k as f64), 0);
    }
    let before = airwire_length(&b);
    let groups: Vec<Vec<Vec<String>>> = vec![(1..=3).map(|k| vec![format!("R{k}"), format!("C{k}")]).collect()];
    turn_copies_toward_ports(&mut b, &groups);
    assert!(airwire_length(&b) < before - 1.0, "{before} -> {}", airwire_length(&b));
    let rots: Vec<u8> = (1..=3).map(|k| b.parts.iter().find(|p| p.refdes == format!("R{k}")).unwrap().rot).collect();
    assert!(rots.iter().all(|r| *r == rots[0]) && rots[0] != 2, "every copy turned alike: {rots:?}");
}

#[test]
fn the_parts_of_a_sheet_stay_together_in_a_block_of_their_own() {
    let mut b = two_sheets();
    let t = Tuning::for_board(&b);
    autoplace_with(&mut b, &Tuning { sheets: true, ..t }).unwrap();
    let (a, c) = (sheet_box(&b, "電源"), sheet_box(&b, "制御"));
    let apart = a.1.x <= c.0.x || c.1.x <= a.0.x || a.1.y <= c.0.y || c.1.y <= a.0.y;
    assert!(apart, "the two sheets' boxes overlap: {a:?} {c:?}");
    // (and without the sheets the same circuit does mix: the test means something)
    let mut mixed = two_sheets();
    let t = Tuning::for_board(&mixed);
    autoplace_with(&mut mixed, &Tuning { sheets: false, ..t }).unwrap();
    let (a, c) = (sheet_box(&mixed, "電源"), sheet_box(&mixed, "制御"));
    assert!(!(a.1.x <= c.0.x || c.1.x <= a.0.x || a.1.y <= c.0.y || c.1.y <= a.0.y), "{a:?} {c:?}");
    // a one-sheet circuit is laid out as it always was
    let mut one = two_sheets();
    one.part_sheets.clear();
    let mut plain = one.clone();
    let t = Tuning::for_board(&one);
    autoplace_with(&mut one, &Tuning { sheets: true, ..t }).unwrap();
    autoplace_with(&mut plain, &Tuning { sheets: false, ..t }).unwrap();
    assert_eq!(one.parts.iter().map(|p| (p.pos, p.rot)).collect::<Vec<_>>(), plain.parts.iter().map(|p| (p.pos, p.rot)).collect::<Vec<_>>());
}

#[test]
fn parts_land_inside_a_fitted_outline_without_rule_violations() {
    let mut b = divider();
    let rep = autoplace(&mut b).unwrap();
    assert_eq!(rep.placed, 6);
    let (lo, hi) = b.bounds().unwrap();
    assert!(b.pads.iter().all(|p| p.pos.x > lo.x && p.pos.x < hi.x && p.pos.y > lo.y && p.pos.y < hi.y));
    let v = drc::check(&b);
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn the_placement_can_be_routed_completely() {
    let mut b = divider();
    autoplace(&mut b).unwrap();
    let r = autoroute::autoroute(&mut b, &autoroute::RouteOptions::default(), None);
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    assert!(drc::check(&b).is_empty());
}

#[test]
fn connected_parts_sit_closer_than_unrelated_ones() {
    let mut b = divider();
    autoplace(&mut b).unwrap();
    let at = |r: &str| b.parts.iter().find(|p| p.refdes == r).unwrap().pos;
    // R4 and R5 share the net C
    let near = at("R4").dist(at("R5"));
    let far = at("R1").dist(at("R6")).max(at("R2").dist(at("R6")));
    assert!(near <= far, "{near} {far}");
}

#[test]
fn turning_parts_shortens_the_airwires_and_keeps_the_board_clean() {
    let (mut plain, mut turned) = (divider(), divider());
    place(&mut plain, false, true, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false, bypass: true, sheets: true, arrays: true, channels: true, groups: true, apart: true }).unwrap();
    place(&mut turned, true, true, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false, bypass: true, sheets: true, arrays: true, channels: true, groups: true, apart: true }).unwrap();
    let (l0, l1) = (airwire_length(&plain), airwire_length(&turned));
    assert!(l1 < l0, "{l1} vs {l0}");
    assert!(drc::check(&turned).is_empty());
    assert!(turned.parts.iter().any(|p| p.rot == 2));
}

/// A header whose pins each drive a resistor and an LED in series (the LED to ground): the greedy order puts all the
/// resistors, then all the LEDs, in rows.
fn fan(pairs: usize) -> Board {
    let part = |r: String, f: &str| NetlistPart { refdes: r, value: "1k".into(), footprint: f.into() };
    let mut b = Board::new("t", 10.0, 10.0);
    let mut parts = vec![part("J1".into(), "PinHeader_1x8")];
    let mut nets: Vec<NetlistNet> = vec![];
    let mut gnd = vec![];
    for k in 0..pairs {
        let (r, d) = (format!("R{k:02}"), format!("D{k:02}"));
        parts.push(part(r.clone(), "R_0805"));
        parts.push(part(d.clone(), "LED_0805"));
        nets.push(NetlistNet { name: format!("Q{k}"), pins: vec![("J1".into(), (k + 1).to_string()), (r.clone(), "1".into())] });
        nets.push(NetlistNet { name: format!("N{k}"), pins: vec![(r, "2".into()), (d.clone(), "A".into())] });
        gnd.push((d, "K".to_string()));
    }
    nets.push(NetlistNet { name: "GND".into(), pins: gnd });
    import(&mut b, &parts, &nets);
    b
}

#[test]
fn improving_the_order_puts_the_parts_of_a_net_together() {
    let (mut plain, mut improved) = (fan(8), fan(8));
    place(&mut plain, false, false, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false, bypass: true, sheets: true, arrays: false, channels: true, groups: true, apart: true }).unwrap();
    place(&mut improved, false, true, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false, bypass: true, sheets: true, arrays: false, channels: true, groups: true, apart: true }).unwrap();
    // every resistor with its LED
    let pair_len = |b: &Board| -> f64 { (0..8).map(|k| { let at = |r: String| b.parts.iter().find(|p| p.refdes == r).unwrap().pos; at(format!("R{k:02}")).dist(at(format!("D{k:02}"))) }).sum() };
    let (l0, l1) = (pair_len(&plain), pair_len(&improved));
    assert!(l1 < l0 * 0.8, "{l1} vs {l0}");
    assert!(drc::check(&improved).is_empty());
    // and the same board always gives the same placement
    let mut again = fan(8);
    place(&mut again, false, true, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false, bypass: true, sheets: true, arrays: false, channels: true, groups: true, apart: true }).unwrap();
    assert!(again.parts.iter().zip(&improved.parts).all(|(a, b)| a.pos == b.pos));
}

#[test]
fn a_hub_with_many_parts_gets_a_compact_board_with_its_parts_round_it() {
    let mut b = fan(8);
    let rep = autoplace(&mut b).unwrap();
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    // not a sprawl: the outline is not much bigger than the parts need
    let cells: f64 = b.parts.iter().filter_map(|p| extent(&b, &p.refdes)).map(|(lo, hi)| (hi.x - lo.x + GAP) * (hi.y - lo.y + GAP + LABEL)).sum();
    assert!(rep.width * rep.height <= 3.0 * cells, "{}x{} for {cells}", rep.width, rep.height);
    // each resistor is next to its LED
    let at = |r: &str| b.parts.iter().find(|p| p.refdes == r).unwrap().pos;
    for k in 0..8 {
        let d = at(&format!("R{k:02}")).dist(at(&format!("D{k:02}")));
        assert!(d < 10.0, "R{k:02} and D{k:02} are {d} mm apart");
    }
}

#[test]
fn parts_the_requirements_keep_apart_are_put_apart_though_their_net_ties_them() {
    // a chain of ten cells, each tied to the next; the first and the second must keep 12 mm
    let items: Vec<(String, Point, f64, f64)> = (0..10).map(|k| (format!("P{k}"), Point::new(0.0, 0.0), 3.0, 3.0)).collect();
    let nets: Vec<NetPins> = (0..9).map(|k| vec![(k, 1.5, 1.5), (k + 1, 1.5, 1.5)]).collect();
    let degree = vec![2usize; 10];
    let tie = ties(&nets, 10);
    let gap = |apart: &[Apart]| {
        let (spots, ..) = layout(&items, &nets, apart, &degree, &tie, true, false, false);
        let at = |i: usize| spots.iter().find(|s| s.0 == i).map(|s| (s.1, s.2)).unwrap();
        cell_gap(&items, 0, at(0), 1, at(1))
    };
    let rule = [Apart { a: vec![0], b: vec![1], limit: 12.0, near: false }];
    let (without, with) = (gap(&[]), gap(&rule));
    assert!(without < 12.0 && with >= 12.0, "the gap is {with} mm with the rule, {without} mm without");
}

#[test]
fn the_parts_of_a_group_are_laid_out_within_its_limit_though_the_nets_would_spread_them() {
    // a chain of ten cells, each tied to the next: the two ends are far apart unless a group says otherwise
    let items: Vec<(String, Point, f64, f64)> = (0..10).map(|k| (format!("P{k}"), Point::new(0.0, 0.0), 3.0, 3.0)).collect();
    let nets: Vec<NetPins> = (0..9).map(|k| vec![(k, 1.5, 1.5), (k + 1, 1.5, 1.5)]).collect();
    let degree = vec![2usize; 10];
    let tie = ties(&nets, 10);
    let apart = |rules: &[Apart]| {
        let (spots, ..) = layout(&items, &nets, rules, &degree, &tie, true, false, false);
        let at = |i: usize| spots.iter().find(|s| s.0 == i).map(|s| (s.1, s.2)).unwrap();
        cell_centres(&items, 0, at(0), 9, at(9))
    };
    let group = [Apart { a: vec![0, 9], b: vec![0, 9], limit: 4.5, near: true }];
    let (without, with) = (apart(&[]), apart(&group));
    assert!(without > 4.5 && with <= 4.5, "the ends are {with} mm apart with the group, {without} mm without");
}

#[test]
fn routed_boards_are_left_alone() {
    let mut b = divider();
    autoplace(&mut b).unwrap();
    autoroute::autoroute(&mut b, &autoroute::RouteOptions::default(), None);
    assert!(autoplace(&mut b).is_err());
}

fn with_pour(b: &mut Board, inset: f64) {
    let (lo, hi) = b.bounds().unwrap();
    let (x0, y0, x1, y1) = (lo.x + inset, lo.y + inset, hi.x - inset, hi.y - inset);
    b.zones.push(Zone::new("GND", Side::Top, vec![Point::new(x0, y0), Point::new(x0, y1), Point::new(x1, y1), Point::new(x1, y0)]));
}

fn area(b: &Board) -> f64 {
    let (lo, hi) = b.bounds().unwrap();
    (hi.x - lo.x) * (hi.y - lo.y)
}

#[test]
fn a_poured_ground_gives_a_smaller_board_that_still_routes() {
    let mut plain = divider();
    autoplace(&mut plain).unwrap();
    let mut poured = divider();
    with_pour(&mut poured, 0.5);
    autoplace(&mut poured).unwrap();
    assert!(area(&poured) < area(&plain), "{} {}", area(&poured), area(&plain));
    let r = autoroute::autoroute(&mut poured, &autoroute::RouteOptions::default(), None);
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    assert!(ratsnest::ratsnest(&poured).is_empty());
    let v = drc::check(&poured);
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn the_tuning_follows_the_pour() {
    let plain = Tuning::for_board(&divider());
    assert_eq!((plain.gap, plain.label, plain.skip_poured), (GAP, LABEL, false));
    let mut b = divider();
    with_pour(&mut b, 0.5);
    let t = Tuning::for_board(&b);
    assert!(t.skip_poured && t.gap < GAP && t.label < LABEL, "{t:?}");
}

#[test]
fn a_full_board_pour_follows_the_new_outline_and_other_pours_stay() {
    let mut b = divider();
    with_pour(&mut b, 0.5);
    let odd = vec![Point::new(1.0, 1.0), Point::new(3.0, 1.0), Point::new(2.0, 3.0)];
    b.zones.push(Zone::new("GND", Side::Bottom, odd.clone()));
    autoplace(&mut b).unwrap();
    let (lo, hi) = b.bounds().unwrap();
    let xs: Vec<f64> = b.zones[0].outline().iter().map(|p| p.x).collect();
    let ys: Vec<f64> = b.zones[0].outline().iter().map(|p| p.y).collect();
    let (zx0, zx1) = (xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max));
    let (zy0, zy1) = (ys.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max));
    assert_eq!((zx0, zy0, zx1, zy1), (lo.x + 0.5, lo.y + 0.5, hi.x - 0.5, hi.y - 0.5));
    assert_eq!(b.zones[1].outline(), odd, "a pour that is not the whole board is left alone");
}

fn centre(b: &Board, r: &str) -> Point {
    let (lo, hi) = extent(b, r).unwrap();
    Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0)
}

#[test]
fn parts_of_a_row_get_one_centre_line() {
    let mut b = divider();
    b.set_rect_outline(0.0, 0.0, 60.0, 40.0);
    // a row that wanders by a millimetre, and a second row far away
    for (r, x, y) in [("R1", 10.0, 10.0), ("R2", 20.0, 10.5), ("R3", 30.0, 11.0), ("R4", 10.0, 30.0), ("R5", 20.0, 30.0), ("R6", 30.0, 30.0)] {
        b.place_part(r, Point::new(x, y), 0);
    }
    align_parts(&mut b, &Default::default());
    let ys: Vec<f64> = ["R1", "R2", "R3"].iter().map(|r| centre(&b, r).y).collect();
    assert!(ys.iter().all(|y| (y - ys[0]).abs() < 1e-9), "{ys:?}");
    assert!((centre(&b, "R4").y - 30.0).abs() < 1e-9, "the other row stays");
    assert!(drc::check(&b).is_empty());
}

#[test]
fn a_part_does_not_move_next_to_another_one() {
    let mut b = divider();
    b.set_rect_outline(0.0, 0.0, 60.0, 40.0);
    for (r, x, y) in [("R1", 10.0, 10.0), ("R2", 20.0, 10.5), ("R3", 40.0, 30.0), ("R4", 40.0, 20.0), ("R5", 50.0, 20.0), ("R6", 50.0, 30.0)] {
        b.place_part(r, Point::new(x, y), 0);
    }
    assert!(!crowded(&b, "R1"));
    b.place_part("R3", Point::new(10.0, 10.5), 0);
    assert!(crowded(&b, "R1"), "R3 lies on R1's pads");
}

#[test]
fn alignment_does_not_add_centre_lines() {
    let rows = |align: bool| {
        let mut b = divider();
        autoplace_with(&mut b, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align, bypass: true, sheets: true, arrays: true, channels: true, groups: true, apart: true }).unwrap();
        let mut ys: Vec<i64> = b.parts.iter().map(|p| (centre(&b, &p.refdes).y * 100.0).round() as i64).collect();
        ys.sort_unstable();
        ys.dedup();
        ys.len()
    };
    assert!(rows(true) <= rows(false), "{} {}", rows(true), rows(false));
}

#[test]
fn a_board_with_only_the_pour_wiring_can_be_placed_again() {
    let mut b = divider();
    autoplace(&mut b).unwrap();
    let r = groundpour::ground_pour(&mut b, &groundpour::PourOptions::default()).unwrap();
    assert!(r.zone_added && r.vias > 0, "{r:?}");
    let vias = b.vias.len();
    // not placed with the plain tuning: the pour is there now, so the poured net drops out
    let rep = autoplace(&mut b).unwrap();
    assert_eq!(rep.poured, vec!["GND".to_string()]);
    assert_eq!(rep.repoured, vias, "the pour is joined through the board again");
    assert!(!b.vias.is_empty());
    assert!(b.zones.iter().any(|z| z.side == Side::Top), "the top pour comes back at the new size");
    assert!(ratsnest::ratsnest(&b).iter().all(|w| w.net != "GND"), "ground is joined");
    let v = drc::check(&b);
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn other_wiring_still_stops_the_placement_and_changes_nothing() {
    let mut b = divider();
    autoplace(&mut b).unwrap();
    autoroute::autoroute(&mut b, &autoroute::RouteOptions::default(), None);
    let before = (b.tracks.len(), b.vias.len(), b.parts.iter().map(|p| p.pos).collect::<Vec<_>>());
    assert!(autoplace(&mut b).is_err());
    assert_eq!(before, (b.tracks.len(), b.vias.len(), b.parts.iter().map(|p| p.pos).collect::<Vec<_>>()));
}

/// Two ICs with their own bypass capacitors, and some other parts for the capacitors to drift off to.
fn ics_with_caps() -> Board {
    let part = |r: &str, f: &str| NetlistPart { refdes: r.into(), value: "1k".into(), footprint: f.into() };
    let net = |n: &str, pins: &[(&str, &str)]| NetlistNet { name: n.into(), pins: pins.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };
    let mut b = Board::new("t", 10.0, 10.0);
    b.add_footprint(package::PackageSpec::Dual { pins: 14, pitch: 2.54, row_spacing: 7.62, pad_len: 1.6, pad_width: 1.6, drill: Some(0.8) }.build("DIP-14_W7.62").unwrap());
    let mut parts = vec![part("U1", "DIP-14_W7.62"), part("U2", "DIP-14_W7.62"), part("C1", "C_0805"), part("C2", "C_0805")];
    let mut nets = vec![net("VCC", &[("U1", "14"), ("U2", "14"), ("C1", "1"), ("C2", "1")]), net("GND", &[("U1", "7"), ("U2", "7"), ("C1", "2"), ("C2", "2")])];
    for k in 0..6 {
        parts.push(part(&format!("R{k}"), "R_0805"));
        nets.push(net(&format!("S{k}"), &[("U1", &(k + 1).to_string()), (&format!("R{k}"), "1")]));
        nets.push(net(&format!("T{k}"), &[("U2", &(k + 1).to_string()), (&format!("R{k}"), "2")]));
    }
    import(&mut b, &parts, &nets);
    b
}

/// The least distance from any pad of `cap` on `net` to a pad of `ic` on that net.
fn gap_to_pin(b: &Board, cap: &str, ic: &str, net: &str) -> f64 {
    let (cp, ip): (Vec<&Pad>, Vec<&Pad>) = (b.pads.iter().filter(|p| p.refdes == cap && p.net == net).collect(), b.pads.iter().filter(|p| p.refdes == ic && p.net == net).collect());
    cp.iter().flat_map(|c| ip.iter().map(move |i| c.pos.dist(i.pos))).fold(f64::MAX, f64::min)
}

#[test]
fn a_capacitor_the_table_places_at_an_ic_goes_to_that_ic_whichever_the_nets_would_choose() {
    // two ICs on one VCC: without the table the capacitors are shared out; with it both go to the one it names
    for ic in ["U1", "U2"] {
        let other = if ic == "U1" { "U2" } else { "U1" };
        let mut b = ics_with_caps();
        b.bypass.push(design_spec::Bypass { ic: ic.into(), caps: vec!["C1".into(), "C2".into()], ..Default::default() });
        autoplace(&mut b).unwrap();
        for cap in ["C1", "C2"] {
            let (near, far) = (gap_to_pin(&b, cap, ic, "VCC"), gap_to_pin(&b, cap, other, "VCC"));
            assert!(near < crate::drc::MAX_DECOUPLING_DISTANCE && near < far, "{cap}: {near} mm from {ic}, {far} mm from {other}");
        }
        assert!(crate::requirements::check(&b).findings.is_empty(), "{:?}", crate::requirements::check(&b).findings);
    }
}

#[test]
fn a_capacitor_far_from_the_ic_the_table_gives_it_is_a_finding() {
    let mut b = ics_with_caps();
    autoplace(&mut b).unwrap();
    // the capacitor that sits by U1 is told to serve U2, which is a few cm away
    let by_u1 = ["C1", "C2"].into_iter().find(|c| gap_to_pin(&b, c, "U1", "VCC") < gap_to_pin(&b, c, "U2", "VCC")).unwrap();
    b.bypass.push(design_spec::Bypass { ic: "U2".into(), caps: vec![by_u1.into()], ..Default::default() });
    let rep = crate::requirements::check(&b);
    assert!(rep.findings.iter().any(|f| f.kind == crate::requirements::FindingKind::Bypass), "{:?}", rep.findings);
    assert!(crate::requirements::marks(&b).bypass.iter().any(|m| !m.ok));
}

#[test]
fn a_capacitor_between_two_supplies_is_pulled_to_the_ic_that_runs_between_them() {
    // the ICs' 0 V is VNEG (a single-supply logic IC on a negative rail): the capacitors sit between VCC and VNEG
    let place_with = |bypass: bool| {
        let mut b = ics_with_caps();
        for p in b.pads.iter_mut().filter(|p| p.net == "GND") {
            p.net = "VNEG".into();
        }
        autoplace_with(&mut b, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false, bypass, sheets: true, arrays: true, channels: true, groups: true, apart: true }).unwrap();
        b
    };
    let (with, without) = (place_with(true), place_with(false));
    let d = |b: &Board| (gap_to_pin(b, "C1", "U1", "VCC") + gap_to_pin(b, "C2", "U2", "VCC") + gap_to_pin(b, "C1", "U1", "VNEG") + gap_to_pin(b, "C2", "U2", "VNEG"))
        .min(gap_to_pin(b, "C1", "U2", "VCC") + gap_to_pin(b, "C2", "U1", "VCC") + gap_to_pin(b, "C1", "U2", "VNEG") + gap_to_pin(b, "C2", "U1", "VNEG"));
    // (the two pins of a DIP-14 are 15 mm apart, so a capacitor cannot be at both: it lands between them)
    assert!(d(&with) < d(&without) * 0.75, "closer than without the pull: {} {}", d(&with), d(&without));
}

#[test]
fn bypass_capacitors_sit_by_their_ic_supply_pin() {
    let place_with = |bypass: bool| {
        let mut b = ics_with_caps();
        autoplace_with(&mut b, &Tuning { gap: GAP, label: LABEL, skip_poured: false, align: false, bypass, sheets: true, arrays: true, channels: true, groups: true, apart: true }).unwrap();
        b
    };
    let (with, without) = (place_with(true), place_with(false));
    let d = |b: &Board| (gap_to_pin(b, "C1", "U1", "VCC") + gap_to_pin(b, "C2", "U2", "VCC")).min(gap_to_pin(b, "C1", "U2", "VCC") + gap_to_pin(b, "C2", "U1", "VCC"));
    assert!(d(&with) <= d(&without) + 1e-9, "{} {}", d(&with), d(&without));
    assert!(d(&with) < 2.0 * crate::drc::MAX_DECOUPLING_DISTANCE, "each capacitor within the limit of the check: {}", d(&with));
    assert!(d(&with) < d(&without) / 2.0, "much closer than without the pull: {} {}", d(&with), d(&without));
    assert!(drc::decoupling_checks(&with).is_empty(), "{:?}", drc::decoupling_checks(&with));
}

/// A microcontroller with a crystal (two ends, two ground pads of the case) and its two load capacitors, and a header on many of its pins.
fn mcu_with_crystal() -> Board {
    let part = |r: &str, f: &str| NetlistPart { refdes: r.into(), value: "1".into(), footprint: f.into() };
    let net = |n: &str, pins: &[(&str, &str)]| NetlistNet { name: n.into(), pins: pins.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };
    let mut b = Board::new("t", 10.0, 10.0);
    b.add_footprint(package::PackageSpec::Quad { pins: 32, pitch: 0.8, span: 8.4, pad_len: 1.6, pad_width: 0.5 }.build("TQFP-32").unwrap());
    b.add_footprint(package::PackageSpec::Dual { pins: 4, pitch: 1.1, row_spacing: 2.2, pad_len: 1.0, pad_width: 0.9, drill: None }.build("XTAL4").unwrap());
    let mut parts = vec![part("U1", "TQFP-32"), part("Y1", "XTAL4"), part("C1", "C_0805"), part("C2", "C_0805"), part("J1", "PinHeader_1x10")];
    let mut nets = vec![
        net("XTAL1", &[("U1", "7"), ("Y1", "1"), ("C1", "1")]),
        net("XTAL2", &[("U1", "8"), ("Y1", "3"), ("C2", "1")]),
        net("GND", &[("U1", "5"), ("Y1", "2"), ("Y1", "4"), ("C1", "2"), ("C2", "2"), ("J1", "10")]),
    ];
    for k in 0..9 {
        nets.push(net(&format!("D{k}"), &[("U1", &(20 + k).to_string()), ("J1", &(k + 1).to_string())]));
    }
    parts.push(part("R1", "R_0805"));
    nets.push(net("RST", &[("U1", "29"), ("R1", "1")]));
    nets.push(net("VCC", &[("U1", "4"), ("R1", "2"), ("J1", "9")]));
    import(&mut b, &parts, &nets);
    b
}

/// The least distance between a pad of `a` and a pad of `b` on `net`.
fn pads_apart(b: &Board, a: &str, c: &str, net: &str) -> f64 {
    gap_to_pin(b, a, c, net)
}

#[test]
fn a_crystal_and_its_load_capacitors_sit_by_the_clock_pins() {
    let mut b = mcu_with_crystal();
    with_pour(&mut b, 0.5);
    autoplace(&mut b).unwrap();
    // the crystal's ends by XTAL1 / XTAL2 (its ground pads do not keep it out of the IC's block), the load capacitors by the crystal
    assert!(pads_apart(&b, "Y1", "U1", "XTAL1") <= 4.0, "{}", pads_apart(&b, "Y1", "U1", "XTAL1"));
    assert!(pads_apart(&b, "Y1", "U1", "XTAL2") <= 4.0, "{}", pads_apart(&b, "Y1", "U1", "XTAL2"));
    for (cap, n) in [("C1", "XTAL1"), ("C2", "XTAL2")] {
        let d = pads_apart(&b, cap, "Y1", n).min(pads_apart(&b, cap, "U1", n));
        assert!(d <= 3.0, "{cap}: {d}");
    }
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
}
