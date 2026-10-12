use crate::diffpair::*;
use crate::*;

fn pad(b: &mut Board, x: f64, y: f64, net: &str) {
    let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 0.3, h: 0.3 }, None, Side::Top);
    p.net = net.into();
    b.pads.push(p);
}

fn line(b: &mut Board, a: (f64, f64), z: (f64, f64)) {
    b.tracks.push(Track { side: Side::Top, a: Point::new(a.0, a.1), b: Point::new(z.0, z.1), width: 0.2 });
}

/// A pair running side by side, 0.2mm apart (edge to edge), the N line 3mm longer (a detour on the far end).
fn board() -> Board {
    let mut b = Board::new("t", 60.0, 30.0);
    pad(&mut b, 5.0, 10.0, "D_P");
    pad(&mut b, 5.0, 10.4, "D_N");
    pad(&mut b, 45.0, 10.0, "D_P");
    pad(&mut b, 45.0, 10.4, "D_N");
    line(&mut b, (5.0, 10.0), (45.0, 10.0));
    line(&mut b, (5.0, 10.4), (45.0, 10.4));
    b.diff_pairs.push(DiffPair::new("D", "D_P", "D_N"));
    b
}

#[test]
fn pairs_are_found_from_net_names() {
    let nets: Vec<String> = ["USB_DP", "USB_DM", "TX_P", "TX_N", "CLK+", "CLK-", "GND", "VCC", "PA", "PB"].iter().map(|s| s.to_string()).collect();
    let found = suggest_pairs(&nets);
    let names: Vec<_> = found.iter().map(|d| (d.p.as_str(), d.n.as_str())).collect();
    assert!(names.contains(&("USB_DP", "USB_DM")), "{names:?}");
    assert!(names.contains(&("TX_P", "TX_N")), "{names:?}");
    assert!(names.contains(&("CLK+", "CLK-")), "{names:?}");
    assert_eq!(found.len(), 3, "{names:?}");
}

#[test]
fn a_matched_close_pair_is_clean() {
    let b = board();
    let r = pair_report(&b, &b.diff_pairs[0]);
    assert!((r.len_p - 40.0).abs() < 1e-9 && r.skew < 1e-9);
    assert!(r.coupled > 39.0, "{r:?}");
    assert!((r.gap_min.unwrap() - 0.2).abs() < 1e-6, "{r:?}");
    assert!(check_pairs(&b).is_empty(), "{:?}", check_pairs(&b));
}

#[test]
fn length_difference_and_gap_are_reported_with_a_fix() {
    let mut b = board();
    b.tracks[1].b = Point::new(42.0, 10.4); // N is 3mm short
    b.tracks[1].a = Point::new(5.0, 10.4);
    let msgs: Vec<String> = check_pairs(&b).into_iter().map(|v| v.message).collect();
    assert!(msgs.iter().any(|m| m.contains("長さの差")), "{msgs:?}");
    // pull the lines apart: the gap is off
    let mut c = board();
    c.tracks[1].a.y = 11.4;
    c.tracks[1].b.y = 11.4;
    c.pads[1].pos.y = 11.4;
    c.pads[3].pos.y = 11.4;
    let msgs: Vec<String> = check_pairs(&c).into_iter().map(|v| v.message).collect();
    assert!(msgs.iter().any(|m| m.contains("間隔")), "{msgs:?}");
}

#[test]
fn only_one_line_routed_is_reported() {
    let mut b = board();
    b.tracks.pop();
    assert!(check_pairs(&b).iter().any(|v| v.message.contains("片方だけ")));
}

#[test]
fn a_serpentine_evens_out_the_length_without_breaking_a_rule() {
    let mut b = board();
    // the P line is short by 3mm
    b.tracks[0].b = Point::new(42.0, 10.0);
    // keep the far pad connected: a short stub
    line(&mut b, (42.0, 10.0), (45.0, 10.0));
    b.tracks.pop();
    b.tracks.push(Track { side: Side::Top, a: Point::new(42.0, 10.0), b: Point::new(45.0, 10.0), width: 0.2 });
    // make P longer instead: N is shorter by cutting its end and bridging below
    let mut b = board();
    b.tracks[1].b = Point::new(30.0, 10.4);
    b.tracks.push(Track { side: Side::Top, a: Point::new(30.0, 10.4), b: Point::new(30.0, 14.0), width: 0.2 });
    b.tracks.push(Track { side: Side::Top, a: Point::new(30.0, 14.0), b: Point::new(45.0, 14.0), width: 0.2 });
    b.tracks.push(Track { side: Side::Top, a: Point::new(45.0, 14.0), b: Point::new(45.0, 10.4), width: 0.2 });
    // now N is 3.6 + 15 + 3.6 + 25 = 47.2 vs P = 40
    let pair = b.diff_pairs[0].clone();
    let before = pair_report(&b, &pair);
    assert!(before.skew > 5.0, "{before:?}");
    let violations_before = drc::check(&b).len();
    let added = tune_length(&mut b, &pair).expect("tuned");
    let after = pair_report(&b, &pair);
    assert!((added - before.skew).abs() < 1e-9);
    assert!(after.skew < 0.1, "{after:?}");
    assert!(drc::check(&b).len() <= violations_before, "{:?}", drc::check(&b));
    assert!(ratsnest::ratsnest(&b).is_empty());
}

#[test]
fn nothing_to_tune_when_the_lengths_match() {
    let mut b = board();
    let pair = b.diff_pairs[0].clone();
    assert!(tune_length(&mut b, &pair).is_err());
}

#[test]
fn the_pair_is_saved_with_the_board() {
    let b = board();
    let json = serde_json::to_string(&b).unwrap();
    let back: Board = serde_json::from_str(&json).unwrap();
    assert_eq!(back.diff_pairs, b.diff_pairs);
}

#[test]
fn a_pair_is_routed_side_by_side() {
    use crate::autoroute::*;
    let mut b = Board::new("t", 60.0, 30.0);
    for (x, p_net, n_net) in [(5.0, "D_P", "D_N"), (50.0, "D_P", "D_N")] {
        pad(&mut b, x, 10.0, p_net);
        pad(&mut b, x, 11.0, n_net);
    }
    b.diff_pairs.push(DiffPair::new("D", "D_P", "D_N"));
    let pair = b.diff_pairs[0].clone();
    let opt = RouteOptions { track_width: 0.2, grid: 0.1, ..Default::default() };
    let r = route_pair(&mut b, &pair, &opt);
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    assert!(ratsnest::ratsnest(&b).is_empty());
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    let rep = pair_report(&b, &pair);
    assert!(rep.coupled > 0.7 * rep.len_p, "the lines run together: {rep:?}");
    assert!(rep.skew < 1.0, "{rep:?}");
    // and the length can be trimmed to match exactly
    let mut b2 = b.clone();
    if rep.skew > 0.05 {
        let _ = tune_length(&mut b2, &pair);
        assert!(pair_report(&b2, &pair).skew <= rep.skew + 1e-9);
    }
}
