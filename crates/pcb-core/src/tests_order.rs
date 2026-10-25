//! Fab presets and the pre-order check.
use crate::fab::*;
use crate::*;

fn pad(b: &mut Board, x: f64, y: f64, net: &str) {
    let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
    p.net = net.into();
    b.pads.push(p);
}

/// Two pads joined by a track on a 30 x 20 board, with a mounting hole.
fn routed() -> Board {
    let mut b = Board::new("demo", 30.0, 20.0);
    pad(&mut b, 5.0, 10.0, "A");
    pad(&mut b, 25.0, 10.0, "A");
    b.tracks.push(Track { side: Side::Top, a: Point::new(5.0, 10.0), b: Point::new(25.0, 10.0), width: 0.25 });
    b.holes.push(Hole::new(Point::new(15.0, 4.0), 3.0));
    b
}

fn errors(checks: &[Check]) -> Vec<&str> {
    checks.iter().filter(|c| c.level == Level::Error).map(|c| c.title.as_str()).collect()
}

#[test]
fn presets_have_unique_ids_sane_numbers_and_a_way_to_order() {
    let mut ids: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), PRESETS.len());
    for p in PRESETS {
        assert!(p.min_track_width >= 0.1 && p.min_clearance >= 0.1 && p.min_drill >= 0.2, "{}", p.name);
        // the suggested via must satisfy the preset's own ring rule
        assert!((p.via_diameter - p.via_drill) / 2.0 + 1e-9 >= p.min_annular_ring, "{}: ring", p.name);
        assert!(p.via_drill >= p.min_drill && p.track_width >= p.min_track_width, "{}", p.name);
        assert!(!p.steps.is_empty());
        if p.id != "standard" {
            assert!(p.url.starts_with("https://"), "{}", p.name);
        }
    }
    assert!(preset("jlcpcb").is_some() && preset("nope").is_none());
}

#[test]
fn applying_a_preset_sets_the_drc_rules_and_covered_by_notices_looser_ones() {
    let mut r = Rules::default();
    let osh = preset("oshpark").unwrap();
    assert!(!osh.covered_by(&r), "the default edge clearance 0.3 is below OSH Park's 0.381");
    osh.apply_rules(&mut r);
    assert!((r.edge_clearance - 0.381).abs() < 1e-9 && (r.min_drill - 0.254).abs() < 1e-9);
    assert!(osh.covered_by(&r));
}

#[test]
fn a_finished_board_is_ready_and_lists_what_was_checked() {
    let b = routed();
    let c = preflight(&b, preset("jlcpcb"));
    assert!(ready(&c), "{:?}", errors(&c));
    assert!(c.iter().any(|x| x.title.contains("30.0 × 20.0")));
    assert!(c.iter().any(|x| x.title.contains("すべて配線")));
}

#[test]
fn missing_outline_unrouted_and_parts_off_the_board_each_block_the_order() {
    let mut empty = routed();
    empty.outline.clear();
    assert!(errors(&preflight(&empty, None))[0].contains("外形"));

    let mut open = routed();
    open.tracks.clear();
    let c = preflight(&open, None);
    assert!(errors(&c).iter().any(|t| t.contains("未配線")));
    assert!(!ready(&c));

    let mut off = routed();
    off.parts.push(Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(50.0, 5.0), rot: 0, side: Side::Top });
    let c = preflight(&off, None);
    assert!(errors(&c).iter().any(|t| t.contains("基板の外") && t.contains("R1")));
}

#[test]
fn drc_violations_block_and_point_at_the_place() {
    let mut b = routed();
    b.tracks.push(Track { side: Side::Top, a: Point::new(10.0, 10.3), b: Point::new(20.0, 10.3), width: 0.25 }); // 0.05 mm gap
    let c = preflight(&b, None);
    let e = c.iter().find(|x| x.level == Level::Error && x.title.contains("DRC")).expect("drc error");
    assert!(e.at.is_some());
}

