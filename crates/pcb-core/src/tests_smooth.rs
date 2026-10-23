use crate::ratsnest::ratsnest;
use crate::smooth::*;
use crate::*;

/// A small pad (a track is not narrowed towards it).
fn pad(b: &mut Board, x: f64, y: f64, net: &str) {
    pad_sized(b, x, y, net, 0.3);
}

fn pad_sized(b: &mut Board, x: f64, y: f64, net: &str, size: f64) {
    let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: size, h: size }, None, Side::Top);
    p.net = net.into();
    b.pads.push(p);
}

fn trk(a: (f64, f64), z: (f64, f64), w: f64) -> Track {
    Track { side: Side::Top, a: Point::new(a.0, a.1), b: Point::new(z.0, z.1), width: w }
}

/// A right-angle route from (5,5) to (25,20) through (25,5).
fn l_shape() -> Board {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 5.0, "A");
    pad(&mut b, 25.0, 20.0, "A");
    b.tracks.push(trk((5.0, 5.0), (25.0, 5.0), 0.25));
    b.tracks.push(trk((25.0, 5.0), (25.0, 20.0), 0.25));
    b
}

#[test]
fn a_right_angle_corner_becomes_an_arc() {
    let mut b = l_shape();
    let r = smooth_tracks(&mut b, &SmoothOptions::default());
    assert_eq!(r, SmoothReport { corners: 1, ..Default::default() });
    assert!(b.tracks.len() >= 5, "two trimmed tracks and at least 3 arc pieces");
    assert!(ratsnest(&b).is_empty(), "still connected");
    assert!(drc::check(&b).is_empty());
    // the old corner point is gone: no track has an end there
    let corner = Point::new(25.0, 5.0);
    assert!(b.tracks.iter().all(|t| t.a.dist(corner) > 0.1 && t.b.dist(corner) > 0.1));
    // the arc cuts the corner: some track ends lie strictly inside it
    let inside = b.tracks.iter().filter(|t| t.a.x < 24.999 && t.a.y > 5.001 && t.a.x > 23.9 && t.a.y < 6.1).count();
    assert!(inside >= 2, "{inside}");
}

#[test]
fn a_straight_join_is_left_alone() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 5.0, "A");
    pad(&mut b, 25.0, 5.0, "A");
    b.tracks.push(trk((5.0, 5.0), (15.0, 5.0), 0.25));
    b.tracks.push(trk((15.0, 5.0), (25.0, 5.0), 0.25));
    assert_eq!(smooth_tracks(&mut b, &SmoothOptions::default()), SmoothReport::default());
    assert_eq!(b.tracks.len(), 2);
}

#[test]
fn a_short_track_gives_a_smaller_radius() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 5.0, "A");
    pad(&mut b, 12.0, 6.0, "A");
    b.tracks.push(trk((5.0, 5.0), (12.0, 5.0), 0.25));
    b.tracks.push(trk((12.0, 5.0), (12.0, 6.0), 0.25)); // 1mm long: half of it is 0.5mm
    let r = smooth_tracks(&mut b, &SmoothOptions { radius: 3.0, ..Default::default() });
    assert_eq!(r.corners, 1);
    assert!(b.tracks[1].a.dist(b.tracks[1].b) >= 0.49, "the short track keeps its other half");
    assert!(ratsnest(&b).is_empty());
}

#[test]
fn a_width_change_is_stepped() {
    let mut b = Board::new("t", 40.0, 30.0);
    // a pad as wide as the track, so that it needs no neck (a track wider than its pad gets one: `a_wide_track_on_a_narrow_pad_...`)
    pad_sized(&mut b, 5.0, 5.0, "A", 0.8);
    pad(&mut b, 25.0, 5.0, "A");
    b.tracks.push(trk((5.0, 5.0), (15.0, 5.0), 0.8));
    b.tracks.push(trk((15.0, 5.0), (25.0, 5.0), 0.25));
    let r = smooth_tracks(&mut b, &SmoothOptions { corners: false, ..Default::default() });
    assert_eq!(r.tapers, 1);
    let mut widths: Vec<f64> = b.tracks.iter().map(|t| t.width).collect();
    widths.sort_by(|a, b| b.partial_cmp(a).unwrap());
    widths.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    assert!(widths.len() >= 4, "in-between widths exist: {widths:?}");
    assert!(widths.iter().all(|w| *w <= 0.8 && *w >= 0.25));
    assert!(ratsnest(&b).is_empty());
    assert!(drc::check(&b).is_empty());
}

