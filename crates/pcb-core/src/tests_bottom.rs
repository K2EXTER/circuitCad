use crate::netlist::{import, NetlistNet, NetlistPart};
use crate::ratsnest::ratsnest;
use crate::*;

fn part(refdes: &str, footprint: &str, x: f64, y: f64, rot: u8, side: Side) -> Part {
    Part { refdes: refdes.into(), value: "1k".into(), footprint: footprint.into(), pos: Point::new(x, y), rot, side }
}

fn pad_pos(b: &Board, refdes: &str, pin: &str) -> Point {
    b.pads.iter().find(|p| p.refdes == refdes && p.pin == pin).unwrap().pos
}

fn board() -> Board {
    Board::new("t", 40.0, 30.0)
}

#[test]
fn a_bottom_part_is_mirrored_left_to_right() {
    let mut b = board();
    b.set_part(part("R1", "R_0805", 10.0, 10.0, 0, Side::Top)).unwrap();
    b.set_part(part("R2", "R_0805", 20.0, 10.0, 0, Side::Bottom)).unwrap();
    assert!((pad_pos(&b, "R1", "1").x - (10.0 - 0.9125)).abs() < 1e-9);
    assert!((pad_pos(&b, "R2", "1").x - (20.0 + 0.9125)).abs() < 1e-9, "pin 1 swaps to the right side");
    assert!((pad_pos(&b, "R2", "2").x - (20.0 - 0.9125)).abs() < 1e-9);
    assert!(b.pads.iter().filter(|p| p.refdes == "R2").all(|p| p.side == Side::Bottom && p.drill.is_none()));
    assert!(b.pads.iter().filter(|p| p.refdes == "R1").all(|p| p.side == Side::Top));
    // silk and designator follow the part's side
    assert!(b.silk.iter().filter(|s| s.refdes == "R2").all(|s| s.side == Side::Bottom) && b.silk.iter().any(|s| s.refdes == "R2"));
    assert_eq!(b.texts.iter().find(|t| t.refdes == "R2").unwrap().side, Side::Bottom);
}

#[test]
fn rotation_is_applied_after_mirroring() {
    let mut b = board();
    b.set_part(part("R1", "R_0805", 10.0, 10.0, 1, Side::Top)).unwrap();
    b.set_part(part("R2", "R_0805", 10.0, 20.0, 1, Side::Bottom)).unwrap();
    // top: pin 1 is below the centre after a quarter turn, bottom: above
    assert!((pad_pos(&b, "R1", "1").y - (10.0 - 0.9125)).abs() < 1e-9);
    assert!((pad_pos(&b, "R2", "1").y - (20.0 + 0.9125)).abs() < 1e-9);
    // pad shapes are turned with the part
    let s = b.pads.iter().find(|p| p.refdes == "R2").unwrap().shape;
    assert_eq!(s, PadShape::Rect { w: 1.4, h: 1.025 });
}

#[test]
fn gerber_layers_follow_the_part_side() {
    let mut b = board();
    b.set_part(part("R1", "R_0805", 10.0, 10.0, 0, Side::Bottom)).unwrap();
    let (gtl, gbl) = (gerber::copper(&b, Side::Top), gerber::copper(&b, Side::Bottom));
    assert!(gbl.contains("R,1.025000X1.400000") && !gtl.contains("R,1.025000X1.400000"));
    let (gts, gbs) = (gerber::mask(&b, Side::Top), gerber::mask(&b, Side::Bottom));
    assert!(gbs.contains("R,1.125000X1.500000") && !gts.contains("R,1.125000X1.500000"), "mask opening = pad + 2 x 0.05");
    let (gto, gbo) = (gerber::silk(&b, Side::Top), gerber::silk(&b, Side::Bottom));
    assert!(!gto.contains("D01*"), "nothing on the top silk");
    assert!(gbo.matches("D01*").count() > 4, "outline and designator on the bottom silk");
}