#[test]
fn a_fab_with_stricter_minimums_finds_what_the_loose_rules_let_through() {
    let mut b = routed();
    b.rules.min_track_width = 0.1; // loose on purpose
    b.tracks[0].width = 0.13;
    assert!(ready(&preflight(&b, preset("jlcpcb"))), "0.13 mm is fine for JLCPCB (0.127)");
    let c = preflight(&b, preset("pcbway"));
    assert!(errors(&c).iter().any(|t| t.contains("PCBWay")), "{:?}", errors(&c));
    // same rules as the fab: no warning about loose rules
    let mut ok = routed();
    preset("pcbway").unwrap().apply_rules(&mut ok.rules);
    let c = preflight(&ok, preset("pcbway"));
    assert!(ready(&c) && !c.iter().any(|x| x.level == Level::Warn), "{:?}", c.iter().map(|x| &x.title).collect::<Vec<_>>());
}

#[test]
fn a_board_over_the_cheap_size_of_the_fab_gets_a_note_and_nothing_else_changes() {
    let small = preflight(&routed(), preset("jlcpcb"));
    assert!(small.iter().any(|c| c.level == Level::Ok && c.title.contains("料金の枠")), "{:?}", small.iter().map(|c| &c.title).collect::<Vec<_>>());
    let mut big = routed();
    big.outline = vec![Point::new(0.0, 0.0), Point::new(150.0, 0.0), Point::new(150.0, 40.0), Point::new(0.0, 40.0)];
    let c = preflight(&big, preset("jlcpcb"));
    assert!(c.iter().any(|c| c.level == Level::Info && c.title.contains("料金の枠")));
    assert!(ready(&c) == ready(&preflight(&routed(), preset("jlcpcb"))), "a size note never blocks");
    // 90 x 100 is inside
    big.outline = vec![Point::new(0.0, 0.0), Point::new(90.0, 0.0), Point::new(90.0, 100.0), Point::new(0.0, 100.0)];
    assert!(preflight(&big, preset("pcbway")).iter().any(|c| c.level == Level::Ok && c.title.contains("料金の枠")));
    assert!(!preflight(&big, preset("oshpark")).iter().any(|c| c.title.contains("料金の枠")), "OSH Park prices by area");
}

#[test]
fn a_loaded_net_with_a_big_copper_drop_is_a_warning_and_a_wide_track_is_not() {
    let mut b = Board::new("t", 100.0, 20.0);
    for x in [5.0, 85.0] {
        let mut p = Pad::new(Point::new(x, 10.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
        p.net = "VIN".into();
        p.refdes = if x < 10.0 { "J1".into() } else { "U1".into() };
        p.pin = "1".into();
        b.pads.push(p);
    }
    b.tracks.push(Track { side: Side::Top, a: Point::new(5.0, 10.0), b: Point::new(85.0, 10.0), width: 0.25 });
    let has = |b: &Board, level: Level| preflight(b, None).iter().any(|c| c.level == level && c.title.contains("電圧降下"));
    assert!(!has(&b, Level::Warn) && !has(&b, Level::Ok), "no current declared, nothing to say");
    // 80 mm of 0.25 mm copper is 0.16 ohm: 1 A drops 0.16 V
    b.net_currents.insert("VIN".into(), 1.0);
    assert!(has(&b, Level::Warn));
    b.tracks[0].width = 2.0;
    assert!(has(&b, Level::Ok) && !has(&b, Level::Warn));
}

#[test]
fn the_schematic_check_blocks_only_when_the_circuit_differs() {
    assert_eq!(schematic_sync_check("追加 1 (R3)", true).level, Level::Error);
    assert_eq!(schematic_sync_check("値・形が違う 1 (R1)", false).level, Level::Warn);
}

#[test]
fn a_part_with_an_estimated_land_pattern_is_noted_until_the_flag_is_removed() {
    let mut b = routed();
    b.parts.push(Part { refdes: "U7".into(), value: "ADC".into(), footprint: "VQFN-32_5x5".into(), pos: Point::new(15.0, 15.0), rot: 0, side: Side::Top });
    assert!(!preflight(&b, None).iter().any(|c| c.title.contains("ランドパターンが概算")));
    b.land_unverified.insert("VQFN-32_5x5".into(), "ADS124S08".into());
    let c = preflight(&b, None);
    let note = c.iter().find(|c| c.title.contains("ランドパターンが概算")).expect("the estimate is noted");
    assert_eq!(note.level, Level::Warn);
    assert!(note.title.contains("U7 (ADS124S08)"), "{}", note.title);
}