#[test]
fn smoothing_never_adds_rule_breaks() {
    let mut b = l_shape();
    // another net's track in the inside of the corner
    pad(&mut b, 5.0, 25.0, "B");
    pad(&mut b, 24.0, 25.0, "B");
    b.tracks.push(trk((24.55, 5.55), (24.6, 5.6), 0.1));
    let (drc_before, open_before) = (drc::check(&b).len(), ratsnest(&b).len());
    smooth_tracks(&mut b, &SmoothOptions::default());
    assert!(drc::check(&b).len() <= drc_before);
    assert!(ratsnest(&b).len() <= open_before);
}

#[test]
fn a_track_meeting_a_pad_narrows_from_the_pad_width() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad_sized(&mut b, 5.0, 5.0, "A", 1.0);
    pad_sized(&mut b, 25.0, 5.0, "A", 1.0);
    b.tracks.push(trk((5.0, 5.0), (25.0, 5.0), 0.25));
    let r = smooth_tracks(&mut b, &SmoothOptions::default());
    assert_eq!(r.tapers, 2, "one at each pad");
    assert!(b.tracks.iter().any(|t| t.width > 0.8), "starts as wide as the 1mm pad");
    assert!(b.tracks.iter().all(|t| t.width >= 0.25 && t.width <= 1.0 + 1e-9));
    assert!(ratsnest(&b).is_empty());
    assert!(drc::check(&b).is_empty());
    // pressing again changes nothing
    let n = b.tracks.len();
    assert_eq!(smooth_tracks(&mut b, &SmoothOptions::default()), SmoothReport::default());
    assert_eq!(b.tracks.len(), n);
}

#[test]
fn a_short_track_at_a_pad_is_left_as_it_is() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 5.0, "A");
    pad(&mut b, 7.0, 5.0, "A");
    b.tracks.push(trk((5.0, 5.0), (7.0, 5.0), 0.25));
    assert_eq!(smooth_tracks(&mut b, &SmoothOptions::default()).tapers, 0);
}

