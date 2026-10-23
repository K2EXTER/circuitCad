use crate::merge::merge_collinear;
use crate::ratsnest::ratsnest;
use crate::*;

fn trk(side: Side, w: f64, a: (f64, f64), z: (f64, f64)) -> Track {
    Track { side, a: Point::new(a.0, a.1), b: Point::new(z.0, z.1), width: w }
}

fn pad(b: &mut Board, x: f64, y: f64, net: &str) {
    let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
    p.net = net.into();
    b.pads.push(p);
}

fn length(b: &Board) -> f64 {
    b.tracks.iter().map(|t| t.a.dist(t.b)).sum()
}

#[test]
fn a_chain_of_straight_pieces_becomes_one_track() {
    let mut b = Board::new("t", 40.0, 30.0);
    // three pieces, written with mixed orientation (the third one backwards)
    b.tracks.push(trk(Side::Top, 0.25, (5.0, 10.0), (8.0, 10.0)));
    b.tracks.push(trk(Side::Top, 0.25, (8.0, 10.0), (12.0, 10.0)));
    b.tracks.push(trk(Side::Top, 0.25, (20.0, 10.0), (12.0, 10.0)));
    assert_eq!(merge_collinear(&mut b, 0), 2);
    assert_eq!(b.tracks.len(), 1);
    let t = &b.tracks[0];
    let (lo, hi) = (t.a.x.min(t.b.x), t.a.x.max(t.b.x));
    assert!((lo - 5.0).abs() < 1e-9 && (hi - 20.0).abs() < 1e-9, "{t:?}");
}

#[test]
fn diagonal_pieces_merge_too() {
    let mut b = Board::new("t", 40.0, 30.0);
    b.tracks.push(trk(Side::Top, 0.25, (5.0, 5.0), (8.0, 8.0)));
    b.tracks.push(trk(Side::Top, 0.25, (8.0, 8.0), (13.0, 13.0)));
    assert_eq!(merge_collinear(&mut b, 0), 1);
    assert_eq!(b.tracks.len(), 1);
}

#[test]
fn bends_widths_sides_and_junctions_are_left_alone() {
    let mut b = Board::new("t", 40.0, 30.0);
    // a bend
    b.tracks.push(trk(Side::Top, 0.25, (5.0, 5.0), (10.0, 5.0)));
    b.tracks.push(trk(Side::Top, 0.25, (10.0, 5.0), (15.0, 10.0)));
    // other width
    b.tracks.push(trk(Side::Top, 0.25, (5.0, 15.0), (10.0, 15.0)));
    b.tracks.push(trk(Side::Top, 0.5, (10.0, 15.0), (15.0, 15.0)));
    // other side
    b.tracks.push(trk(Side::Top, 0.25, (5.0, 20.0), (10.0, 20.0)));
    b.tracks.push(trk(Side::Bottom, 0.25, (10.0, 20.0), (15.0, 20.0)));
    // a T junction: three ends at (25, 5)
    b.tracks.push(trk(Side::Top, 0.25, (20.0, 5.0), (25.0, 5.0)));
    b.tracks.push(trk(Side::Top, 0.25, (25.0, 5.0), (30.0, 5.0)));
    b.tracks.push(trk(Side::Top, 0.25, (25.0, 5.0), (25.0, 9.0)));
    // a doubling back (same direction from the joint)
    b.tracks.push(trk(Side::Top, 0.25, (20.0, 20.0), (25.0, 20.0)));
    b.tracks.push(trk(Side::Top, 0.25, (20.0, 20.0), (24.0, 20.0)));
    let n = b.tracks.len();
    assert_eq!(merge_collinear(&mut b, 0), 0);
    assert_eq!(b.tracks.len(), n);
}

#[test]
fn a_joint_on_a_pad_or_a_via_stays() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 10.0, 10.0, "A");
    b.tracks.push(trk(Side::Top, 0.25, (5.0, 10.0), (10.0, 10.0)));
    b.tracks.push(trk(Side::Top, 0.25, (10.0, 10.0), (15.0, 10.0)));
    b.vias.push(Via { pos: Point::new(10.0, 20.0), diameter: 0.6, drill: 0.3, net: String::new() });
    b.tracks.push(trk(Side::Top, 0.25, (5.0, 20.0), (10.0, 20.0)));
    b.tracks.push(trk(Side::Top, 0.25, (10.0, 20.0), (15.0, 20.0)));
    assert_eq!(merge_collinear(&mut b, 0), 0);
}

#[test]
fn only_tracks_from_the_given_index_are_touched() {
    let mut b = Board::new("t", 40.0, 30.0);
    b.tracks.push(trk(Side::Top, 0.25, (5.0, 10.0), (8.0, 10.0))); // old
    b.tracks.push(trk(Side::Top, 0.25, (8.0, 10.0), (12.0, 10.0))); // new, joined to the old one
    b.tracks.push(trk(Side::Top, 0.25, (12.0, 10.0), (15.0, 10.0))); // new
    assert_eq!(merge_collinear(&mut b, 1), 1);
    assert_eq!(b.tracks.len(), 2);
    assert_eq!(b.tracks[0].b, Point::new(8.0, 10.0), "the old track is as it was");
}

#[test]
fn routed_samples_keep_their_copper_and_lose_pieces() {
    let mut b = Board::new("t", 40.0, 30.0);
    for (x, y, n) in [(5.0, 10.0, "A"), (30.0, 22.0, "A"), (5.0, 22.0, "B"), (30.0, 10.0, "B")] {
        pad(&mut b, x, y, n);
    }
    let opt = autoroute::RouteOptions { pad_lead: Some(0.5), ..Default::default() };
    autoroute::autoroute(&mut b, &opt, None);
    let all: Vec<usize> = (0..b.tracks.len()).collect();
    padentry::align_entries(&mut b, &padentry::EntryOptions::default(), Some(&all));
    let (before_len, before_n) = (length(&b), b.tracks.len());
    let drc_before = drc::check(&b).len();
    let removed = merge_collinear(&mut b, 0);
    assert_eq!(b.tracks.len(), before_n - removed);
    assert!((length(&b) - before_len).abs() < 1e-6, "the copper is the same");
    assert!(ratsnest(&b).is_empty(), "still connected");
    assert_eq!(drc::check(&b).len(), drc_before);
    // nothing is left to merge
    assert_eq!(merge_collinear(&mut b, 0), 0);
}
