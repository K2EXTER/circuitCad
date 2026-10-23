use crate::ratsnest::ratsnest;
use crate::shove::*;
use crate::*;

fn pad(b: &mut Board, x: f64, y: f64, net: &str) {
    let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
    p.net = net.into();
    b.pads.push(p);
}

fn trk(a: (f64, f64), z: (f64, f64), w: f64) -> Track {
    Track { side: Side::Top, a: Point::new(a.0, a.1), b: Point::new(z.0, z.1), width: w }
}

/// Net B runs from a pad at the left to a pad at the right; net A's new track is laid too close to it.
fn two_nets() -> Board {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 12.0, "B");
    pad(&mut b, 35.0, 12.0, "B");
    b.tracks.push(trk((5.0, 12.0), (20.0, 12.0), 0.25));
    b.tracks.push(trk((20.0, 12.0), (35.0, 12.0), 0.25));
    b
}

#[test]
fn a_track_in_the_way_is_pushed_aside() {
    let mut b = two_nets();
    let before = b.tracks.len();
    // 0.35mm from net B's line: the gap is 0.1mm, less than the 0.15mm rule
    let n = add_tracks(&mut b, vec![trk((12.0, 11.65), (28.0, 11.65), 0.25)]);
    assert!(n >= 1, "net B was pushed");
    assert_eq!(b.tracks.len(), before + 1);
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    assert!(ratsnest(&b).is_empty(), "net B is still connected");
    // the new track stays exactly where it was drawn
    let new = b.tracks.last().unwrap();
    assert_eq!((new.a.y, new.b.y), (11.65, 11.65));
    // and it was pushed away from net A, not through it
    assert!(b.tracks[..before].iter().all(|t| t.a.y >= 11.9 && t.b.y >= 11.9));
}

#[test]
fn nothing_is_moved_when_there_is_room() {
    let mut b = two_nets();
    let n = add_tracks(&mut b, vec![trk((12.0, 8.0), (28.0, 8.0), 0.25)]);
    assert_eq!(n, 0);
    assert_eq!(b.tracks[0].b.y, 12.0);
}

#[test]
fn a_track_pinned_at_both_ends_is_bent_aside() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 12.0, "B");
    pad(&mut b, 35.0, 12.0, "B");
    b.tracks.push(trk((5.0, 12.0), (35.0, 12.0), 0.25));
    let n = add_tracks(&mut b, vec![trk((12.0, 11.65), (28.0, 11.65), 0.25)]);
    assert!(n >= 1);
    assert!(b.tracks.len() > 2, "the held track was split to bend");
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    assert!(ratsnest(&b).is_empty(), "net B is still connected");
    assert_eq!((b.tracks[0].a.x, b.tracks[0].a.y), (5.0, 12.0), "the end on the pad stays");
}

#[test]
fn a_track_too_short_to_bend_is_left_and_the_new_track_is_still_added() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 12.0, "B");
    pad(&mut b, 7.0, 12.0, "B");
    b.tracks.push(trk((5.0, 12.0), (7.0, 12.0), 0.25));
    let n = add_tracks(&mut b, vec![trk((4.0, 11.65), (8.0, 11.65), 0.25)]);
    assert_eq!(n, 0);
    assert_eq!(b.tracks.len(), 2);
    assert_eq!(b.tracks[0].a.y, 12.0);
}

#[test]
fn a_dragged_track_keeps_its_neighbours_joined() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 10.0, "A");
    pad(&mut b, 30.0, 10.0, "A");
    b.tracks.push(trk((5.0, 10.0), (12.0, 10.0), 0.25));
    b.tracks.push(trk((12.0, 10.0), (22.0, 10.0), 0.25)); // the one to drag
    b.tracks.push(trk((22.0, 10.0), (30.0, 10.0), 0.25));
    assert!(ratsnest(&b).is_empty());
    let n = move_track_shoving(&mut b, 1, 0.0, 3.0);
    assert_eq!(n, 0);
    assert_eq!((b.tracks[1].a.y, b.tracks[1].b.y), (13.0, 13.0));
    assert_eq!(b.tracks[0].b.y, 13.0, "the neighbour stretches along");
    assert!(ratsnest(&b).is_empty(), "still one connection");
}

#[test]
fn dragging_a_track_that_ends_on_a_pad_adds_a_stub() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 10.0, "A");
    pad(&mut b, 25.0, 10.0, "A");
    b.tracks.push(trk((5.0, 10.0), (25.0, 10.0), 0.25));
    move_track_shoving(&mut b, 0, 0.0, 4.0);
    assert_eq!(b.tracks.len(), 3, "two stubs from the pads");
    assert!(ratsnest(&b).is_empty());
}

#[test]
fn a_dragged_track_pushes_the_one_in_its_way() {
    let mut b = two_nets();
    b.tracks.push(trk((10.0, 8.0), (30.0, 8.0), 0.25)); // floating track of another net
    let n = move_track_shoving(&mut b, 2, 0.0, 3.65); // up to y = 11.65: closer to net B than the rule allows
    assert!(n >= 1);
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
}

#[test]
fn ripping_up_a_net_removes_its_tracks_and_vias_only() {
    let mut b = two_nets();
    pad(&mut b, 5.0, 20.0, "A");
    pad(&mut b, 35.0, 20.0, "A");
    b.tracks.push(trk((5.0, 20.0), (35.0, 20.0), 0.25));
    b.vias.push(Via { pos: Point::new(20.0, 20.0), diameter: 0.6, drill: 0.3, net: String::new() });
    assert_eq!(rip_up_net(&mut b, "A"), 2);
    assert_eq!(b.tracks.len(), 2);
    assert!(b.vias.is_empty());
    assert_eq!(ratsnest(&b).len(), 1, "the airwire is back");
}

