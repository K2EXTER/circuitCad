use crate::autoroute::*;
use crate::*;

fn pad(b: &mut Board, x: f64, y: f64, net: &str) {
    let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
    p.net = net.into();
    b.pads.push(p);
}

/// Two pads of net A, 30 mm apart, with a wall of another net across the way that has a gap at the top.
fn wall_board() -> Board {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 15.0, "A");
    pad(&mut b, 35.0, 15.0, "A");
    for k in 0..13 {
        pad(&mut b, 20.0, 3.0 + k as f64, "W");
    }
    b
}

const BUDGET: std::time::Duration = std::time::Duration::from_millis(2000);

#[test]
fn a_path_goes_round_an_obstacle_and_keeps_the_clearance() {
    let b = wall_board();
    let r = Router::new(&b, &RouteOptions::default()).unwrap();
    let (a, z) = (Point::new(5.0, 15.0), Point::new(35.0, 15.0));
    let res = r.path(Some("A"), a, z, Side::Top, false, 6.0, BUDGET).expect("a way round the wall");
    assert!(res.vias.is_empty());
    assert_eq!(res.tracks[0].a, a);
    assert_eq!(res.tracks.last().unwrap().b, z);
    // the tracks are joined end to end
    assert!(res.tracks.windows(2).all(|w| w[0].b.dist(w[1].a) < 1e-9));
    let mut t = b.clone();
    t.tracks.extend(res.tracks.clone());
    assert!(drc::check(&t).len() <= drc::check(&b).len(), "{:?}", drc::check(&t));
    // it had to go round the end of the wall
    assert!(res.tracks.iter().any(|k| k.a.y > 15.7), "above the last wall pad (top edge 15.5 + clearance)");
}

#[test]
fn a_path_with_vias_may_change_layer_and_without_them_it_may_not() {
    let mut b = wall_board();
    // close the gap: the wall now runs from edge to edge, on the top layer only
    for y in [16.0, 17.0, 18.0, 19.0, 20.0, 21.0, 22.0, 23.0, 24.0, 25.0, 26.0, 27.0, 28.0, 29.0, 1.0, 2.0] {
        pad(&mut b, 20.0, y, "W");
    }
    let r = Router::new(&b, &RouteOptions::default()).unwrap();
    let (a, z) = (Point::new(5.0, 15.0), Point::new(35.0, 15.0));
    assert!(r.path(Some("A"), a, z, Side::Top, false, 6.0, BUDGET).is_none(), "the wall blocks the top layer");
    let res = r.path(Some("A"), a, z, Side::Top, true, 6.0, BUDGET).expect("under the wall by the bottom layer");
    assert!(!res.vias.is_empty());
    assert!(res.tracks.iter().any(|t| t.side == Side::Bottom));
}

#[test]
fn a_search_with_no_room_or_from_blocked_copper_gives_up() {
    let b = wall_board();
    let r = Router::new(&b, &RouteOptions::default()).unwrap();
    let (a, z) = (Point::new(5.0, 15.0), Point::new(35.0, 15.0));
    // the box around the two points is too small to go round the wall
    assert!(r.path(Some("A"), a, z, Side::Top, false, 0.5, BUDGET).is_none());
    // a start on the wall's copper (net W, not A)
    assert!(r.path(Some("A"), Point::new(20.0, 8.0), z, Side::Top, false, 6.0, BUDGET).is_none());
}

/// How long the live route takes on a large board (run with `--ignored --nocapture`, in release).
#[test]
#[ignore]
fn timing_on_a_large_board() {
    let mut b = Board::new("big", 100.0, 80.0);
    let mut seed = 12345u64;
    let mut rnd = |n: f64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as f64 / (1u64 << 31) as f64) * n
    };
    pad(&mut b, 5.0, 40.0, "A");
    pad(&mut b, 95.0, 40.0, "A");
    for k in 0..600 {
        let (x, y) = (rnd(90.0) + 5.0, rnd(70.0) + 5.0);
        if (y - 40.0).abs() < 1.0 {
            continue;
        }
        pad(&mut b, x, y, &format!("N{}", k % 40));
    }
    let t = std::time::Instant::now();
    let r = Router::new(&b, &RouteOptions::default()).unwrap();
    println!("grid build   {:?}", t.elapsed());
    for (label, from, to) in [("near (10 mm)", Point::new(5.0, 40.0), Point::new(15.0, 40.0)), ("far (90 mm)", Point::new(5.0, 40.0), Point::new(95.0, 40.0))] {
        let t = std::time::Instant::now();
        let res = r.path(Some("A"), from, to, Side::Top, false, 3.0 + 0.3 * from.dist(to), std::time::Duration::from_secs(5));
        println!("path {label:<14} {:?}  found: {}", t.elapsed(), res.is_some());
    }
}

#[test]
fn the_autorouter_leaves_and_enters_rectangular_pads_straight_out_of_an_edge_when_asked() {
    let build = || {
        let mut b = Board::new("t", 40.0, 30.0);
        pad(&mut b, 5.0, 5.0, "A");
        pad(&mut b, 30.0, 22.0, "A");
        b
    };
    let axis_aligned = |t: &Track| (t.a.x - t.b.x).abs() < 1e-9 || (t.a.y - t.b.y).abs() < 1e-9;
    let mut b = build();
    let opt = RouteOptions { pad_lead: Some(0.5), ..Default::default() };
    let r = autoroute(&mut b, &opt, None);
    assert_eq!(r.routed, 1);
    assert!(crate::ratsnest::ratsnest(&b).is_empty());
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    for p in [Point::new(5.0, 5.0), Point::new(30.0, 22.0)] {
        // the track on the pad runs straight out of its edge: the pad's half size 0.5 plus the lead 0.5
        let t = b.tracks.iter().find(|t| t.a == p || t.b == p).unwrap();
        assert!(axis_aligned(t) && (t.a.dist(t.b) - 1.0).abs() < 1e-9, "{t:?}");
    }
    // without it, the first piece is the router's own: usually a short slanted link
    let mut c = build();
    autoroute(&mut c, &RouteOptions::default(), None);
    assert!(c.tracks.iter().any(|t| (t.a == Point::new(5.0, 5.0) || t.b == Point::new(5.0, 5.0)) && !axis_aligned(t) || t.a.dist(t.b) < 0.95));
}

#[test]
fn a_lead_that_does_not_fit_falls_back_to_any_cell_next_to_the_pad() {
    // two pads 1.5 mm apart: no room for two leads of 0.5 mm plus the pads' half sizes between them, but the route is still found
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 5.0, "A");
    pad(&mut b, 6.5, 5.0, "A");
    let opt = RouteOptions { pad_lead: Some(0.5), ..Default::default() };
    assert_eq!(autoroute(&mut b, &opt, None).routed, 1);
    assert!(crate::ratsnest::ratsnest(&b).is_empty());
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
}
