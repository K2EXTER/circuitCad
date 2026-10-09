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

#[test]
fn two_pads_are_joined_without_breaking_a_rule() {
    let mut b = Board::new("t", 30.0, 20.0);
    pad(&mut b, 5.0, 10.0, "A", false);
    pad(&mut b, 25.0, 10.0, "A", false);
    let r = autoroute(&mut b, &RouteOptions::default(), None);
    assert_eq!((r.routed, r.failed.len()), (1, 0));
    assert!(ratsnest(&b).is_empty());
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    assert!(b.vias.is_empty(), "a free straight path needs no via");
    assert!(b.tracks.len() <= 3, "straight runs are merged: {}", b.tracks.len());
}

#[test]
fn a_wall_of_another_net_is_crossed_with_vias() {
    let mut b = Board::new("t", 30.0, 20.0);
    pad(&mut b, 5.0, 10.0, "A", false);
    pad(&mut b, 25.0, 10.0, "A", false);
    // a wall of net B on the top layer, from edge to edge
    track(&mut b, Side::Top, (15.0, 0.5), (15.0, 19.5));
    let r = autoroute(&mut b, &RouteOptions::default(), None);
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    assert!(ratsnest(&b).is_empty());
    assert_eq!(b.vias.len(), 2, "down and up again");
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
}

#[test]
fn a_blocked_connection_is_reported_and_the_board_is_left_alone() {
    let mut b = Board::new("t", 30.0, 20.0);
    pad(&mut b, 5.0, 10.0, "A", true);
    pad(&mut b, 25.0, 10.0, "A", true);
    // a wall on both layers, edge to edge
    track(&mut b, Side::Top, (15.0, 0.5), (15.0, 19.5));
    track(&mut b, Side::Bottom, (15.0, 0.5), (15.0, 19.5));
    let before = b.tracks.len();
    let r = autoroute(&mut b, &RouteOptions::default(), None);
    assert_eq!((r.routed, r.failed.len()), (0, 1));
    assert_eq!(b.tracks.len(), before);
    assert_eq!(ratsnest(&b).len(), 1);
}

#[test]
fn only_the_requested_net_is_routed() {
    let mut b = Board::new("t", 30.0, 20.0);
    pad(&mut b, 5.0, 5.0, "A", false);
    pad(&mut b, 25.0, 5.0, "A", false);
    pad(&mut b, 5.0, 15.0, "B", false);
    pad(&mut b, 25.0, 15.0, "B", false);
    let r = autoroute(&mut b, &RouteOptions::default(), Some("B"));
    assert_eq!(r.routed, 1);
    let left = ratsnest(&b);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].net, "A");
    assert!(drc::check(&b).is_empty());
}

#[test]
fn many_nets_cross_each_other_cleanly() {
    let mut b = Board::new("t", 40.0, 30.0);
    // four nets that must cross: left column to right column in reverse order
    for (k, net) in ["N1", "N2", "N3", "N4"].iter().enumerate() {
        pad(&mut b, 5.0, 6.0 + 6.0 * k as f64, net, true);
        pad(&mut b, 35.0, 24.0 - 6.0 * k as f64, net, true);
    }
    let r = autoroute(&mut b, &RouteOptions::default(), None);
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    assert!(ratsnest(&b).is_empty());
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
}