#[test]
fn through_hole_parts_stay_on_both_copper_layers() {
    let mut b = board();
    b.set_part(part("J1", "PinHeader_1x2", 10.0, 10.0, 0, Side::Bottom)).unwrap();
    let (gtl, gbl) = (gerber::copper(&b, Side::Top), gerber::copper(&b, Side::Bottom));
    assert_eq!(gtl.matches("D03*").count(), 2);
    assert_eq!(gbl.matches("D03*").count(), 2);
    assert_eq!(excellon::plated(&b).matches("\nX").count(), 2);
}

#[test]
fn flipping_a_part_keeps_position_rotation_and_nets() {
    let mut b = board();
    b.set_part(part("R1", "R_0805", 10.0, 10.0, 2, Side::Top)).unwrap();
    b.pads.iter_mut().find(|p| p.pin == "1").unwrap().net = "SIG".into();
    let before = pad_pos(&b, "R1", "1");
    assert!(b.flip_part("R1"));
    let part = &b.parts[0];
    assert_eq!((part.side, part.rot, part.pos), (Side::Bottom, 2, Point::new(10.0, 10.0)));
    assert_eq!(b.pads.iter().find(|p| p.pin == "1").unwrap().net, "SIG");
    assert!((pad_pos(&b, "R1", "1").x - before.x).abs() > 1.0, "pad 1 moved to the other side of the part");
    // flipping twice restores the original geometry exactly
    assert!(b.flip_part("R1"));
    assert_eq!(pad_pos(&b, "R1", "1"), before);
    assert_eq!(b.parts.len(), 1);
    assert_eq!(b.texts.len(), 1);
    assert!(!b.flip_part("NOPE"));
}

#[test]
fn bottom_pads_connect_through_bottom_tracks_and_vias() {
    let mut b = board();
    b.set_part(part("R1", "R_0805", 10.0, 10.0, 0, Side::Bottom)).unwrap();
    b.set_part(part("R2", "R_0805", 20.0, 10.0, 0, Side::Top)).unwrap();
    for (r, pin, net) in [("R1", "1", "A"), ("R2", "2", "A")] {
        b.pads.iter_mut().find(|p| p.refdes == r && p.pin == pin).unwrap().net = net.into();
    }
    assert_eq!(ratsnest(&b).len(), 1);
    // bottom pad R1.1 -> bottom track -> via -> top track -> top pad R2.2
    let (a, c) = (pad_pos(&b, "R1", "1"), pad_pos(&b, "R2", "2"));
    let via = Point::new(14.0, a.y);
    b.tracks.push(Track { side: Side::Bottom, a, b: via, width: 0.25 });
    b.vias.push(Via { pos: via, diameter: 0.6, drill: 0.3 });
    b.tracks.push(Track { side: Side::Top, a: via, b: c, width: 0.25 });
    assert_eq!(ratsnest(&b).len(), 0);
    assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
    // a top-side track cannot reach a bottom SMD pad by itself
    b.tracks.clear();
    b.vias.clear();
    b.tracks.push(Track { side: Side::Top, a, b: c, width: 0.25 });
    assert_eq!(ratsnest(&b).len(), 1);
}

#[test]
fn parts_on_opposite_sides_may_overlap() {
    let mut b = board();
    b.set_part(part("R1", "R_0805", 10.0, 10.0, 0, Side::Top)).unwrap();
    b.set_part(part("R2", "R_0805", 10.0, 10.0, 0, Side::Bottom)).unwrap();
    for (r, net) in [("R1", "A"), ("R2", "B")] {
        for p in b.pads.iter_mut().filter(|p| p.refdes == r) {
            p.net = net.into();
        }
    }
    assert!(drc::clearance(&b).is_empty(), "different layers do not interact");
    // the same layer does
    b.flip_part("R2");
    assert!(!drc::clearance(&b).is_empty() || !drc::shorts(&b).is_empty());
}

