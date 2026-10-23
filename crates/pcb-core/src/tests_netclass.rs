use crate::autoroute::*;
use crate::*;

fn pad(b: &mut Board, x: f64, y: f64, net: &str) {
    let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
    p.net = net.into();
    b.pads.push(p);
}

fn class(name: &str, nets: &[&str], width: f64, clearance: f64) -> NetClass {
    NetClass { name: name.into(), nets: nets.iter().map(|n| n.to_string()).collect(), track_width: width, clearance, ..Default::default() }
}

/// The first class that lists a net is the one that counts; a net in no class has none.
#[test]
fn a_net_is_looked_up_in_the_classes() {
    let mut b = Board::new("t", 30.0, 20.0);
    b.net_classes.push(class("電源", &["VIN", "GND"], 0.6, 0.3));
    b.net_classes.push(class("別", &["VIN", "SIG"], 0.2, 0.2));
    assert_eq!(b.class_of("VIN").unwrap().name, "電源");
    assert_eq!(b.class_of("SIG").unwrap().name, "別");
    assert!(b.class_of("X").is_none() && b.class_of("").is_none());
    // the clearance between two nets is the board's rule, or the more of what either class asks for
    assert_eq!(b.clearance_between(Some("X"), Some("Y")), b.rules.min_clearance);
    assert_eq!(b.clearance_between(Some("X"), Some("VIN")), 0.3);
    assert_eq!(b.clearance_between(Some("SIG"), Some("VIN")), 0.3);
    assert_eq!(b.max_clearance(), 0.3);
}

/// An old board file has no classes and still loads; a board with none writes no `net_classes`.
#[test]
fn boards_without_classes_load_and_save_as_before() {
    let b = Board::new("t", 30.0, 20.0);
    let json = serde_json::to_string(&b).unwrap();
    assert!(!json.contains("net_classes"), "nothing is written for an empty list");
    let back: Board = serde_json::from_str(&json).unwrap();
    assert!(back.net_classes.is_empty());
    let mut c = b.clone();
    c.net_classes.push(class("電源", &["VIN"], 0.6, 0.3));
    let back: Board = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
    assert_eq!(back.net_classes, c.net_classes);
}

/// The DRC asks for the clearance of the class: two tracks 0.2 mm apart are fine for the board, too close for a class that wants 0.3 mm.
#[test]
fn the_drc_keeps_the_clearance_of_a_net_class() {
    let mut b = Board::new("t", 30.0, 20.0);
    pad(&mut b, 6.0, 10.0, "VIN");
    pad(&mut b, 24.0, 10.45, "SIG");
    b.tracks.push(Track { side: Side::Top, a: Point::new(6.0, 10.0), b: Point::new(20.0, 10.0), width: 0.25 });
    // 0.45 mm centre to centre: a gap of 0.2 mm, between VIN and a track that ends on the pad of SIG
    b.tracks.push(Track { side: Side::Top, a: Point::new(10.0, 10.45), b: Point::new(24.0, 10.45), width: 0.25 });
    let plain = drc::clearance(&b).len();
    b.net_classes.push(class("電源", &["VIN"], 0.25, 0.3));
    let classed = drc::clearance(&b);
    assert!(classed.len() > plain, "{plain} -> {}", classed.len());
    assert!(classed.iter().any(|v| v.message.contains("ネットクラス")), "{:?}", classed);
}

/// A net of a class is routed with the width of the class, and the others with the width asked for.
#[test]
fn a_classed_net_is_routed_with_its_class_width() {
    let mut b = Board::new("t", 40.0, 20.0);
    pad(&mut b, 5.0, 5.0, "VIN");
    pad(&mut b, 35.0, 5.0, "VIN");
    pad(&mut b, 5.0, 15.0, "SIG");
    pad(&mut b, 35.0, 15.0, "SIG");
    b.net_classes.push(class("電源", &["VIN"], 0.6, 0.3));
    let r = autoroute_sized(&mut b, &RouteOptions { grid: 0.25, ..Default::default() }, None);
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    let conn = drc::connectivity(&b);
    let widths = |net: &str| b.tracks.iter().enumerate().filter(|(i, _)| conn.track_net(*i) == Some(net)).map(|(_, t)| t.width).collect::<Vec<_>>();
    assert!(!widths("VIN").is_empty() && widths("VIN").iter().all(|w| (*w - 0.6).abs() < 1e-9), "{:?}", widths("VIN"));
    assert!(widths("SIG").iter().all(|w| (*w - 0.25).abs() < 1e-9), "{:?}", widths("SIG"));
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
}

/// The clearance of a class is kept by the router: a track next to a classed net stays that far from it.
#[test]
fn the_router_keeps_the_clearance_of_a_net_class() {
    let mut b = Board::new("t", 40.0, 20.0);
    pad(&mut b, 5.0, 10.0, "SIG");
    pad(&mut b, 35.0, 10.0, "SIG");
    // a long wall of VIN copper that the signal has to go around, with a clearance of 1 mm asked for
    b.tracks.push(Track { side: Side::Top, a: Point::new(20.0, 0.8), b: Point::new(20.0, 19.2), width: 0.4 });
    b.tracks.push(Track { side: Side::Bottom, a: Point::new(20.0, 0.8), b: Point::new(20.0, 19.2), width: 0.4 });
    pad(&mut b, 20.0, 0.8, "VIN");
    b.net_classes.push(class("電源", &["VIN"], 0.4, 1.0));
    let _ = autoroute_sized(&mut b, &RouteOptions { grid: 0.25, ..Default::default() }, None);
    // whatever was found (or not), the DRC with the class clearance finds nothing wrong
    assert!(drc::clearance(&b).is_empty(), "{:?}", drc::clearance(&b));
}
