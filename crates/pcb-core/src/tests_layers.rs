use crate::autoroute::*;
use crate::ratsnest::ratsnest;
use crate::*;

fn pad(b: &mut Board, x: f64, y: f64, net: &str, through_hole: bool) {
    let shape = if through_hole { PadShape::Circle { d: 1.6 } } else { PadShape::Rect { w: 1.0, h: 1.0 } };
    let mut p = Pad::new(Point::new(x, y), shape, through_hole.then_some(0.8), Side::Top);
    p.net = net.into();
    b.pads.push(p);
}

fn track(b: &mut Board, side: Side, a: (f64, f64), z: (f64, f64)) {
    b.tracks.push(Track { side, a: Point::new(a.0, a.1), b: Point::new(z.0, z.1), width: 0.25 });
}

fn four_layer() -> Board {
    let mut b = Board::new("t", 30.0, 20.0);
    b.inner_layers = 2;
    b
}

#[test]
fn copper_layers_run_from_top_to_bottom() {
    assert_eq!(Board::new("t", 1.0, 1.0).copper_layers(), [Side::Top, Side::Bottom]);
    assert_eq!(four_layer().copper_layers(), [Side::Top, Side::Inner(1), Side::Inner(2), Side::Bottom]);
}

#[test]
fn an_old_file_without_inner_layers_loads_as_two_layers() {
    let json = serde_json::to_string(&Board::new("t", 10.0, 10.0)).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v.as_object_mut().unwrap().remove("inner_layers");
    let b: Board = serde_json::from_value(v).unwrap();
    assert_eq!(b.inner_layers, 0);
}

#[test]
fn a_via_joins_an_inner_track_to_the_outer_layers() {
    let mut b = four_layer();
    pad(&mut b, 5.0, 10.0, "A", false);
    pad(&mut b, 25.0, 10.0, "A", false);
    track(&mut b, Side::Top, (5.0, 10.0), (8.0, 10.0));
    b.vias.push(Via { pos: Point::new(8.0, 10.0), diameter: 0.6, drill: 0.3, net: String::new() });
    track(&mut b, Side::Inner(1), (8.0, 10.0), (22.0, 10.0));
    assert_eq!(ratsnest(&b).len(), 1, "the far end is not joined yet");
    b.vias.push(Via { pos: Point::new(22.0, 10.0), diameter: 0.6, drill: 0.3, net: String::new() });
    track(&mut b, Side::Top, (22.0, 10.0), (25.0, 10.0));
    assert!(ratsnest(&b).is_empty());
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
}

#[test]
fn an_inner_track_does_not_touch_the_track_above_it() {
    let mut b = four_layer();
    track(&mut b, Side::Top, (5.0, 10.0), (25.0, 10.0));
    track(&mut b, Side::Inner(1), (15.0, 0.5), (15.0, 19.5));
    assert!(drc::check(&b).is_empty(), "different layers cross freely: {:?}", drc::check(&b));
}

#[test]
fn clearance_is_checked_on_each_inner_layer() {
    let mut b = four_layer();
    track(&mut b, Side::Inner(2), (10.0, 10.0), (20.0, 10.0));
    track(&mut b, Side::Inner(2), (10.0, 10.35), (20.0, 10.35));
    track(&mut b, Side::Inner(1), (10.0, 10.35), (20.0, 10.35)); // the other layer is clear
    let v = drc::check(&b);
    assert!(v.iter().any(|x| x.message.contains("内層2")), "{v:?}");
}

#[test]
fn a_wall_on_both_outer_layers_is_crossed_on_an_inner_layer() {
    let mut b = four_layer();
    pad(&mut b, 5.0, 10.0, "A", true);
    pad(&mut b, 25.0, 10.0, "A", true);
    track(&mut b, Side::Top, (15.0, 0.5), (15.0, 19.5));
    track(&mut b, Side::Bottom, (15.0, 0.5), (15.0, 19.5));
    let r = autoroute(&mut b, &RouteOptions::default(), None);
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    assert!(b.tracks.iter().any(|t| t.side.is_inner()), "the inner layer carries the crossing");
    assert!(ratsnest(&b).is_empty());
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
}

#[test]
fn the_same_wall_blocks_a_two_layer_board() {
    let mut b = Board::new("t", 30.0, 20.0);
    pad(&mut b, 5.0, 10.0, "A", true);
    pad(&mut b, 25.0, 10.0, "A", true);
    track(&mut b, Side::Top, (15.0, 0.5), (15.0, 19.5));
    track(&mut b, Side::Bottom, (15.0, 0.5), (15.0, 19.5));
    assert_eq!(autoroute(&mut b, &RouteOptions::default(), None).failed.len(), 1);
}

#[test]
fn the_fab_package_has_a_file_for_every_inner_layer() {
    let mut b = four_layer();
    pad(&mut b, 5.0, 10.0, "A", true);
    track(&mut b, Side::Inner(1), (5.0, 10.0), (20.0, 10.0));
    let files = export::fab_files(&b);
    let names: Vec<_> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"t.G1") && names.contains(&"t.G2"), "{names:?}");
    let g1 = &files.iter().find(|(n, _)| n == "t.G1").unwrap().1;
    assert!(g1.contains("Copper,L2,Inr"), "{g1}");
    let gbl = &files.iter().find(|(n, _)| n == "t.GBL").unwrap().1;
    assert!(gbl.contains("Copper,L4,Bot"), "the bottom layer is the last: {gbl}");
}
