use crate::ratsnest::ratsnest;
use crate::*;

fn board() -> Board {
    Board::new("t", 40.0, 30.0)
}

fn pad(b: &mut Board, x: f64, y: f64, net: &str, through_hole: bool) {
    let shape = if through_hole { PadShape::Circle { d: 1.6 } } else { PadShape::Rect { w: 1.0, h: 1.0 } };
    let mut p = Pad::new(Point::new(x, y), shape, through_hole.then_some(0.8), Side::Top);
    p.net = net.into();
    b.pads.push(p);
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
    vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)]
}

fn count(g: &str, needle: &str) -> usize {
    g.matches(needle).count()
}

#[test]
fn a_zone_connects_pads_of_its_net_and_nothing_else() {
    let mut b = board();
    pad(&mut b, 10.0, 10.0, "GND", true);
    pad(&mut b, 30.0, 20.0, "GND", true);
    pad(&mut b, 20.0, 15.0, "SIG", false);
    pad(&mut b, 22.0, 15.0, "SIG", false);
    assert_eq!(ratsnest(&b).iter().filter(|a| a.net == "GND").count(), 1);

    b.zones.push(Zone::new("GND", Side::Top, rect(5.0, 5.0, 35.0, 25.0)));
    let w = ratsnest(&b);
    assert_eq!(w.iter().filter(|a| a.net == "GND").count(), 0, "the pour joins both GND pads");
    assert_eq!(w.iter().filter(|a| a.net == "SIG").count(), 1, "SIG pads inside the pour stay separate");
    assert!(drc::shorts(&b).is_empty());
}

#[test]
fn a_zone_on_the_other_layer_does_not_connect_smd_pads() {
    let mut b = board();
    pad(&mut b, 10.0, 10.0, "GND", false);
    pad(&mut b, 30.0, 20.0, "GND", false);
    b.zones.push(Zone::new("GND", Side::Bottom, rect(5.0, 5.0, 35.0, 25.0)));
    assert_eq!(ratsnest(&b).len(), 1);
    // ... but a via inside the zone does bring it to the bottom layer
    b.vias.push(Via { pos: Point::new(10.0, 10.0), diameter: 0.6, drill: 0.3 });
    b.vias.push(Via { pos: Point::new(30.0, 20.0), diameter: 0.6, drill: 0.3 });
    assert_eq!(ratsnest(&b).len(), 0);
}

#[test]
fn zone_output_has_region_cutouts_and_thermals() {
    let mut b = board();
    pad(&mut b, 10.0, 10.0, "GND", true); // thermal relief
    pad(&mut b, 20.0, 15.0, "SIG", false); // other net: cleared
    pad(&mut b, 24.0, 15.0, "SIG", false);
    b.tracks.push(Track { side: Side::Top, a: Point::new(20.0, 15.0), b: Point::new(24.0, 15.0), width: 0.25 });
    b.holes.push(Hole::new(Point::new(30.0, 8.0), 3.0));
    b.zones.push(Zone::new("GND", Side::Top, rect(5.0, 5.0, 35.0, 25.0)));

    let top = gerber::copper(&b, Side::Top);
    // the pour, and one hole each for the foreign pads and track (merged into one) and the mounting hole
    assert_eq!(count(&top, "G36*"), 3);
    assert_eq!(count(&top, "G37*"), 3);
    assert!(top.find("G36*").unwrap() < top.find("%LPC*%").unwrap());
    // the clear section ends before the normal copper is drawn dark again
    let last_dark = top.rfind("%LPD*%").unwrap();
    assert!(last_dark > top.find("%LPC*%").unwrap());
    assert!(top[last_dark..].contains("D03*"), "pads are flashed after the cut-outs");

    assert!(top.contains("C,2.200000"), "thermal ring: pad 1.6 + 2 x 0.3 gap");
    assert!(top.contains("C,0.300000"), "thermal spokes");
    assert_eq!(count(&top, "X10000000Y10000000D03*"), 2, "thermal ring and the pad itself");
    // the cut-outs are real geometry: the mounting hole's hole is a ring of r = 1.5 + 0.3 around (30, 8)
    let fill = crate::zonefill::zone_fill(&b, &b.zones[0], &drc::connectivity(&b));
    assert_eq!(fill.len(), 1);
    let near_hole = fill[0].holes.iter().find(|h| crate::geom::inside_polygon(h, Point::new(30.0, 8.0))).expect("hole around the mounting hole");
    let r = near_hole.iter().map(|p| p.dist(Point::new(30.0, 8.0))).fold(0.0, f64::max);
    assert!((r - 1.8).abs() < 0.01, "radius {r}");

    // the bottom layer is untouched by a top zone
    let bottom = gerber::copper(&b, Side::Bottom);
    assert!(!bottom.contains("G36*") && !bottom.contains("%LPC*%"));
}

