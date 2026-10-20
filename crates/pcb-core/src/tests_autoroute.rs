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
    assert!(drc::acute_angle_checks(&b).is_empty(), "{:?}", drc::acute_angle_checks(&b));
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
    assert!(drc::acute_angle_checks(&b).is_empty(), "{:?}", drc::acute_angle_checks(&b));
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
    assert!(drc::acute_angle_checks(&b).is_empty(), "{:?}", drc::acute_angle_checks(&b));
}

#[test]
fn vias_are_not_put_on_pads() {
    let mut b = Board::new("t", 30.0, 20.0);
    pad(&mut b, 5.0, 10.0, "A", false);
    pad(&mut b, 25.0, 10.0, "A", false);
    // the top is walled at x=15 and the bottom at x=11, so the only place to change layer is between them,
    // where a big surface-mount pad of the same net sits: a via there would land on the pad
    let mut big = Pad::new(Point::new(13.0, 10.0), PadShape::Circle { d: 2.4 }, None, Side::Top);
    big.net = "A".into();
    b.pads.push(big);
    track(&mut b, Side::Top, (15.0, 0.5), (15.0, 19.5));
    track(&mut b, Side::Bottom, (11.0, 0.5), (11.0, 19.5));
    let _ = autoroute(&mut b, &RouteOptions::default(), None);
    for v in &b.vias {
        for p in &b.pads {
            let (w, h) = p.shape.size();
            assert!(v.pos.dist(p.pos) > v.diameter / 2.0 + w.min(h) / 2.0 - 1e-6, "via at {:?} sits on pad at {:?}", v.pos, p.pos);
        }
    }
}

fn lead_options() -> RouteOptions {
    RouteOptions { grid: 0.25, pad_lead: Some(0.5), ..Default::default() }
}

/// No two tracks that meet leave their meeting point the same way (a stub that runs out and returns on itself).
fn no_backtrack(b: &Board) -> bool {
    b.tracks.iter().enumerate().all(|(i, t)| {
        b.tracks.iter().skip(i + 1).all(|u| {
            [(t.a, t.b), (t.b, t.a)].iter().all(|&(v, tw)| {
                [(u.a, u.b), (u.b, u.a)].iter().all(|&(x, uw)| {
                    if v.dist(x) > 1e-9 {
                        return true;
                    }
                    let (d1, d2) = ((tw.x - v.x, tw.y - v.y), (uw.x - v.x, uw.y - v.y));
                    let (l1, l2) = (d1.0.hypot(d1.1), d2.0.hypot(d2.1));
                    l1 < 1e-9 || l2 < 1e-9 || (d1.0 * d2.0 + d1.1 * d2.1) / (l1 * l2) < 0.999
                })
            })
        })
    })
}

#[test]
fn a_pad_off_the_grid_leaves_with_its_lead_slid_onto_the_grid_line_not_with_a_jog() {
    let mut b = Board::new("t", 30.0, 20.0);
    pad(&mut b, 5.0875, 10.0, "A", false); // 0.0875 mm off the 0.25 mm grid column
    pad(&mut b, 25.0, 14.0, "A", false);
    let r = autoroute(&mut b, &lead_options(), None);
    assert_eq!((r.routed, r.failed.len()), (1, 0));
    assert!(ratsnest(&b).is_empty());
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    let steps: Vec<_> = b.tracks.iter().filter(|t| t.a.dist(t.b) < 0.25 && (t.b.x - t.a.x).abs() > 1e-6 && (t.b.y - t.a.y).abs() > 1e-6).collect();
    assert!(steps.is_empty(), "no tiny diagonal: {steps:?}");
    // the track leaves the pad at a right angle
    let first = b.tracks.iter().find(|t| t.a == Point::new(5.0875, 10.0) || t.b == Point::new(5.0875, 10.0) || (t.a.x - 5.0875).abs() < 0.2 && (t.a.y - 10.0).abs() < 0.2).unwrap();
    assert!((first.a.x - first.b.x).abs() < 1e-9 || (first.a.y - first.b.y).abs() < 1e-9, "{first:?}");
}

#[test]
fn a_lead_does_not_run_past_the_grid_cell_and_back() {
    let mut b = Board::new("t", 30.0, 30.0);
    // the lead's end would be at x = 13.105, the nearest grid column is x = 13.0: the route turns there
    let mut p = Pad::new(Point::new(12.15, 21.0), PadShape::Rect { w: 0.91, h: 1.22 }, None, Side::Top);
    p.net = "A".into();
    b.pads.push(p);
    pad(&mut b, 13.0, 27.0, "A", false);
    // other nets above and below the pad: the lead can only go sideways
    pad(&mut b, 12.15, 22.7, "B", false);
    pad(&mut b, 12.15, 19.3, "B", false);
    let r = autoroute(&mut b, &lead_options(), Some("A"));
    assert_eq!((r.routed, r.failed.len()), (1, 0));
    assert!(no_backtrack(&b), "{:?}", b.tracks);
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
}

#[test]
fn a_net_that_cannot_be_routed_wide_falls_back_to_the_plain_run() {
    let mut b = Board::new("t", 50.0, 30.0);
    pad(&mut b, 5.0, 14.5, "A", false);
    pad(&mut b, 45.0, 14.5, "A", false);
    // two big pads of other nets (from the board's edge to 1.5 mm off the line) leave a 3 mm channel: 5 A needs a 2.8 mm track
    // plus clearances, which does not fit, and there is no way round
    for (y, net) in [(6.5, "C"), (22.5, "D")] {
        // through-hole: they block both layers, so the wide track cannot go under them on the bottom
        let mut p = Pad::new(Point::new(25.0, y), PadShape::Rect { w: 10.0, h: 13.0 }, Some(1.0), Side::Top);
        p.net = net.into();
        b.pads.push(p);
    }
    b.net_currents.insert("A".into(), 5.0);
    let opt = RouteOptions::default();
    let r = autoroute_sized(&mut b, &opt, None);
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    assert!(ratsnest(&b).is_empty());
    assert!(r.wide.is_empty(), "the plain run was kept: {:?}", r.wide);
    assert!(b.tracks.iter().all(|t| (t.width - opt.track_width).abs() < 1e-9));
    // with room, the same net is routed wide
    let mut c = Board::new("t", 50.0, 30.0);
    pad(&mut c, 5.0, 14.5, "A", false);
    pad(&mut c, 45.0, 14.5, "A", false);
    c.net_currents.insert("A".into(), 5.0);
    let r = autoroute_sized(&mut c, &opt, None);
    assert!(r.failed.is_empty() && r.wide.len() == 1 && r.wide[0].1 >= 2.5, "{:?}", r.wide);
    assert!(c.tracks.iter().all(|t| t.width >= 2.5));
}