#[test]
fn a_round_pad_gets_the_teardrop_from_its_centre_not_a_wide_base_beyond_its_edge() {
    let mut b = Board::new("t", 40.0, 30.0);
    let mut p = Pad::new(Point::new(5.0, 5.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
    p.net = "A".into();
    b.pads.push(p);
    pad(&mut b, 25.0, 15.0, "A");
    b.tracks.push(trk((5.0, 5.0), (25.0, 15.0), 0.25));
    smooth_tracks(&mut b, &SmoothOptions::default());
    let at_pad: Vec<&Track> = b.tracks.iter().filter(|t| t.a.dist(Point::new(5.0, 5.0)) < 1e-6 || t.b.dist(Point::new(5.0, 5.0)) < 1e-6).collect();
    assert!(at_pad.iter().any(|t| (t.width - 1.6).abs() < 1e-9), "as wide as the pad at its centre");
    assert!(b.tracks.iter().all(|t| t.width <= 1.6 + 1e-9));
    assert!(ratsnest(&b).is_empty());
}

#[test]
fn only_the_selected_track_is_smoothed() {
    let mut b = l_shape();
    // a second right-angle route, far away
    pad(&mut b, 5.0, 25.0, "B");
    pad(&mut b, 20.0, 28.0, "B");
    b.tracks.push(trk((5.0, 25.0), (20.0, 25.0), 0.25));
    b.tracks.push(trk((20.0, 25.0), (20.0, 28.0), 0.25));
    let r = smooth_selected(&mut b, &SmoothOptions::default(), &[2]);
    assert_eq!(r.corners, 1);
    // the first route's corner is untouched
    assert!(b.tracks.iter().any(|t| t.a == Point::new(25.0, 5.0) || t.b == Point::new(25.0, 5.0)));
    assert!(b.tracks.iter().all(|t| t.a != Point::new(20.0, 25.0) && t.b != Point::new(20.0, 25.0)));
    assert_eq!(smooth_selected(&mut b, &SmoothOptions::default(), &[]), SmoothReport::default());
}

/// Two tracks leaving a small pad at 30 degrees from each other.
fn fork_board() -> Board {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 15.0, "A");
    let a = 15f64.to_radians();
    b.tracks.push(trk((5.0, 15.0), (5.0 + 10.0 * a.cos(), 15.0 + 10.0 * a.sin()), 0.25));
    b.tracks.push(trk((5.0, 15.0), (5.0 + 10.0 * a.cos(), 15.0 - 10.0 * a.sin()), 0.25));
    pad(&mut b, 5.0 + 10.0 * a.cos(), 15.0 + 10.0 * a.sin(), "A");
    pad(&mut b, 5.0 + 10.0 * a.cos(), 15.0 - 10.0 * a.sin(), "A");
    b
}

#[test]
fn the_notch_between_two_tracks_at_a_sharp_angle_is_filled_with_a_concave_round_end() {
    let mut b = fork_board();
    assert!(finished(&b).is_none(), "off: the board is used as it is");
    b.finish = Finish { on: true, corners: false, tapers: false, forks: true, ..Default::default() };
    let before = b.tracks.clone();
    let f = finished(&b).unwrap();
    assert_eq!(f.patches.len(), 1);
    assert!(f.board.tracks.iter().zip(&before).all(|(x, y)| x.a == y.a && x.b == y.b), "the routes are not changed");
    let poly = &f.patches[0].outline;
    // the free circle (radius = track width) that touches both tracks' edges: the patch does not reach into it
    let theta = 15f64.to_radians();
    let centre = Point::new(5.0 + (0.25 + 0.125) / theta.sin(), 15.0);
    assert!(!geom::inside_polygon(poly, centre), "the circle stays free");
    assert!(poly.iter().all(|p| p.dist(centre) >= 0.25 - 1e-9), "no corner of the patch inside the circle");
    // but the notch between the tip and the circle is filled
    assert!(geom::inside_polygon(poly, Point::new(5.0 + 0.9, 15.0)));
    assert!(ratsnest(&f.board).is_empty());
    assert!(drc::check_final(&b).is_empty());
    // the Gerber file has the patch as a region; with the finish off it does not
    assert!(gerber::copper(&b, Side::Top).contains("G36*"));
    b.finish.on = false;
    assert!(!gerber::copper(&b, Side::Top).contains("G36*"));
}

#[test]
fn the_finish_follows_the_tracks_when_they_move() {
    let mut b = fork_board();
    b.finish = Finish { on: true, corners: true, tapers: false, forks: false, ..Default::default() };
    b.tracks.push(trk((30.0, 5.0), (38.0, 5.0), 0.25));
    b.tracks.push(trk((38.0, 5.0), (38.0, 12.0), 0.25));
    let n = finished(&b).unwrap().board.tracks.len();
    assert!(n > b.tracks.len(), "the corner is rounded in the derived tracks only");
    assert_eq!(b.tracks.len(), 4, "the board's own tracks are as drawn");
    b.tracks[3].b = Point::new(38.0, 14.0);
    assert!(finished(&b).unwrap().board.tracks.len() >= n, "worked out again for the moved track");
}

#[test]
fn two_tracks_at_a_wide_angle_are_not_a_fork() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 15.0, "A");
    b.tracks.push(trk((5.0, 15.0), (15.0, 25.0), 0.25));
    b.tracks.push(trk((5.0, 15.0), (15.0, 5.0), 0.25)); // 90 degrees apart
    
}

#[test]
fn smoothing_one_selected_track_leaves_the_other_alone_even_on_a_shared_pad() {
    let mut b = fork_board(); // tracks 0 and 1 leave the same pad
    let lower_end = b.tracks[1].b;
    let n = b.tracks.len();
    smooth_selected(&mut b, &SmoothOptions { corners: false, ..Default::default() }, &[0]);
    assert_eq!(b.tracks[1].a, Point::new(5.0, 15.0), "the other track is untouched");
    assert_eq!(b.tracks[1].b, lower_end);
    assert!(b.tracks.len() >= n);
    // nothing new was added near the other track's far end
    assert!(b.tracks[n..].iter().all(|t| t.a.dist(lower_end) > 1.0 && t.b.dist(lower_end) > 1.0));
}

