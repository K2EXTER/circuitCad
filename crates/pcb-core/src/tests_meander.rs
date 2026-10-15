use crate::meander::*;
use crate::ratsnest::ratsnest;
use crate::*;

fn pad(b: &mut Board, x: f64, y: f64, net: &str) {
    let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 0.8, h: 0.8 }, None, Side::Top);
    p.net = net.into();
    b.pads.push(p);
}

fn line(b: &mut Board, net: &str, y: f64, x0: f64, x1: f64) {
    pad(b, x0, y, net);
    pad(b, x1, y, net);
    b.tracks.push(Track { side: Side::Top, a: Point::new(x0, y), b: Point::new(x1, y), width: 0.25 });
}

fn opts(style: MeanderStyle) -> MeanderOptions {
    MeanderOptions { style, ..Default::default() }
}

#[test]
fn a_serpentine_adds_the_length_asked_for_in_every_style() {
    for style in [MeanderStyle::Square, MeanderStyle::Chamfer, MeanderStyle::Round] {
        let mut b = Board::new("t", 60.0, 30.0);
        line(&mut b, "A", 15.0, 5.0, 45.0);
        let before = length_of(&b, "A");
        let added = tune_net(&mut b, "A", 6.0, &opts(style)).unwrap_or_else(|e| panic!("{style:?}: {e}"));
        assert!((added - 6.0).abs() < 0.12, "{style:?}: {added}");
        assert!((length_of(&b, "A") - before - added).abs() < 1e-9);
        assert!(ratsnest(&b).is_empty(), "{style:?}: still connected");
        assert!(drc::check(&b).is_empty(), "{style:?}: {:?}", drc::check(&b));
        // the ends stay on the pads
        assert!(b.tracks.iter().any(|t| t.a == Point::new(5.0, 15.0)) && b.tracks.iter().any(|t| t.b == Point::new(45.0, 15.0)));
    }
}

#[test]
fn chamfered_and_round_corners_have_no_right_angle() {
    let turn_deg = |p: Point, q: Point, r: Point| {
        let (u, v) = ((q.x - p.x, q.y - p.y), (r.x - q.x, r.y - q.y));
        let c = (u.0 * v.0 + u.1 * v.1) / (u.0.hypot(u.1) * v.0.hypot(v.1));
        c.clamp(-1.0, 1.0).acos().to_degrees()
    };
    for (style, worst) in [(MeanderStyle::Square, 89.0), (MeanderStyle::Chamfer, 50.0), (MeanderStyle::Round, 30.0)] {
        let mut b = Board::new("t", 60.0, 30.0);
        line(&mut b, "A", 15.0, 5.0, 45.0);
        tune_net(&mut b, "A", 6.0, &opts(style)).unwrap();
        // the tracks in order along the line
        let mut ts = b.tracks.clone();
        ts.sort_by(|x, y| x.a.x.min(x.b.x).total_cmp(&y.a.x.min(y.b.x)).then(x.a.y.total_cmp(&y.a.y)));
        let max_turn = ts.windows(2).filter(|w| w[0].b.dist(w[1].a) < 1e-9).map(|w| turn_deg(w[0].a, w[0].b, w[1].b)).fold(0.0, f64::max);
        match style {
            MeanderStyle::Square => assert!(max_turn > worst, "{max_turn}"),
            _ => assert!(max_turn < worst, "{style:?}: the sharpest turn is {max_turn} degrees"),
        }
    }
}

#[test]
fn matching_brings_the_shorter_nets_to_the_longest() {
    let mut b = Board::new("t", 80.0, 60.0);
    line(&mut b, "A", 10.0, 5.0, 65.0); // 60 mm
    line(&mut b, "B", 25.0, 5.0, 50.0); // 45 mm
    line(&mut b, "C", 40.0, 5.0, 55.0); // 50 mm
    let nets: Vec<String> = ["A", "B", "C"].iter().map(|s| s.to_string()).collect();
    let r = match_nets(&mut b, &nets, None, &opts(MeanderStyle::Round));
    assert!((r.target - 60.0).abs() < 1e-9);
    assert!(r.nets.iter().all(|(_, _, res)| res.is_ok()), "{:?}", r.nets);
    for n in &nets {
        assert!((length_of(&b, n) - 60.0).abs() < 0.2, "{n}: {}", length_of(&b, n));
    }
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    assert!(ratsnest(&b).is_empty());
    // a target beyond the longest net: all of them, the longest too, go up to it
    let r = match_nets(&mut b, &nets, Some(64.0), &opts(MeanderStyle::Square));
    assert!((r.target - 64.0).abs() < 1e-9 && r.nets.iter().all(|(_, _, res)| res.is_ok()), "{:?}", r.nets);
}

#[test]
fn a_run_that_is_too_short_or_blocked_is_left_alone() {
    // a 3 mm run has no room for a serpentine
    let mut b = Board::new("t", 60.0, 30.0);
    line(&mut b, "A", 15.0, 5.0, 8.0);
    let before = b.tracks.len();
    assert!(tune_net(&mut b, "A", 4.0, &opts(MeanderStyle::Square)).is_err());
    assert_eq!(b.tracks.len(), before);
    // no tracks at all
    let mut c = Board::new("t", 60.0, 30.0);
    pad(&mut c, 5.0, 5.0, "N");
    assert!(tune_net(&mut c, "N", 4.0, &opts(MeanderStyle::Square)).unwrap_err().contains("配線"));
    // a net that is already long enough needs nothing
    assert!(tune_net(&mut b, "A", 0.0, &opts(MeanderStyle::Square)).is_err());
}

#[test]
fn another_nets_track_beside_the_run_makes_the_serpentine_go_to_the_free_side() {
    let mut b = Board::new("t", 60.0, 30.0);
    line(&mut b, "A", 15.0, 5.0, 45.0);
    // a track of another net 1 mm above the line (on the +y side): clear of it, but with no room for bumps
    line(&mut b, "W", 16.0, 5.0, 45.0);
    let before_drc = drc::check(&b).len();
    let added = tune_net(&mut b, "A", 5.0, &opts(MeanderStyle::Square)).unwrap();
    assert!((added - 5.0).abs() < 0.1);
    assert_eq!(drc::check(&b).len(), before_drc);
    // the bumps are below the line (y < 15)
    let conn = drc::connectivity(&b);
    assert!(b.tracks.iter().enumerate().filter(|(i, _)| conn.track_net(*i) == Some("A")).all(|(_, t)| t.a.y <= 15.0 + 1e-9 && t.b.y <= 15.0 + 1e-9));
}
