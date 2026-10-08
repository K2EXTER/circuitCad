use crate::ratsnest::ratsnest;
use crate::*;

fn board() -> Board {
    Board::new("t", 40.0, 30.0)
}

fn pad(b: &mut Board, x: f64, y: f64, net: &str, through_hole: bool) {
    let shape = if through_hole { PadShape::Circle { d: 1.6 } } else { PadShape::Rect { w: 1.0, h: 1.0 } };
    let mut p = Pad::new(Point::new(x, y), shape, through_hole.then_some(0.8), Side::Top);
    p.net = net.into();
    b.pads.push(p);
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
    vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)]
}

fn count(g: &str, needle: &str) -> usize {
    g.matches(needle).count()
}

#[test]
fn a_zone_connects_pads_of_its_net_and_nothing_else() {
    let mut b = board();
    pad(&mut b, 10.0, 10.0, "GND", true);
    pad(&mut b, 30.0, 20.0, "GND", true);
    pad(&mut b, 20.0, 15.0, "SIG", false);
    pad(&mut b, 22.0, 15.0, "SIG", false);
    assert_eq!(ratsnest(&b).iter().filter(|a| a.net == "GND").count(), 1);

    b.zones.push(Zone::new("GND", Side::Top, rect(5.0, 5.0, 35.0, 25.0)));
    let w = ratsnest(&b);
    assert_eq!(w.iter().filter(|a| a.net == "GND").count(), 0, "the pour joins both GND pads");
    assert_eq!(w.iter().filter(|a| a.net == "SIG").count(), 1, "SIG pads inside the pour stay separate");
    assert!(drc::shorts(&b).is_empty());
}

#[test]
fn a_zone_on_the_other_layer_does_not_connect_smd_pads() {
    let mut b = board();
    pad(&mut b, 10.0, 10.0, "GND", false);
    pad(&mut b, 30.0, 20.0, "GND", false);
    b.zones.push(Zone::new("GND", Side::Bottom, rect(5.0, 5.0, 35.0, 25.0)));
    assert_eq!(ratsnest(&b).len(), 1);
    // ... but a via inside the zone does bring it to the bottom layer
    b.vias.push(Via { pos: Point::new(10.0, 10.0), diameter: 0.6, drill: 0.3 });
    b.vias.push(Via { pos: Point::new(30.0, 20.0), diameter: 0.6, drill: 0.3 });
    assert_eq!(ratsnest(&b).len(), 0);
}

#[test]
fn zone_output_has_region_cutouts_and_thermals() {
    let mut b = board();
    pad(&mut b, 10.0, 10.0, "GND", true); // thermal relief
    pad(&mut b, 20.0, 15.0, "SIG", false); // other net: cleared
    pad(&mut b, 24.0, 15.0, "SIG", false);
    b.tracks.push(Track { side: Side::Top, a: Point::new(20.0, 15.0), b: Point::new(24.0, 15.0), width: 0.25 });
    b.holes.push(Hole { pos: Point::new(30.0, 8.0), drill: 3.0 });
    b.zones.push(Zone::new("GND", Side::Top, rect(5.0, 5.0, 35.0, 25.0)));

    let top = gerber::copper(&b, Side::Top);
    assert_eq!(count(&top, "G36*"), 1);
    assert_eq!(count(&top, "G37*"), 1);
    assert!(top.find("G36*").unwrap() < top.find("%LPC*%").unwrap());
    // the clear section ends before the normal copper is drawn dark again
    let last_dark = top.rfind("%LPD*%").unwrap();
    assert!(last_dark > top.find("%LPC*%").unwrap());
    assert!(top[last_dark..].contains("D03*"), "pads are flashed after the cut-outs");

    assert!(top.contains("C,0.850000"), "track 0.25 + 2 x 0.3 clearance");
    assert!(top.contains("R,1.600000X1.600000"), "SMD pad 1.0 + 2 x 0.3");
    assert!(top.contains("C,2.200000"), "thermal ring: pad 1.6 + 2 x 0.3 gap");
    assert!(top.contains("C,0.300000"), "thermal spokes");
    assert!(top.contains("C,0.600000"), "board edge clearance line (2 x 0.3), also the NPTH hole ring is 3.6");
    assert!(top.contains("C,3.600000"), "mounting hole 3.0 + 2 x 0.3");
    assert_eq!(count(&top, "X10000000Y10000000D03*"), 2, "thermal ring and the pad itself");

    // the bottom layer is untouched by a top zone
    let bottom = gerber::copper(&b, Side::Bottom);
    assert!(!bottom.contains("G36*") && !bottom.contains("%LPC*%"));
}