#[test]
fn a_foreign_net_pad_inside_the_zone_is_cleared_not_connected() {
    let mut b = board();
    pad(&mut b, 10.0, 10.0, "GND", true);
    pad(&mut b, 20.0, 15.0, "VCC", false);
    b.zones.push(Zone::new("GND", Side::Top, rect(5.0, 5.0, 35.0, 25.0)));
    let fill = crate::zonefill::zone_fill(&b, &b.zones[0], &drc::connectivity(&b));
    assert!(fill.iter().any(|s| s.holes.iter().any(|h| crate::geom::inside_polygon(h, Point::new(20.0, 15.0)))), "the VCC pad is cut out of the pour");
    assert!(!fill.iter().any(|s| s.holes.iter().any(|h| crate::geom::inside_polygon(h, Point::new(10.0, 10.0)))), "the GND pad is not (it gets thermal spokes)");
    assert!(drc::shorts(&b).is_empty());
}

#[test]
fn zones_without_a_net_are_output_but_connect_nothing() {
    let mut b = board();
    pad(&mut b, 10.0, 10.0, "GND", true);
    pad(&mut b, 30.0, 20.0, "GND", true);
    b.zones.push(Zone::new("", Side::Top, rect(5.0, 5.0, 35.0, 25.0)));
    assert_eq!(ratsnest(&b).len(), 1);
    assert!(gerber::copper(&b, Side::Top).contains("G36*"));
}

#[test]
fn drc_checks_zone_geometry() {
    let mut b = board();
    // sticking out is fine (the copper is cut to the outline); sharing nothing with the board is not
    b.zones.push(Zone::new("GND", Side::Top, rect(5.0, 5.0, 60.0, 25.0)));
    assert!(!drc::check(&b).iter().any(|v| v.message.contains("外")));
    b.zones.push(Zone::new("GND", Side::Top, rect(50.0, 50.0, 60.0, 60.0)));
    b.zones.push(Zone::new("GND", Side::Top, vec![Point::new(1.0, 1.0), Point::new(2.0, 2.0)]));
    let msgs: Vec<_> = drc::check(&b).into_iter().map(|v| v.message).collect();
    assert!(msgs.iter().any(|m| m.contains("外")), "{msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("3点未満")), "{msgs:?}");
    b.zones.clear();
    b.zones.push(Zone::new("GND", Side::Top, rect(2.0, 2.0, 38.0, 28.0)));
    assert!(drc::check(&b).is_empty());
}

#[test]
fn zones_round_trip_through_json_with_defaults() {
    let json = r#"{"name":"x","outline":[],"tracks":[],"pads":[],"vias":[],"holes":[],"silk":[],"rules":{},
        "zones":[{"net":"GND","side":"Top","polygon":[{"x":0,"y":0},{"x":1,"y":0},{"x":1,"y":1}]}]}"#;
    let b: Board = serde_json::from_str(json).unwrap();
    assert_eq!(b.zones.len(), 1);
    assert!((b.zones[0].clearance - 0.3).abs() < 1e-12 && b.zones[0].thermal);
    let again: Board = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
    assert_eq!(again.zones[0].polygon.len(), 3);
}

