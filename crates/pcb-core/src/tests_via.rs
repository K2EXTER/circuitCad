use crate::*;

fn pad(b: &mut Board, x: f64, y: f64, net: &str) {
    let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
    p.net = net.into();
    b.pads.push(p);
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
    vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)]
}

fn via(x: f64, y: f64, net: &str) -> Via {
    Via { pos: Point::new(x, y), diameter: 0.6, drill: 0.3, net: net.into() }
}

/// A via placed for a net in empty board is something to connect to: an airwire joins it to the net's pad, until a track does.
#[test]
fn a_via_of_a_net_in_empty_board_gets_an_airwire() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 10.0, "VIN");
    b.vias.push(via(20.0, 10.0, "VIN"));
    let rats = ratsnest::ratsnest(&b);
    assert_eq!(rats.len(), 1, "{rats:?}");
    assert_eq!(rats[0].net, "VIN");
    // a track from the pad to the via joins them
    b.tracks.push(Track { side: Side::Top, a: Point::new(5.0, 10.0), b: Point::new(20.0, 10.0), width: 0.25 });
    assert!(ratsnest::ratsnest(&b).is_empty());
    // a via with no net is nobody's: no airwire
    let mut c = Board::new("t", 40.0, 30.0);
    pad(&mut c, 5.0, 10.0, "VIN");
    c.vias.push(via(20.0, 10.0, ""));
    assert!(ratsnest::ratsnest(&c).is_empty());
}

/// A via of the net of a pour, inside it, is part of the pour: the pour is not cut round it, and the connection counts.
#[test]
fn a_via_of_the_net_of_a_pour_joins_the_pour() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 5.0, 5.0, "GND");
    b.zones.push(Zone::new("GND", Side::Top, rect(2.0, 2.0, 38.0, 28.0)));
    b.vias.push(via(20.0, 15.0, "GND"));
    b.vias.push(via(30.0, 15.0, "SIG"));
    let conn = drc::connectivity(&b);
    assert_eq!(conn.via_net(0), Some("GND"));
    assert_eq!(conn.via_group(0), conn.pad_group(0), "one piece of copper through the pour");
    assert_ne!(conn.via_group(1), conn.pad_group(0), "a via of another net is cut out of the pour");
    // the pour has a hole round the SIG via and none round the GND one
    let fills = zonefill::merged_fills(&b, &conn);
    let (_, _, shapes) = fills.iter().find(|(_, n, _)| n == "GND").unwrap();
    let holes: usize = shapes.iter().map(|s| s.holes.len()).sum();
    assert_eq!(holes, 1, "only the SIG via is cleared");
    assert!(ratsnest::ratsnest(&b).iter().all(|w| w.net != "GND"), "the GND via is already connected");
}

/// A via of one net that sits on copper of another is a short.
#[test]
fn a_via_of_a_net_on_a_pad_of_another_net_is_a_short() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 10.0, 10.0, "A");
    b.vias.push(via(10.0, 10.0, "B"));
    assert!(drc::shorts(&b).iter().any(|v| v.message.contains("A") && v.message.contains("B")), "{:?}", drc::shorts(&b));
    b.vias[0].net = "A".into();
    assert!(drc::shorts(&b).is_empty());
}

/// The net of a via is written only when it has one, and old files (no `net`) load.
#[test]
fn the_net_of_a_via_is_saved_only_when_set() {
    let mut b = Board::new("t", 10.0, 10.0);
    b.vias.push(via(5.0, 5.0, ""));
    let json = serde_json::to_string(&b).unwrap();
    assert!(!json.contains("\"net\""), "{json}");
    b.vias[0].net = "GND".into();
    let back: Board = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
    assert_eq!(back.vias[0].net, "GND");
}

fn ring(b: &mut Board, c: Point, r: f64) {
    // a closed ring of tracks of net VIN round `c`, its corners on VIN pads
    let corners = [Point::new(c.x - r, c.y - r), Point::new(c.x + r, c.y - r), Point::new(c.x + r, c.y + r), Point::new(c.x - r, c.y + r)];
    for p in corners {
        pad(b, p.x, p.y, "VIN");
    }
    for i in 0..4 {
        b.tracks.push(Track { side: Side::Top, a: corners[i], b: corners[(i + 1) % 4], width: 0.3 });
    }
}

/// A pour cut apart by a closed ring of another net: the pad inside the ring is in the outline of the pour, but its copper is not joined
/// to the rest. The DRC says so, and there is an airwire for it.
#[test]
fn a_pour_cut_in_pieces_does_not_join_what_is_in_the_pieces() {
    let mut b = Board::new("t", 40.0, 30.0);
    pad(&mut b, 30.0, 20.0, "GND");
    pad(&mut b, 5.0, 5.0, "GND");
    pad(&mut b, 12.0, 20.0, "GND");
    b.zones.push(Zone::new("GND", Side::Top, rect(2.0, 2.0, 38.0, 28.0)));
    assert!(drc::split_pour_checks(&b).is_empty(), "one piece of pour joins all three");
    assert!(drc::pour_split_airwires(&b).is_empty());
    // a ring of VIN round the pad at (5, 5)
    ring(&mut b, Point::new(5.0, 5.0), 2.5);
    let found = drc::split_pour_checks(&b);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].message.contains("GND") && found[0].message.contains("分断"), "{}", found[0].message);
    let wires = drc::pour_split_airwires(&b);
    assert_eq!(wires.len(), 1);
    assert_eq!(wires[0].net, "GND");
    assert!(wires[0].a.dist(Point::new(5.0, 5.0)) < 1e-9, "the airwire starts at the pad that was cut off: {:?}", wires[0]);
}