#[test]
fn two_tracks_from_one_pad_point_do_not_get_overlapping_teardrops() {
    let mut b = fork_board();
    for p in &mut b.pads {
        p.shape = PadShape::Rect { w: 1.0, h: 1.0 };
    }
    let n = b.tracks.len();
    smooth_tracks(&mut b, &SmoothOptions { corners: false, ..Default::default() });
    // no teardrop at the shared pad (5, 15), only at the two far pads (one each)
    let near = b.tracks[n..].iter().filter(|t| t.a.dist(Point::new(5.0, 15.0)) < 1.5 || t.b.dist(Point::new(5.0, 15.0)) < 1.5).count();
    assert_eq!(near, 0);
    assert!(b.tracks.len() > n, "the far pads still get theirs");
}

#[test]
fn an_excluded_net_is_left_as_drawn_and_the_other_is_finished() {
    let mut b = l_shape(); // net A
    pad(&mut b, 5.0, 25.0, "B");
    pad(&mut b, 20.0, 28.0, "B");
    b.tracks.push(trk((5.0, 25.0), (20.0, 25.0), 0.25));
    b.tracks.push(trk((20.0, 25.0), (20.0, 28.0), 0.25));
    b.finish_exclude = vec!["A".into()];
    let r = smooth_tracks(&mut b, &SmoothOptions::default());
    assert_eq!(r.corners, 1, "only net B's corner");
    assert!(b.tracks.iter().any(|t| t.a == Point::new(25.0, 5.0) || t.b == Point::new(25.0, 5.0)), "net A's corner is still sharp");
    // the same in the live finish
    let mut c = l_shape();
    c.finish = Finish { on: true, ..Default::default() };
    c.finish_exclude = vec!["A".into()];
    assert_eq!(finished(&c).unwrap().board.tracks.len(), c.tracks.len(), "nothing is added for an excluded net");
}

fn t_board(split: bool) -> Board {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 15.0, "A");
    pad(&mut b, 25.0, 15.0, "A");
    pad(&mut b, 15.0, 25.0, "A");
    if split {
        b.tracks.push(trk((5.0, 15.0), (15.0, 15.0), 0.25));
        b.tracks.push(trk((15.0, 15.0), (25.0, 15.0), 0.25));
    } else {
        b.tracks.push(trk((5.0, 15.0), (25.0, 15.0), 0.25));
    }
    b.tracks.push(trk((15.0, 15.0), (15.0, 25.0), 0.25)); // the branch ends on the trunk
    b.finish = Finish { on: true, corners: false, tapers: false, forks: true, ..Default::default() };
    b
}

#[test]
fn both_corners_of_a_t_get_a_concave_fillet() {
    for split in [false, true] {
        let b = t_board(split);
        let f = finished(&b).unwrap();
        assert_eq!(f.patches.len(), 2, "split = {split}: one corner on each side of the branch");
        // the corner on the right: apex (15.125, 15.125), fillet radius 0.5, so the free circle is centred at (15.625, 15.625)
        let right = f.patches.iter().find(|p| p.outline.iter().any(|q| q.x > 15.2)).unwrap();
        assert!(geom::inside_polygon(&right.outline, Point::new(15.2, 15.2)), "the corner is filled");
        assert!(!geom::inside_polygon(&right.outline, Point::new(15.625, 15.625)), "the arc stays free");
        assert!(right.outline.iter().all(|q| q.dist(Point::new(15.625, 15.625)) >= 0.5 - 1e-6 || q.y < 15.2 || q.x < 15.2 || q.dist(Point::new(15.125, 15.125)) < 0.2), "no corner of the fill reaches into the circle");
        assert!(ratsnest(&f.board).is_empty());
        assert!(drc::check_final(&b).is_empty());
        assert!(gerber::copper(&b, Side::Top).contains("G36*"));
    }
}

#[test]
fn a_plain_corner_is_not_a_junction_fillet() {
    let mut b = l_shape();
    b.finish = Finish { on: true, corners: false, tapers: false, forks: true, ..Default::default() };
    assert!(finished(&b).unwrap().patches.is_empty());
}

