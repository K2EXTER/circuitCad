//! Pads with rounded corners (`PadShape::RoundRect`): the DRC, the pour clearance, the Gerber files and the footprints.

use crate::gerber_view::{parse_gerber, rasterize, Aperture, Frame, Prim};
use crate::package::{PackageSpec, PadSpec};
use crate::*;

fn two_pads(shape: PadShape, off: f64) -> Board {
    let mut b = Board::new("t", 30.0, 20.0);
    for (k, (x, net)) in [(10.0, "A"), (10.0 + off, "B")].into_iter().enumerate() {
        let mut p = Pad::new(Point::new(x, 10.0 + if k == 1 { off } else { 0.0 }), shape, None, Side::Top);
        p.net = net.into();
        b.pads.push(p);
    }
    b
}

#[test]
fn the_shape_knows_its_corner_area_and_inside() {
    let s = PadShape::RoundRect { w: 2.0, h: 1.0, r: ROUNDRECT_DEFAULT };
    assert!((s.corner_radius() - 0.25).abs() < 1e-12);
    assert!((s.area() - (2.0 - (4.0 - std::f64::consts::PI) * 0.0625)).abs() < 1e-12);
    // the very corner of the box is outside, a point just in from it is inside
    assert!(!s.contains(0.99, 0.49, 0.0) && s.contains(0.8, 0.3, 0.0));
    // shrunk by a margin: a sharp rectangle keeps its corner (rounded only by what is left of r)
    let sharp = PadShape::Rect { w: 2.0, h: 1.0 };
    assert!(sharp.contains(0.89, 0.39, 0.1) && !sharp.contains(0.91, 0.0, 0.1));
    // turned and grown: the corners stay concentric
    assert_eq!(s.rotated90(), PadShape::RoundRect { w: 1.0, h: 2.0, r: ROUNDRECT_DEFAULT });
    let g = s.grown(0.05);
    assert!((g.corner_radius() - 0.30).abs() < 1e-9 && (g.size().0 - 2.1).abs() < 1e-12);
}

#[test]
fn rounded_corners_give_the_drc_more_room_diagonally() {
    // two 1 mm pads corner to corner, 0.08 mm apart in x and y: sharp corners are 0.11 mm apart (under the 0.15 mm clearance),
    // rounded ones (r = 0.25 mm) 0.32 mm
    let sharp = drc::check(&two_pads(PadShape::Rect { w: 1.0, h: 1.0 }, 1.08));
    let round = drc::check(&two_pads(PadShape::RoundRect { w: 1.0, h: 1.0, r: ROUNDRECT_DEFAULT }, 1.08));
    assert!(round.len() < sharp.len(), "sharp {sharp:?}\nround {round:?}");
    // side by side the faces are straight: the same gap either way
    let a = drc::check(&two_pads(PadShape::Rect { w: 1.0, h: 1.0 }, 0.0));
    let b = drc::check(&two_pads(PadShape::RoundRect { w: 1.0, h: 1.0, r: ROUNDRECT_DEFAULT }, 0.0));
    assert_eq!(a.len(), b.len());
}

#[test]
fn the_pour_keeps_clear_round_the_corner_not_square() {
    // the clearance outline of a sharp rectangle is rounded at its corners (radius = the clearance): no point of it is further
    // than the clearance from the pad
    let pos = Point::new(5.0, 5.0);
    for shape in [PadShape::Rect { w: 2.0, h: 1.0 }, PadShape::RoundRect { w: 2.0, h: 1.0, r: ROUNDRECT_DEFAULT }] {
        let path = zonefill::pad_path(pos, shape, 0.2);
        assert!(path.len() > 4, "{shape:?}: {}", path.len());
        let r = shape.corner_radius();
        for q in &path {
            let (dx, dy) = ((q[0] - pos.x).abs() - (1.0 - r), (q[1] - pos.y).abs() - (0.5 - r));
            let d = dx.max(0.0).hypot(dy.max(0.0)) - r;
            assert!((d - 0.2).abs() < 1e-9, "{shape:?}: {q:?} is {d} from the pad");
        }
    }
    // the pad itself (no clearance) of a sharp rectangle is still its 4 corners
    assert_eq!(zonefill::pad_path(pos, PadShape::Rect { w: 2.0, h: 1.0 }, 0.0).len(), 4);
}

