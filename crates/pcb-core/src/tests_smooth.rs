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
    pad(&mut b, 5.0, 5.0, "A");
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
fn the_notch_between_two_tracks_at_a_sharp_angle_is_filled_and_rounded() {
    let mut b = fork_board();
    let before = b.tracks.clone();
    let r = smooth_tracks(&mut b, &SmoothOptions { corners: false, tapers: false, ..Default::default() });
    assert_eq!(r.forks, 1);
    assert!(b.tracks[..2].iter().zip(&before).all(|(x, y)| x.a == y.a && x.b == y.b), "the routes are not changed");
    let fill = &b.tracks[2..];
    assert!(fill.len() >= 3);
    // thin tracks side by side along the line between the two tracks (y = 15), none thinner than the rules allow
    assert!(fill.iter().all(|t| (t.a.y - t.b.y).abs() < 1e-9));
    assert!(fill.iter().all(|t| t.width >= b.rules.min_track_width - 1e-9));
    // the fill stops short of a free circle (radius = track width) that touches both tracks' edges: none of it
    // reaches into that circle, so the gap ends in a round, concave end and not in a bump
    let theta = 15f64.to_radians();
    let centre = Point::new(5.0 + (0.25 + 0.125) / theta.sin(), 15.0);
    let into_circle = fill.iter().any(|t| {
        let (dx, dy) = (t.b.x - t.a.x, t.b.y - t.a.y);
        let u = (((centre.x - t.a.x) * dx + (centre.y - t.a.y) * dy) / (dx * dx + dy * dy).max(1e-12)).clamp(0.0, 1.0);
        Point::new(t.a.x + dx * u, t.a.y + dy * u).dist(centre) < 0.25 - t.width / 2.0 - 0.01
    });
    assert!(!into_circle);
    // and the fill does reach close to it, on the middle line
    let nearest = fill.iter().map(|t| t.a.x.max(t.b.x)).fold(0.0, f64::max);
    assert!(nearest > centre.x - 0.25 - 0.2, "{nearest}");
    assert!(ratsnest(&b).is_empty());
    assert!(drc::check(&b).is_empty());
    // a second run adds nothing
    assert_eq!(smooth_tracks(&mut b, &SmoothOptions::default()).forks, 0);
}

#[test]
fn two_tracks_at_a_wide_angle_are_not_a_fork() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 15.0, "A");
    b.tracks.push(trk((5.0, 15.0), (15.0, 25.0), 0.25));
    b.tracks.push(trk((5.0, 15.0), (15.0, 5.0), 0.25)); // 90 degrees apart
    assert_eq!(smooth_tracks(&mut b, &SmoothOptions::default()).forks, 0);
}
