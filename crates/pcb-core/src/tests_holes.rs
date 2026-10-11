//! Part holes (locating pegs), slots and plated slot pads: they follow the part and reach the drill files.
use crate::gerber_view::*;
use crate::package::{HoleSpec, PackageSpec, PadSpec};
use crate::*;

fn usb_like() -> PackageSpec {
    let pad = |pin: &str, x: f64, y: f64, drill: f64, slot: f64, slot_v: bool| PadSpec {
        pin: pin.into(),
        x,
        y,
        shape: "oval".into(),
        w: 1.6,
        h: 1.6_f64.max(slot),
        drill: Some(drill),
        slot,
        slot_v,
    };
    PackageSpec::Explicit {
        pads: vec![pad("1", -1.0, 0.0, 0.9, 0.0, false), pad("SH", -6.0, -3.0, 0.8, 2.4, true)],
        holes: vec![HoleSpec { x: 3.0, y: 0.0, drill: 1.1, slot: 0.0, slot_v: false }, HoleSpec { x: 0.0, y: 5.0, drill: 1.0, slot: 2.0, slot_v: false }],
    }
}

fn board_with(rot: u8) -> Board {
    let mut b = Board::new("t", 40.0, 30.0);
    b.add_footprint(usb_like().build("USB").unwrap());
    b.set_part(Part { refdes: "J1".into(), value: "usb".into(), footprint: "USB".into(), pos: Point::new(20.0, 15.0), rot, side: Side::Top }).unwrap();
    b
}

#[test]
fn a_part_brings_its_holes_and_they_follow_it() {
    let mut b = board_with(0);
    assert_eq!(b.holes.len(), 2);
    assert!(b.holes.iter().all(|h| h.refdes == "J1"));
    assert!((b.holes[0].pos.x - 23.0).abs() < 1e-9 && (b.holes[0].pos.y - 15.0).abs() < 1e-9);
    assert!(!b.holes[0].is_slot() && b.holes[1].is_slot() && !b.holes[1].slot_v);
    // turning the part a quarter turn moves the peg and turns the slot
    b.place_part("J1", Point::new(20.0, 15.0), 1);
    assert_eq!(b.holes.len(), 2, "no duplicates after a move");
    assert!((b.holes[0].pos.x - 20.0).abs() < 1e-9 && (b.holes[0].pos.y - 18.0).abs() < 1e-9);
    assert!(b.holes[1].slot_v, "the slot turned with the part");
    let sh = b.pads.iter().find(|p| p.pin == "SH").unwrap();
    assert!(!sh.slot_v, "the plated slot (vertical to begin with) turned too");
    b.remove_part("J1");
    assert!(b.holes.is_empty());
}

#[test]
fn slots_are_written_with_g85_and_read_back() {
    let b = board_with(0);
    let files = export::fab_files(&b);
    let find = |suffix: &str| files.iter().find(|(n, _)| n.ends_with(suffix)).map(|(_, t)| t.clone()).unwrap();
    let pth = find("-PTH.DRL");
    let npth = find("-NPTH.DRL");
    assert!(pth.contains("G85"), "the plated slot is routed:\n{pth}");
    assert!(npth.contains("G85"), "the slot-shaped peg hole is routed:\n{npth}");
    let hits = parse_excellon_slots(&npth).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits.iter().filter(|h| h.2.is_some()).count(), 1);
    let p = Preview::from_files(&files, 1000).unwrap();
    assert_eq!(p.drills.iter().map(|d| d.1).sum::<usize>(), 4, "one entry per hole, a slot is one");
}

#[test]
fn bad_slots_and_holes_are_explained() {
    let bad = |slot: f64, drill: Option<f64>| PackageSpec::Explicit {
        pads: vec![PadSpec { pin: "1".into(), x: 0.0, y: 0.0, shape: "oval".into(), w: 1.6, h: 2.0, drill, slot, slot_v: false }],
        holes: vec![],
    };
    assert!(bad(0.5, Some(0.8)).build("x").unwrap_err().contains("slot"));
    assert!(bad(2.0, None).build("x").unwrap_err().contains("drill"));
    let hole = PackageSpec::Explicit {
        pads: vec![PadSpec { pin: "1".into(), x: 0.0, y: 0.0, shape: "circle".into(), w: 1.6, h: 0.0, drill: Some(0.8), slot: 0.0, slot_v: false }],
        holes: vec![HoleSpec { x: 0.0, y: 0.0, drill: 0.0, slot: 0.0, slot_v: false }],
    };
    assert!(hole.build("x").is_err());
}

#[test]
fn a_part_hole_outside_the_board_is_reported_by_the_drc() {
    let mut b = board_with(0);
    b.place_part("J1", Point::new(38.5, 15.0), 0);
    let r = drc::check(&b);
    assert!(r.iter().any(|v| v.message.contains("取り付け穴")), "{:?}", r.iter().map(|v| &v.message).collect::<Vec<_>>());
}