#[test]
fn the_gerber_writes_a_macro_and_reads_back_rounded() {
    let mut b = Board::new("t", 30.0, 20.0);
    b.pads.push(Pad::new(Point::new(10.0, 10.0), PadShape::RoundRect { w: 2.0, h: 1.0, r: ROUNDRECT_DEFAULT }, None, Side::Top));
    let g = gerber::copper(&b, Side::Top);
    assert!(g.contains("%AMRoundRect*") && g.contains("RoundRect,2.000000X1.000000X0.250000*%"), "{g}");
    // the mask opening is grown with concentric corners
    assert!(gerber::mask(&b, Side::Top).contains("RoundRect,2.100000X1.100000X0.300000"));
    let prims = parse_gerber(&g).unwrap();
    assert!(prims.iter().any(|p| matches!(p, Prim::Flash { ap: Aperture::RoundRect(w, h, r), .. } if (*w - 2.0).abs() < 1e-9 && (*h - 1.0).abs() < 1e-9 && (*r - 0.25).abs() < 1e-9)));
    // drawn: the middle and the edges are copper, the very corner is not
    let f = Frame { x0: 8.0, y1: 12.0, px: 50.0, w: 200, h: 200 };
    let px = rasterize(&prims, &f);
    let at = |x: f64, y: f64| px[(((12.0 - y) * 50.0) as usize) * 200 + ((x - 8.0) * 50.0) as usize];
    assert_eq!(at(10.0, 10.0), 255);
    assert_eq!(at(10.95, 10.0), 255);
    assert_eq!(at(10.97, 10.47), 0);
    // a sharp rectangle stays a plain aperture with no macro
    let mut s = Board::new("t", 30.0, 20.0);
    s.pads.push(Pad::new(Point::new(10.0, 10.0), PadShape::Rect { w: 2.0, h: 1.0 }, None, Side::Top));
    assert!(!gerber::copper(&s, Side::Top).contains("%AM"));
}

#[test]
fn built_in_smd_pads_are_rounded_and_explicit_ones_keep_their_shape() {
    let fp = footprints::get("R_0805").unwrap();
    assert!(fp.pads.iter().all(|p| matches!(p.shape, PadShape::RoundRect { r, .. } if r == ROUNDRECT_DEFAULT)), "{:?}", fp.pads);
    // a pin header keeps the square pin 1 (it has a hole)
    let hdr = footprints::get("PinHeader_1x3").unwrap();
    assert!(matches!(hdr.pads[0].shape, PadShape::Rect { .. }));
    let pad = |shape: &str, r: f64| PadSpec { pin: "1".into(), x: 0.0, y: 0.0, shape: shape.into(), w: 1.0, h: 0.6, r, drill: None, slot: 0.0, slot_v: false };
    let build = |p: PadSpec| PackageSpec::Explicit { pads: vec![p], holes: vec![] }.build("x").unwrap().pads[0].shape;
    assert_eq!(build(pad("rect", 0.0)), PadShape::Rect { w: 1.0, h: 0.6 });
    assert_eq!(build(pad("roundrect", 0.0)), PadShape::RoundRect { w: 1.0, h: 0.6, r: ROUNDRECT_DEFAULT });
    assert_eq!(build(pad("roundrect", 0.1)), PadShape::RoundRect { w: 1.0, h: 0.6, r: 0.1 });
    // the JSON of a pad without corners has no `r`
    assert!(!serde_json::to_string(&pad("rect", 0.0)).unwrap().contains("\"r\""));
}

#[test]
fn rounding_a_board_leaves_holes_and_fiducials_alone() {
    let mut b = Board::new("t", 30.0, 20.0);
    b.pads.push(Pad::new(Point::new(5.0, 5.0), PadShape::Rect { w: 1.0, h: 0.6 }, None, Side::Top));
    b.pads.push(Pad::new(Point::new(8.0, 5.0), PadShape::Rect { w: 1.7, h: 1.7 }, Some(1.0), Side::Top));
    let mut fid = Pad::new(Point::new(2.0, 2.0), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
    fid.mask_open = 2.0;
    b.pads.push(fid);
    assert_eq!(b.round_smd_pads(ROUNDRECT_DEFAULT), 1);
    assert!(matches!(b.pads[0].shape, PadShape::RoundRect { .. }));
    assert!(matches!(b.pads[1].shape, PadShape::Rect { .. }) && matches!(b.pads[2].shape, PadShape::Rect { .. }));
    assert_eq!(b.round_smd_pads(ROUNDRECT_DEFAULT), 0);
}