#[test]
fn a_corner_in_the_live_finish_is_a_band_of_the_exact_arc() {
    let mut b = l_shape();
    b.finish = Finish { on: true, corners: true, tapers: false, forks: false, radius: 1.0 };
    let f = finished(&b).unwrap();
    assert_eq!(f.patches.len(), 1);
    // the corner (25, 5) is cut by a circle of radius 1 centred at (24, 6): every point of the band lies between the
    // circles of radius 1 - 0.125 and 1 + 0.125 (the track is 0.25 wide), and the band follows them closely
    let c = Point::new(24.0, 6.0);
    let ds: Vec<f64> = f.patches[0].outline.iter().map(|p| p.dist(c)).collect();
    assert!(ds.iter().all(|d| (d - 0.875).abs() < 1e-9 || (d - 1.125).abs() < 1e-9), "{ds:?}");
    assert!(ds.len() >= 20, "sampled finely: {}", ds.len());
    // the tracks that keep it connected are few, and the board is still one piece
    assert!(f.board.tracks.len() <= 8, "two trimmed tracks and a coarse chain");
    assert!(ratsnest(&f.board).is_empty());
    assert!(drc::check_final(&b).is_empty());
}

#[test]
fn a_short_lead_that_turns_still_gets_a_shorter_teardrop() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad_sized(&mut b, 5.0, 5.0, "A", 1.0);
    pad_sized(&mut b, 6.6, 15.0, "A", 1.0);
    // 1.6 mm out of the pad, then up to the other pad: less than the full teardrop (1.5 x the width step) beside the pad
    b.tracks.push(trk((5.0, 5.0), (6.6, 5.0), 0.25));
    b.tracks.push(trk((6.6, 5.0), (6.6, 15.0), 0.25));
    let before = b.tracks.len();
    let r = smooth_tracks(&mut b, &SmoothOptions { corners: false, ..Default::default() });
    assert!(r.tapers >= 1, "{r:?}");
    assert!(b.tracks.len() > before);
    assert!(b.tracks.iter().any(|t| t.width > 0.8), "starts as wide as the pad");
    assert!(ratsnest(&b).is_empty());
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    // the teardrop does not run past the corner
    assert!(b.tracks.iter().filter(|t| t.width > 0.3).all(|t| t.a.x <= 6.6 + 1e-9 && t.b.x <= 6.6 + 1e-9));
}

#[test]
fn the_fillets_at_a_pad_stay_within_the_pads_edge_and_reach_its_corner_where_there_is_room_up_to_the_largest() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad_sized(&mut b, 5.0, 5.0, "A", 1.0);
    pad_sized(&mut b, 5.1, 12.0, "A", 1.0);
    // the track starts 0.1 mm to the side of the pad's centre
    b.tracks.push(trk((5.1, 5.0), (5.1, 12.0), 0.25));
    b.finish.on = true;
    let f = finished(&b).unwrap();
    assert!(!f.patches.is_empty(), "a fillet polygon");
    // every point of the polygon at the first pad lies within the pad's width (4.5 .. 5.5): on the far side the
    // fillet reaches the pad's corner (0.275 mm of room), on the near side it is the largest one (0.4 mm) from the track
    let near: Vec<&Point> = f.patches.iter().flat_map(|p| p.outline.iter()).filter(|p| p.y < 8.0).collect();
    assert!(near.iter().all(|p| p.x >= 4.5 - 1e-6 && p.x <= 5.5 + 1e-6), "{near:?}");
    let (lo, hi) = near.iter().fold((f64::MAX, f64::MIN), |(l, h), p| (l.min(p.x), h.max(p.x)));
    assert!((lo - (5.1 - 0.125 - crate::smooth::MAX_FILLET)).abs() < 1e-6 && (hi - 5.5).abs() < 1e-6, "{lo} {hi}");
    // the track itself is left as it is
    assert_eq!(f.board.tracks.len(), b.tracks.len());
}