#[test]
fn re_import_keeps_the_side_of_existing_parts() {
    let mut b = board();
    let parts = vec![NetlistPart { refdes: "R1".into(), value: "10k".into(), footprint: "R_0805".into() }];
    let nets = vec![NetlistNet { name: "A".into(), pins: vec![("R1".into(), "1".into())] }];
    import(&mut b, &parts, &nets);
    assert_eq!(b.parts[0].side, Side::Top, "new parts start on the top");
    b.flip_part("R1");
    let again = vec![NetlistPart { refdes: "R1".into(), value: "22k".into(), footprint: "R_0603".into() }];
    let rep = import(&mut b, &again, &nets);
    assert!(rep.warnings.is_empty());
    assert_eq!((b.parts[0].side, b.parts[0].value.as_str()), (Side::Bottom, "22k"));
    assert!(b.pads.iter().all(|p| p.side == Side::Bottom));
    assert_eq!(b.pads.iter().find(|p| p.pin == "1").unwrap().net, "A");
}

#[test]
fn placement_file_lists_the_layer() {
    let mut b = board();
    b.set_part(part("R1", "R_0805", 10.0, 10.0, 1, Side::Bottom)).unwrap();
    b.set_part(part("R2", "R_0805", 20.0, 10.0, 0, Side::Top)).unwrap();
    let cpl = assembly::cpl_csv(&b);
    assert!(cpl.contains("R1,10.0000mm,10.0000mm,Bottom,90"), "{cpl}");
    assert!(cpl.contains("R2,20.0000mm,10.0000mm,Top,0"), "{cpl}");
    // the BOM does not care about the side
    assert!(assembly::bom_csv(&b).contains("\"R1,R2\""));
}

#[test]
fn old_files_without_a_side_load_as_top_parts() {
    let json = r#"{"refdes":"R1","value":"1k","footprint":"R_0805","pos":{"x":1.0,"y":2.0},"rot":0}"#;
    let p: Part = serde_json::from_str(json).unwrap();
    assert_eq!(p.side, Side::Top);
    let b: Part = serde_json::from_str(&serde_json::to_string(&Part { side: Side::Bottom, ..p }).unwrap()).unwrap();
    assert_eq!(b.side, Side::Bottom);
}

#[test]
fn pads_sharing_a_pin_are_one_connection_and_all_get_the_net() {
    use crate::footprints::{Footprint, FpPad};
    // a SOT-223-like package: pin 2 is both a normal pad and the large tab
    let pad = |pin: &str, x: f64, y: f64, w: f64, h: f64| FpPad { pin: pin.into(), pos: Point::new(x, y), shape: PadShape::Rect { w, h }, drill: None, slot: 0.0, slot_v: false };
    let fp = Footprint {
        name: "TAB3".into(),
        holes: vec![],
        pads: vec![pad("1", -3.0, 2.3, 2.0, 1.5), pad("2", -3.0, 0.0, 2.0, 1.5), pad("3", -3.0, -2.3, 2.0, 1.5), pad("2", 3.0, 0.0, 2.0, 3.8)],
    };
    let mut b = board();
    b.add_footprint(fp);
    let parts = vec![NetlistPart { refdes: "U1".into(), value: "x".into(), footprint: "TAB3".into() }];
    let nets = vec![NetlistNet { name: "OUT".into(), pins: vec![("U1".into(), "2".into()), ("C1".into(), "1".into())] }];
    import(&mut b, &parts, &nets);
    assert_eq!(b.pads.iter().filter(|p| p.net == "OUT").count(), 2, "both pad 2 and the tab carry the net");
    // a second OUT pad elsewhere: the two U1 pads count as one connection, so one airwire is enough
    let mut other = Pad::new(Point::new(30.0, 20.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
    other.net = "OUT".into();
    b.pads.push(other);
    let wires = ratsnest(&b);
    assert_eq!(wires.len(), 1, "{wires:?}");
    // the pads of one pin never cause a short or a clearance error against each other
    assert!(drc::shorts(&b).is_empty());
}
