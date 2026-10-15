use crate::padentry::*;
use crate::ratsnest::ratsnest;
use crate::*;

fn pad(b: &mut Board, x: f64, y: f64, w: f64, h: f64, net: &str) {
    let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w, h }, None, Side::Top);
    p.net = net.into();
    b.pads.push(p);
}

fn trk(a: (f64, f64), z: (f64, f64)) -> Track {
    Track { side: Side::Top, a: Point::new(a.0, a.1), b: Point::new(z.0, z.1), width: 0.25 }
}

fn leaves_at_right_angle(b: &Board, pad_pos: Point) -> bool {
    // some track starts (or ends) at `pad_pos`-ish and runs straight out of the pad's edge
    b.tracks.iter().all(|t| {
        let (v, w) = if t.a.dist(pad_pos) < 1.0 { (t.a, t.b) } else { return true };
        let (dx, dy) = (w.x - v.x, w.y - v.y);
        dx.abs() < 1e-9 || dy.abs() < 1e-9 || dx.hypot(dy) < 0.3
    })
}

#[test]
fn a_diagonal_track_gets_a_lead_at_a_right_angle_out_of_the_pad() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 10.0, 1.0, 1.0, "A");
    pad(&mut b, 25.0, 25.0, 1.0, 1.0, "A");
    // from the pad's centre straight at 45 degrees up to (20, 25), then along to the other pad
    b.tracks.push(trk((5.0, 10.0), (20.0, 25.0)));
    b.tracks.push(trk((20.0, 25.0), (25.0, 25.0)));
    let before: f64 = b.tracks.iter().map(|t| t.a.dist(t.b)).sum();
    let r = align_entries(&mut b, &EntryOptions::default(), None);
    assert_eq!(r.aligned, 1, "{r:?}");
    assert!(ratsnest(&b).is_empty(), "still connected");
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    // the first track starts at the pad's centre and runs straight out (horizontal or vertical) for the lead
    let first = b.tracks.iter().find(|t| t.a == Point::new(5.0, 10.0)).unwrap();
    let (dx, dy) = ((first.b.x - first.a.x).abs(), (first.b.y - first.a.y).abs());
    assert!(dx < 1e-9 || dy < 1e-9, "{first:?}");
    assert!(first.a.dist(first.b) >= 0.5 + 0.5 - 1e-9, "the pad's half size 0.5 plus the lead 0.5: {first:?}");
    assert!(leaves_at_right_angle(&b, Point::new(5.0, 10.0)));
    let after: f64 = b.tracks.iter().map(|t| t.a.dist(t.b)).sum();
    assert!((after - before).abs() < 2.0, "the route is changed only near the pad: {before} {after}");
    // doing it again changes nothing
    let n = b.tracks.len();
    assert_eq!(align_entries(&mut b, &EntryOptions::default(), None), EntryReport::default());
    assert_eq!(b.tracks.len(), n);
}

#[test]
fn a_track_that_already_leaves_straight_is_left_alone() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 10.0, 1.0, 1.0, "A");
    pad(&mut b, 25.0, 10.0, 1.0, 1.0, "A");
    b.tracks.push(trk((5.0, 10.0), (25.0, 10.0)));
    assert_eq!(align_entries(&mut b, &EntryOptions::default(), None), EntryReport::default());
    assert_eq!(b.tracks.len(), 1);
}

#[test]
fn two_tracks_from_the_centre_of_a_wide_pad_are_moved_to_its_two_ends() {
    let mut b = Board::new("t", 50.0, 30.0);
    pad(&mut b, 10.0, 15.0, 3.0, 0.8, "A");
    pad(&mut b, 30.0, 25.0, 1.0, 1.0, "A");
    pad(&mut b, 30.0, 5.0, 1.0, 1.0, "A");
    b.tracks.push(trk((10.0, 15.0), (30.0, 25.0)));
    b.tracks.push(trk((10.0, 15.0), (30.0, 5.0)));
    let r = align_entries(&mut b, &EntryOptions::default(), None);
    assert_eq!(r.spread, 1, "{r:?}");
    assert!(ratsnest(&b).is_empty() && drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    // the starts are now at the two ends of the pad (x = 10 +- 1.325), not at the centre
    let starts: Vec<Point> = b.tracks.iter().map(|t| t.a).filter(|p| p.dist(Point::new(10.0, 15.0)) < 2.0).collect();
    assert!(starts.iter().any(|p| (p.x - 8.675).abs() < 1e-6) && starts.iter().any(|p| (p.x - 11.325).abs() < 1e-6), "{starts:?}");
    assert!(starts.iter().all(|p| p.dist(Point::new(10.0, 15.0)) > 1.0 || (p.y - 15.0).abs() > 0.2 || (p.x - 10.0).abs() > 1.0));
}

#[test]
fn the_pads_of_a_net_that_is_not_listed_can_be_left_out_and_round_pads_are_skipped() {
    let mut b = Board::new("t", 40.0, 30.0);
    let mut round = Pad::new(Point::new(5.0, 10.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
    round.net = "A".into();
    b.pads.push(round);
    pad(&mut b, 25.0, 25.0, 1.0, 1.0, "A");
    b.tracks.push(trk((5.0, 10.0), (20.0, 25.0)));
    b.tracks.push(trk((20.0, 25.0), (25.0, 25.0)));
    // a round pad has no edge to be at a right angle to
    let r = align_entries(&mut b, &EntryOptions::default(), None);
    assert_eq!(r.aligned, 0);
    // only the listed tracks are handled
    let mut c = Board::new("t", 40.0, 30.0);
    pad(&mut c, 5.0, 10.0, 1.0, 1.0, "A");
    pad(&mut c, 25.0, 25.0, 1.0, 1.0, "A");
    c.tracks.push(trk((5.0, 10.0), (20.0, 25.0)));
    c.tracks.push(trk((20.0, 25.0), (25.0, 25.0)));
    assert_eq!(align_entries(&mut c, &EntryOptions::default(), Some(&[1])).aligned, 0, "track 1 has no end on the pad at (5, 10)");
    assert_eq!(align_entries(&mut c, &EntryOptions::default(), Some(&[0])).aligned, 1);
}