#[test]
fn a_fillet_is_concave_and_its_base_is_flat_on_the_pads_edge() {
    let mut b = Board::new("t", 40.0, 30.0);
    // a 1.025 x 1.4 mm pad (an 0805) with a track leaving through its top edge (y = 5.7), 0.25 mm wide
    let mut p = Pad::new(Point::new(5.0, 5.0), PadShape::Rect { w: 1.025, h: 1.4 }, None, Side::Top);
    p.net = "A".into();
    b.pads.push(p);
    pad_sized(&mut b, 5.0, 14.0, "A", 1.0);
    b.tracks.push(trk((5.0, 5.0), (5.0, 14.0), 0.25));
    b.finish.on = true;
    let f = finished(&b).unwrap();
    let patch = f.patches.iter().find(|p| p.outline.iter().any(|q| q.y < 8.0)).expect("the fillet at the first pad");
    // the pad's edge is at y = 5.7 (above the track's end): the base lies on it, as wide as the pad (corner to corner)
    let on_edge: Vec<&Point> = patch.outline.iter().filter(|q| (q.y - 5.7).abs() < 1e-6).collect();
    let (x0, x1) = on_edge.iter().fold((f64::MAX, f64::MIN), |(l, h), q| (l.min(q.x), h.max(q.x)));
    assert!((x0 - (5.0 - 0.5125)).abs() < 1e-6 && (x1 - (5.0 + 0.5125)).abs() < 1e-6, "{x0} {x1}");
    // no part sticks out of the pad's width
    assert!(patch.outline.iter().all(|q| (q.x - 5.0).abs() <= 0.5125 + 1e-6));
    // the arc is concave: the middle of the chord from the pad's corner to the track's edge is free of copper, and a
    // point close to the sharp corner between the pad's edge and the track is copper
    let r = 0.3875; // the room to the corner: half the pad's width less half the track's
    let chord_mid = Point::new(5.0 - 0.125 - r / 2.0, 5.7 + r / 2.0);
    assert!(!crate::geom::inside_polygon(&patch.outline, chord_mid), "{chord_mid:?} should be free");
    let corner = Point::new(5.0 - 0.125 - 0.05, 5.7 + 0.05);
    assert!(crate::geom::inside_polygon(&patch.outline, corner), "{corner:?} should be copper");
}

#[test]
fn two_tracks_leaving_one_pad_side_by_side_get_one_round_arch_between_them() {
    let mut b = Board::new("t", 40.0, 30.0);
    let mut p = Pad::new(Point::new(5.0, 5.0), PadShape::Rect { w: 1.025, h: 1.4 }, None, Side::Top);
    p.net = "A".into();
    b.pads.push(p);
    for x in [4.6125, 5.3875] {
        pad_sized(&mut b, x, 14.0, "A", 1.0);
    }
    // flush with the pad's two sides (corner points): centres 0.3875 either side, 0.25 wide: a gap of 0.525 between them
    b.tracks.push(trk((4.6125, 5.0), (4.6125, 14.0), 0.25));
    b.tracks.push(trk((5.3875, 5.0), (5.3875, 14.0), 0.25));
    b.finish.on = true;
    let f = finished(&b).unwrap();
    let at_pad: Vec<&Patch> = f.patches.iter().filter(|p| p.outline.iter().any(|q| q.y < 8.0)).collect();
    assert_eq!(at_pad.len(), 2, "one for each track");
    let inside = |q: Point| at_pad.iter().any(|p| crate::geom::inside_polygon(&p.outline, q));
    // the gap is 0.525: each inner fillet is 0.2625 and together they leave a half circle free (the half towards the
    // pad), its centre on the axis between the tracks 0.2625 beyond the pad's edge (y = 5.7)
    let (cx, cy, r) = (5.0, 5.7 + 0.2625, 0.2625);
    assert!(!inside(Point::new(cx, cy - 0.1 * r)), "the middle of the arch is free");
    assert!(!inside(Point::new(cx, 5.7 + 0.02)), "right at the pad's edge in the middle, the top of the arch, is free");
    assert!(inside(Point::new(cx - 0.9 * r, 5.7 + 0.02)), "near the track's edge, just beyond the pad, is copper");
    assert!(!inside(Point::new(cx + 0.4 * r, cy - 0.3 * r)), "the arch is hollow all the way to the circle");
    // and the outer sides have their own (smaller) fillets: at the pad's corner they end flush
    let xs = at_pad.iter().flat_map(|p| p.outline.iter()).map(|q| q.x);
    let (lo, hi) = xs.fold((f64::MAX, f64::MIN), |(l, h), x| (l.min(x), h.max(x)));
    assert!(lo >= 5.0 - 0.5125 - 1e-6 && hi <= 5.0 + 0.5125 + 1e-6, "{lo} {hi}");
}

