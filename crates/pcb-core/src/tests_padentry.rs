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

/// The tracks as a polyline from `from` (following the joined ends), as points.
fn polyline(b: &Board, from: Point) -> Vec<Point> {
    let mut pts = vec![from];
    let mut used = vec![false; b.tracks.len()];
    loop {
        let cur = *pts.last().unwrap();
        let Some((i, t)) = b.tracks.iter().enumerate().find(|(i, t)| !used[*i] && (t.a.dist(cur) < 1e-9 || t.b.dist(cur) < 1e-9)) else { break };
        used[i] = true;
        pts.push(if t.a.dist(cur) < 1e-9 { t.b } else { t.a });
    }
    pts
}

fn sharpest_turn_deg(pts: &[Point]) -> f64 {
    pts.windows(3)
        .map(|w| {
            let (u, v) = ((w[1].x - w[0].x, w[1].y - w[0].y), (w[2].x - w[1].x, w[2].y - w[1].y));
            let c = (u.0 * v.0 + u.1 * v.1) / (u.0.hypot(u.1) * v.0.hypot(v.1));
            c.clamp(-1.0, 1.0).acos().to_degrees()
        })
        .fold(0.0, f64::max)
}

#[test]
fn a_diagonal_that_runs_on_gets_no_sharp_fold_at_the_pad() {
    let mut b = Board::new("t", 50.0, 40.0);
    pad(&mut b, 5.0, 10.0, 1.0, 1.0, "A");
    pad(&mut b, 25.0, 30.0, 1.0, 1.0, "A");
    // one straight diagonal, in two tracks that meet at (15, 20)
    b.tracks.push(trk((5.0, 10.0), (15.0, 20.0)));
    b.tracks.push(trk((15.0, 20.0), (25.0, 30.0)));
    let r = align_entries(&mut b, &EntryOptions::default(), None);
    assert!(r.aligned >= 1, "{r:?}");
    assert!(ratsnest(&b).is_empty() && drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    let pts = polyline(&b, Point::new(5.0, 10.0));
    let turn = sharpest_turn_deg(&pts);
    assert!(turn < 50.0, "the sharpest turn is {turn} degrees: {pts:?}");
}

#[test]
fn two_tracks_spread_on_a_wide_pad_are_laid_with_gentle_folds() {
    let mut b = Board::new("t", 50.0, 30.0);
    pad(&mut b, 10.0, 15.0, 3.0, 0.8, "A");
    pad(&mut b, 30.0, 25.0, 1.0, 1.0, "A");
    pad(&mut b, 30.0, 5.0, 1.0, 1.0, "A");
    b.tracks.push(trk((10.0, 15.0), (30.0, 25.0)));
    b.tracks.push(trk((10.0, 15.0), (30.0, 5.0)));
    let r = align_entries(&mut b, &EntryOptions::default(), None);
    assert_eq!(r.spread, 1, "{r:?}");
    assert!(ratsnest(&b).is_empty() && drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    // each track runs from its end of the pad to the far pad, and no fold along the way is sharper than 50 degrees
    for anchor in [Point::new(8.675, 15.0), Point::new(11.325, 15.0)] {
        let pts = polyline(&b, anchor);
        assert!(pts.len() >= 3, "{pts:?}");
        let turn = sharpest_turn_deg(&pts);
        assert!(turn < 50.0, "from {anchor:?}: the sharpest turn is {turn} degrees: {pts:?}");
    }
}

/// No piece of the route near the pad is a tiny diagonal: the little step that a pad a hair off the grid line used to get.
fn has_tiny_step(b: &Board) -> bool {
    b.tracks.iter().any(|t| {
        let (dx, dy) = ((t.b.x - t.a.x).abs(), (t.b.y - t.a.y).abs());
        t.a.dist(t.b) < 0.25 && dx > 1e-6 && dy > 1e-6
    })
}

#[test]
fn a_pad_a_hair_off_the_grid_line_slides_the_lead_instead_of_making_a_jog() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 21.4125, 7.0, 0.6, 1.0, "A");
    pad(&mut b, 21.5, 20.0, 1.0, 1.0, "A");
    // the router came from the grid column x = 21.5 to the pad's centre: a slightly slanted foot, then straight up
    b.tracks.push(trk((21.4125, 7.0), (21.5, 7.75)));
    b.tracks.push(trk((21.5, 7.75), (21.5, 20.0)));
    let r = align_entries(&mut b, &EntryOptions::default(), None);
    assert_eq!(r.aligned, 1, "{r:?}");
    assert!(!has_tiny_step(&b), "{:?}", b.tracks);
    assert!(ratsnest(&b).is_empty(), "still connected");
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    // one straight line up the grid column, starting on the pad
    assert!(b.tracks.iter().all(|t| (t.a.x - 21.5).abs() < 1e-9 && (t.b.x - 21.5).abs() < 1e-9), "{:?}", b.tracks);
    assert!(b.tracks.iter().any(|t| (t.a.y - 7.0).abs() < 1e-9 || (t.b.y - 7.0).abs() < 1e-9));
}