#[test]
fn a_foreign_net_pad_inside_the_zone_is_cleared_not_connected() {
    let mut b = board();
    pad(&mut b, 10.0, 10.0, "GND", true);
    pad(&mut b, 20.0, 15.0, "VCC", false);
    b.zones.push(Zone::new("GND", Side::Top, rect(5.0, 5.0, 35.0, 25.0)));
    let top = gerber::copper(&b, Side::Top);
    let clear_part = &top[top.find("%LPC*%").unwrap()..];
    let flash_before_dark = &clear_part[..clear_part.find("%LPD*%").unwrap()];
    assert!(flash_before_dark.contains("X20000000Y15000000D03*"), "VCC pad is flashed in clear polarity");
    assert!(drc::shorts(&b).is_empty());
}

#[test]
fn zones_without_a_net_are_output_but_connect_nothing() {
    let mut b = board();
    pad(&mut b, 10.0, 10.0, "GND", true);
    pad(&mut b, 30.0, 20.0, "GND", true);
    b.zones.push(Zone::new("", Side::Top, rect(5.0, 5.0, 35.0, 25.0)));
    assert_eq!(ratsnest(&b).len(), 1);
    assert!(gerber::copper(&b, Side::Top).contains("G36*"));
}

#[test]
fn drc_checks_zone_geometry() {
    let mut b = board();
    b.zones.push(Zone::new("GND", Side::Top, rect(5.0, 5.0, 60.0, 25.0)));
    b.zones.push(Zone::new("GND", Side::Top, vec![Point::new(1.0, 1.0), Point::new(2.0, 2.0)]));
    let msgs: Vec<_> = drc::check(&b).into_iter().map(|v| v.message).collect();
    assert!(msgs.iter().any(|m| m.contains("外")), "{msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("3点未満")), "{msgs:?}");
    b.zones.truncate(0);
    b.zones.push(Zone::new("GND", Side::Top, rect(2.0, 2.0, 38.0, 28.0)));
    assert!(drc::check(&b).is_empty());
}

#[test]
fn zones_round_trip_through_json_with_defaults() {
    let json = r#"{"name":"x","outline":[],"tracks":[],"pads":[],"vias":[],"holes":[],"silk":[],"rules":{},
        "zones":[{"net":"GND","side":"Top","polygon":[{"x":0,"y":0},{"x":1,"y":0},{"x":1,"y":1}]}]}"#;
    let b: Board = serde_json::from_str(json).unwrap();
    assert_eq!(b.zones.len(), 1);
    assert!((b.zones[0].clearance - 0.3).abs() < 1e-12 && b.zones[0].thermal);
    let again: Board = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
    assert_eq!(again.zones[0].polygon.len(), 3);
}

#[test]
fn designator_text_follows_the_part() {
    let mut b = board();
    b.set_part(Part { refdes: "R1".into(), value: "10k".into(), footprint: "R_0805".into(), pos: Point::new(10.0, 10.0), rot: 0 }).unwrap();
    assert_eq!(b.texts.len(), 1);
    let t = b.texts[0].clone();
    assert_eq!((t.text.as_str(), t.refdes.as_str(), t.side), ("R1", "R1", Side::Top));
    assert!((t.pos.x - 10.0).abs() < 1e-9 && t.pos.y > 10.0 + 0.7, "above the part, got {:?}", t.pos);

    b.place_part("R1", Point::new(20.0, 5.0), 0);
    assert_eq!(b.texts.len(), 1, "regenerated, not duplicated");
    assert!((b.texts[0].pos.x - 20.0).abs() < 1e-9 && b.texts[0].pos.y > 5.0);

    let silk = gerber::silk(&b, Side::Top);
    assert!(silk.contains("D01*"), "the stroke font produces drawn lines");
    assert!(gerber::silk(&b, Side::Bottom).matches("D01*").count() == 0);

    b.remove_part("R1");
    assert!(b.texts.is_empty());
}