#[test]
fn a_thin_track_ending_at_the_edge_of_a_wide_one_gets_the_notches_filled_like_any_t() {
    let mut b = Board::new("t", 40.0, 30.0);
    // a wide track (0.5) in one piece, and a thin one (0.25) that comes in at 45 degrees and ends in its copper, off its
    // centre line (0.25 from it, at its edge): the two touch, but there is no joint at the end of the thin one
    b.tracks.push(trk((5.0, 10.0), (25.0, 10.0), 0.5));
    b.tracks.push(trk((4.0, 14.0), (8.0, 14.0), 0.25));
    b.tracks.push(trk((8.0, 14.0), (11.75, 10.25), 0.25));
    b.finish.on = true;
    let f = finished(&b).unwrap();
    // a fillet in the notch on each side of the branch: one polygon each, near the joint (at about x = 11.5 .. 12)
    let near: Vec<&Patch> = f.patches.iter().filter(|p| p.outline.iter().any(|q| (q.x - 11.75).abs() < 1.0 && (q.y - 10.0).abs() < 1.0)).collect();
    assert_eq!(near.len(), 2, "{} patches in all", f.patches.len());
    // the wide track is as it was (the corner of the branch elsewhere is rounded, which is another matter)
    assert!(f.board.tracks.iter().any(|t| t.a == Point::new(5.0, 10.0) && t.b == Point::new(25.0, 10.0) && t.width == 0.5));
    // and every piece of the fillets keeps within the copper of the two tracks' union or the notch: nothing is put on the
    // other side of the wide track
    assert!(near.iter().all(|p| p.outline.iter().all(|q| q.y >= 10.0 - 0.25 - 0.05)), "nothing below the wide track's edge");
    assert!(drc::check_final(&b).is_empty(), "{:?}", drc::check_final(&b));
}

#[test]
fn a_branch_that_does_not_touch_the_other_track_gets_no_fillet() {
    let mut b = Board::new("t", 40.0, 30.0);
    b.tracks.push(trk((5.0, 10.0), (25.0, 10.0), 0.5));
    // 0.3 mm from the wide track's edge: a gap, not a joint
    b.tracks.push(trk((8.0, 14.0), (11.45, 10.55), 0.25));
    b.finish.on = true;
    let f = finished(&b).unwrap();
    assert!(f.patches.is_empty(), "{} patches", f.patches.len());
}

#[test]
fn a_baked_width_change_goes_in_steps_too_fine_to_show() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 5.0, "A");
    pad(&mut b, 25.0, 5.0, "A");
    b.tracks.push(trk((5.0, 5.0), (15.0, 5.0), 0.5));
    b.tracks.push(trk((15.0, 5.0), (25.0, 5.0), 0.25));
    smooth_tracks(&mut b, &SmoothOptions { corners: false, ..Default::default() });
    let mut widths: Vec<f64> = b.tracks.iter().map(|t| t.width).collect();
    widths.sort_by(|a, b| a.partial_cmp(b).unwrap());
    widths.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    // from 0.25 to 0.5 in steps of 0.02 mm at most
    let biggest = widths.windows(2).map(|w| w[1] - w[0]).fold(0.0f64, f64::max);
    assert!(biggest <= 0.02 + 1e-9, "{widths:?}");
    assert!(ratsnest(&b).is_empty() && drc::check(&b).is_empty());
}