#[test]
fn a_jog_that_would_be_a_tiny_step_is_not_made_when_the_pad_cannot_slide() {
    let mut b = Board::new("t", 40.0, 30.0);
    // a narrow pad: the foot cannot move 0.2 mm sideways and keep the track on the pad
    pad(&mut b, 21.3, 7.0, 0.3, 1.0, "A");
    pad(&mut b, 21.5, 20.0, 1.0, 1.0, "A");
    b.tracks.push(trk((21.3, 7.0), (21.5, 7.75)));
    b.tracks.push(trk((21.5, 7.75), (21.5, 20.0)));
    align_entries(&mut b, &EntryOptions::default(), None);
    assert!(!has_tiny_step(&b), "{:?}", b.tracks);
    assert!(ratsnest(&b).is_empty(), "still connected");
}

#[test]
fn a_track_that_sticks_out_over_the_pad_edge_is_moved_onto_the_pad() {
    // a 0.3 mm wide pin with a 0.25 mm track that ends 0.075 mm off its centre line: one edge of the track is outside the pad
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 10.0, 10.0, 0.3, 0.85, "A");
    pad(&mut b, 20.0, 18.0, 1.0, 1.0, "A");
    b.tracks.push(trk((9.925, 10.0), (9.925, 14.0)));
    b.tracks.push(trk((9.925, 14.0), (13.925, 18.0)));
    b.tracks.push(trk((13.925, 18.0), (20.0, 18.0)));
    let overhang = |b: &Board| b.tracks.iter().flat_map(|t| [t.a, t.b]).filter(|p| (p.y - 10.0).abs() < 0.43).map(|p| (p.x - 10.0).abs() + 0.125 - 0.15).fold(f64::MIN, f64::max);
    assert!(overhang(&b) > 0.04);
    let r = align_entries(&mut b, &EntryOptions::default(), None);
    assert!(r.aligned >= 1, "{r:?}");
    assert!(overhang(&b) < 1e-6, "{:?}", b.tracks);
    assert!(ratsnest(&b).is_empty());
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
}

#[test]
fn a_via_a_hair_from_its_own_pad_gets_the_gap_filled() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 10.0, 10.0, 0.3, 0.85, "A");
    pad(&mut b, 10.0, 20.0, 1.0, 1.0, "A");
    b.tracks.push(trk((10.0, 10.0), (10.0, 20.0)));
    // a via of the same net beside the pad's corner, 0.07 mm from it
    b.vias.push(Via { pos: Point::new(10.545, 10.9), diameter: 0.6, drill: 0.3 });
    b.tracks.push(trk((10.0, 11.5), (10.545, 10.9)));
    assert!(!drc::slit_checks(&b).is_empty());
    let n = drc::fill_slits(&mut b);
    assert!(n >= 1);
    assert!(drc::slit_checks(&b).is_empty(), "{:?}", drc::slit_checks(&b));
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
}