#[test]
fn designator_text_follows_the_part() {
    let mut b = board();
    b.set_part(Part { refdes: "R1".into(), value: "10k".into(), footprint: "R_0805".into(), pos: Point::new(10.0, 10.0), rot: 0, side: Side::Top }).unwrap();
    assert_eq!(b.texts.len(), 1);
    let t = b.texts[0].clone();
    assert_eq!((t.text.as_str(), t.refdes.as_str(), t.side), ("R1", "R1", Side::Top));
    assert!((t.pos.x - 10.0).abs() < 1e-9 && t.pos.y > 10.0 + 0.7, "above the part, got {:?}", t.pos);

    b.place_part("R1", Point::new(20.0, 5.0), 0);
    assert_eq!(b.texts.len(), 1, "regenerated, not duplicated");
    assert!((b.texts[0].pos.x - 20.0).abs() < 1e-9 && b.texts[0].pos.y > 5.0);

    let silk = gerber::silk(&b, Side::Top);
    assert!(silk.contains("D01*"), "the stroke font produces drawn lines");
    assert!(gerber::silk(&b, Side::Bottom).matches("D01*").count() == 0);

    b.remove_part("R1");
    assert!(b.texts.is_empty());
}

/// A pour drawn past the board edge is cut to the board, with the edge clearance left free, and the
/// clearance around a track has round ends (no square stubs).
#[test]
fn a_pour_is_cut_to_the_outline_and_round_around_track_ends() {
    let mut b = board();
    b.zones.push(Zone::new("GND", Side::Top, rect(-10.0, -10.0, 60.0, 50.0)));
    b.tracks.push(Track { side: Side::Top, a: Point::new(10.0, 15.0), b: Point::new(20.0, 15.0), width: 0.25 });
    b.pads.push({
        let mut p = Pad::new(Point::new(10.0, 15.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
        p.net = "SIG".into();
        p
    });
    let fill = crate::zonefill::zone_fill(&b, &b.zones[0], &drc::connectivity(&b));
    assert_eq!(fill.len(), 1);
    let edge = b.rules.edge_clearance.max(b.zones[0].clearance);
    for p in &fill[0].outer {
        assert!(p.x >= edge - 1e-6 && p.x <= 40.0 - edge + 1e-6 && p.y >= edge - 1e-6 && p.y <= 30.0 - edge + 1e-6, "{p:?} is outside the board or in the edge clearance");
    }
    // past the end of the track the hole is round: a point on the extension of the track, within the clearance, is cut out
    let c = b.zones[0].clearance;
    let end = Point::new(20.0 + 0.125 + c - 0.01, 15.0);
    assert!(fill[0].holes.iter().any(|h| crate::geom::inside_polygon(h, end)));
    // ... but the square corner a flat cap would leave is not: it stays copper
    let corner = Point::new(20.0 + 0.125 + c - 0.02, 15.0 + 0.125 + c - 0.02);
    assert!(!fill[0].holes.iter().any(|h| crate::geom::inside_polygon(h, corner)));
    // the triangles of the shape cover its area
    let area: f64 = fill[0].triangles().iter().map(|t| crate::geom::signed_area(t).abs()).sum();
    let exact = crate::geom::signed_area(&fill[0].outer).abs() - fill[0].holes.iter().map(|h| crate::geom::signed_area(h).abs()).sum::<f64>();
    assert!((area - exact).abs() < 0.01, "{area} vs {exact}");
}

/// Timing of the per-frame work of the editor on a big board (`cargo test -p pcb-core --release frame_cost -- --ignored --nocapture`).
#[test]
#[ignore]
fn frame_cost() {
    use std::time::Instant;
    let (cols, rows): (usize, usize) = std::env::var("FC").ok().and_then(|s| s.split_once('x').map(|(a, b)| (a.parse().unwrap(), b.parse().unwrap()))).unwrap_or((20, 10));
    let mut b = Board::new("big", 200.0, 100.0);
    for r in 0..rows {
        for c in 0..cols {
            let refdes = format!("R{}", r * cols + c + 1);
            b.set_part(Part { refdes: refdes.clone(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(8.0 + c as f64 * 9.5, 8.0 + r as f64 * 9.5), rot: 0, side: Side::Top }).unwrap();
            for p in b.pads.iter_mut().filter(|p| p.refdes == refdes) {
                p.net = format!("N{}", (r * cols + c) % 40 + if p.pin == "1" { 0 } else { 1 });
            }
        }
    }
    let pads: Vec<Point> = b.pads.iter().map(|p| p.pos).collect();
    for w in pads.chunks(2) {
        if let [a, c] = w {
            b.tracks.push(Track { side: Side::Top, a: *a, b: Point::new(a.x, a.y + 3.0), width: 0.25 });
            b.tracks.push(Track { side: Side::Top, a: Point::new(a.x, a.y + 3.0), b: Point::new(c.x, a.y + 3.0), width: 0.25 });
        }
    }
    for i in 0..60 {
        b.vias.push(Via { pos: Point::new(5.0 + i as f64 * 3.0, 4.0), diameter: 0.6, drill: 0.3 });
    }
    b.zones.push(Zone::new("N0", Side::Bottom, vec![Point::new(1.0, 1.0), Point::new(199.0, 1.0), Point::new(199.0, 99.0), Point::new(1.0, 99.0)]));
    println!("parts {} pads {} tracks {} vias {}", b.parts.len(), b.pads.len(), b.tracks.len(), b.vias.len());
    let t = Instant::now();
    let conn = drc::connectivity(&b);
    println!("connectivity  {:?}", t.elapsed());
    let t = Instant::now();
    let fill = crate::zonefill::zone_fill(&b, &b.zones[0], &conn);
    println!("zone_fill     {:?} ({} shapes)", t.elapsed(), fill.len());
    let t = Instant::now();
    let tris: usize = fill.iter().map(|s| s.triangles().len()).sum();
    println!("triangles     {:?} ({tris})", t.elapsed());
    let t = Instant::now();
    let n = ratsnest(&b).len();
    println!("ratsnest      {:?} ({n})", t.elapsed());
    let t = Instant::now();
    let _ = crate::zonefill::geometry_key(&b);
    println!("geometry_key  {:?}", t.elapsed());
    let t = Instant::now();
    let v = drc::check(&b).len();
    println!("drc::check    {:?} ({v})", t.elapsed());
    let t = Instant::now();
    let _ = crate::gerber::copper(&b, Side::Bottom);
    println!("gerber bottom {:?}", t.elapsed());
}

#[test]
fn a_later_pour_of_another_net_cuts_the_earlier_one_with_a_gap() {
    use crate::geom::inside_polygon;
    let mut b = board();
    let (mut a, mut c) = (Zone::new("A", Side::Top, rect(2.0, 2.0, 20.0, 20.0)), Zone::new("B", Side::Top, rect(12.0, 8.0, 30.0, 25.0)));
    a.clearance = 0.5;
    c.clearance = 0.5;
    b.zones.push(a);
    b.zones.push(c);
    let conn = drc::connectivity(&b);
    let fill_a = crate::zonefill::zone_fill(&b, &b.zones[0], &conn);
    let fill_b = crate::zonefill::zone_fill(&b, &b.zones[1], &conn);
    // B keeps its whole rectangle; A is cut back by B's clearance
    let area = |f: &[crate::zonefill::FillShape]| f.iter().map(|s| crate::geom::signed_area(&s.outer).abs() - s.holes.iter().map(|h| crate::geom::signed_area(h).abs()).sum::<f64>()).sum::<f64>();
    assert!((area(&fill_b) - 18.0 * 17.0).abs() < 1.0, "{}", area(&fill_b));
    assert!(area(&fill_a) < 18.0 * 18.0 - 8.0 * 12.0 + 1.0, "the overlap is gone: {}", area(&fill_a));
    for s in &fill_a {
        for p in &s.outer {
            assert!(!inside_polygon(&b.zones[1].polygon, *p), "{p:?} is inside the other pour");
        }
    }
    // a gap of the clearance remains between the two coppers
    let gap = |p: &Point| ((12.0 - p.x).max(0.0).max(p.x - 30.0)).hypot((8.0 - p.y).max(0.0).max(p.y - 25.0));
    assert!(fill_a.iter().all(|s| s.outer.iter().all(|p| gap(p) > 0.5 - 1e-3)), "{fill_a:?}");
    // bringing A to the front turns it around: A keeps its whole rectangle, B gives way
    let at = b.move_zone(0, true);
    assert_eq!(at, 1);
    assert_eq!(b.zones[1].net, "A");
    let conn = drc::connectivity(&b);
    assert!((area(&crate::zonefill::zone_fill(&b, &b.zones[1], &conn)) - 18.0 * 18.0).abs() < 1.0);
    assert!(area(&crate::zonefill::zone_fill(&b, &b.zones[0], &conn)) < 18.0 * 17.0 - 1.0);
    b.move_zone(1, false);
    assert_eq!(b.zones[0].net, "A");
    // the same net on both pours is not cut
    b.zones[1].net = "A".into();
    let conn = drc::connectivity(&b);
    assert!(area(&crate::zonefill::zone_fill(&b, &b.zones[0], &conn)) > 18.0 * 18.0 - 1.0);
}

fn fill_area(b: &Board, i: usize) -> f64 {
    let fill = crate::zonefill::zone_fill(b, &b.zones[i], &drc::connectivity(b));
    fill.iter().map(|s| crate::geom::signed_area(&s.outer).abs() - s.holes.iter().map(|h| crate::geom::signed_area(h).abs()).sum::<f64>()).sum()
}

/// A second pour of the same net next to (or over) the first one is copper that joins the first: it must not be dropped as an
/// "island" because no pad of the net lies inside it.
#[test]
fn a_pour_of_the_same_net_beside_another_is_not_an_island() {
    let mut b = board();
    pad(&mut b, 8.0, 10.0, "GND", true);
    let mut a = Zone::new("GND", Side::Top, rect(5.0, 5.0, 15.0, 15.0));
    a.remove_islands = true;
    let mut next = Zone::new("GND", Side::Top, rect(15.0, 5.0, 25.0, 15.0));
    next.remove_islands = true;
    b.zones.push(a);
    b.zones.push(next);
    assert!(fill_area(&b, 1) > 80.0, "the pour next to the first one stays: {}", fill_area(&b, 1));
    // over it, and reaching out of it
    b.zones[1].polygon = rect(10.0, 5.0, 25.0, 15.0);
    assert!(fill_area(&b, 1) > 120.0, "the overlapping pour stays: {}", fill_area(&b, 1));
    // still an island when it touches nothing of the net
    b.zones[1].polygon = rect(30.0, 5.0, 38.0, 15.0);
    assert!(fill_area(&b, 1) < 1e-6, "a pour that touches nothing of the net is removed");
}

/// Drawn, pours of one net on one layer are one piece of copper: where they overlap it is not there twice, and where they touch
/// the seam is gone.
#[test]
fn merged_fills_join_overlapping_and_touching_pours_of_one_net() {
    let mut b = board();
    pad(&mut b, 8.0, 10.0, "GND", true);
    b.zones.push(Zone::new("GND", Side::Top, rect(5.0, 5.0, 15.0, 15.0)));
    b.zones.push(Zone::new("GND", Side::Top, rect(10.0, 5.0, 20.0, 15.0)));
    b.zones.push(Zone::new("GND", Side::Top, rect(20.0, 5.0, 30.0, 15.0)));
    b.zones.push(Zone::new("GND", Side::Bottom, rect(5.0, 5.0, 15.0, 15.0)));
    b.zones.push(Zone::new("SIG", Side::Top, rect(5.0, 18.0, 15.0, 25.0)));
    let merged = crate::zonefill::merged_fills(&b, &drc::connectivity(&b));
    assert_eq!(merged.len(), 3, "GND top, GND bottom, SIG top");
    let (side, net, top) = &merged[0];
    assert_eq!(net, "GND");
    assert_eq!(*side, Side::Top);
    assert_eq!(top.len(), 1, "one piece: {top:?}");
    let area = crate::geom::signed_area(&top[0].outer).abs();
    assert!((area - 25.0 * 10.0).abs() < 2.0, "5..30 x 5..15, not counted twice where they overlap: {area}");
}
