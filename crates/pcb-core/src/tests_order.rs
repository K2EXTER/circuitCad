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