/// A wide track ending on a pad that is narrower than it would stick out of the pad with its round end: it gets a neck as wide as the pad.
#[test]
fn a_wide_track_on_a_narrow_pad_gets_a_neck_as_wide_as_the_pad() {
    let mut b = Board::new("t", 40.0, 30.0);
    // a QFN-like pad, 0.3 wide and 0.85 tall, and a 0.6 mm track coming in from the lower right
    let mut p = Pad::new(Point::new(10.0, 10.0), PadShape::Rect { w: 0.3, h: 0.85 }, None, Side::Top);
    p.net = "VS".into();
    b.pads.push(p);
    let mut far = Pad::new(Point::new(20.0, 3.0), PadShape::Rect { w: 1.5, h: 1.5 }, None, Side::Top);
    far.net = "VS".into();
    b.pads.push(far);
    b.tracks.push(trk((10.0, 10.0), (14.0, 6.0), 0.6));
    b.tracks.push(trk((14.0, 6.0), (20.0, 3.0), 0.6));
    b.finish.on = true;
    let f = finished(&b).expect("the finish is on");
    let at_pad = |t: &Track| t.a.dist(Point::new(10.0, 10.0)) < 1e-9 || t.b.dist(Point::new(10.0, 10.0)) < 1e-9;
    let first: Vec<&Track> = f.board.tracks.iter().filter(|t| at_pad(t)).collect();
    assert_eq!(first.len(), 1, "one piece starts at the pad");
    assert!((first[0].width - 0.3).abs() < 1e-9, "the neck is as wide as the pad: {:?}", first[0]);
    // no copper on the pad's end is wider than the pad within the neck's length
    let near_pad = f.board.tracks.iter().filter(|t| t.a.dist(Point::new(10.0, 10.0)) < 0.34).all(|t| t.width <= 0.3 + 1e-9);
    assert!(near_pad);
    // away from the pad the track keeps its width
    assert!(f.board.tracks.iter().any(|t| (t.width - 0.6).abs() < 1e-9));
    assert!(ratsnest(&f.board).is_empty(), "still connected");
    assert!(drc::check(&f.board).is_empty(), "{:?}", drc::check(&f.board));
    // a pad as wide as the track needs no neck
    let mut wide = Board::new("t", 40.0, 30.0);
    let mut p2 = Pad::new(Point::new(10.0, 10.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
    p2.net = "A".into();
    wide.pads.push(p2);
    let mut q2 = Pad::new(Point::new(20.0, 3.0), PadShape::Rect { w: 1.5, h: 1.5 }, None, Side::Top);
    q2.net = "A".into();
    wide.pads.push(q2);
    wide.tracks.push(trk((10.0, 10.0), (20.0, 3.0), 0.6));
    wide.finish.on = true;
    let g = finished(&wide).unwrap();
    assert!(g.board.tracks.iter().all(|t| t.width >= 0.6 - 1e-9 || t.width < 0.0), "{:?}", g.board.tracks.iter().map(|t| t.width).collect::<Vec<_>>());
}

/// From the side (straight into a face of the pad, across its 0.3 mm width): the pad is tall enough for the track, so the track is not
/// narrowed: it is shortened until the tip of its round end just touches the far face of the pad, which cuts what stuck out.
#[test]
fn a_wide_track_entering_a_narrow_pad_from_the_side_is_cut_at_the_far_face_of_the_pad() {
    for (end, label) in [((10.0, 10.0), "ending at the centre"), ((9.9, 10.0), "ending off the centre")] {
        let mut b = Board::new("t", 40.0, 30.0);
        let mut p = Pad::new(Point::new(10.0, 10.0), PadShape::Rect { w: 0.3, h: 0.85 }, None, Side::Top);
        p.net = "VS".into();
        b.pads.push(p);
        let mut far = Pad::new(Point::new(20.0, 10.0), PadShape::Rect { w: 1.5, h: 1.5 }, None, Side::Top);
        far.net = "VS".into();
        b.pads.push(far);
        // a 0.6 mm track from the right, across the 0.3 mm width of the pad, with only a short first piece
        b.tracks.push(trk(end, (10.7, 10.0), 0.6));
        b.tracks.push(trk((10.7, 10.0), (20.0, 10.0), 0.6));
        b.finish.on = true;
        let f = finished(&b).unwrap();
        // no copper of the track lies to the left of the pad's far face (x = 9.85)
        let leftmost = f.board.tracks.iter().map(|t| t.a.x.min(t.b.x) - t.width / 2.0).fold(f64::MAX, f64::min);
        assert!(leftmost >= 9.85 - 1e-6, "{label}: the round end reaches x = {leftmost:.3}, past the pad's far face at 9.85");
        // and it still reaches the pad (its tip is on the far face), at full width
        assert!(leftmost <= 9.85 + 1e-6, "{label}: the track is not cut back more than needed: {leftmost:.3}");
        assert!(f.board.tracks.iter().all(|t| (t.width - 0.6).abs() < 1e-9), "{label}: the width is not changed");
        // the fillets (the teardrop curves) at the face where the track meets the pad are kept
        assert!(!f.patches.is_empty(), "{label}: the teardrop at the pad is still there");
        assert!(drc::check(&f.board).is_empty(), "{label}: {:?}", drc::check(&f.board));
        assert!(ratsnest(&f.board).is_empty(), "{label}: still connected to the pad");
    }
}
