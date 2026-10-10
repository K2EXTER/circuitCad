//! Rough 3D shapes for the board preview: bodies and leads for every part, sized from its pads and
//! a height guessed from the footprint. They show how the board will look and help to spot parts
//! that collide with a case; they are not real component models.

use crate::model::*;
use std::collections::BTreeSet;

/// FR-4 thickness used for the preview (mm). The usual 2-layer board is 1.6 mm.
pub const BOARD_THICKNESS: f64 = 1.6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BodyShape {
    Box,
    /// A cylinder along this axis (0 = x, 1 = y, 2 = z); the box's other two sizes give its width.
    Cylinder(u8),
}

/// One solid of a part: `lo`..`hi` in board coordinates (z = 0 is the bottom face, z = `BOARD_THICKNESS` the top).
#[derive(Clone, Debug, PartialEq)]
pub struct PartBox {
    pub refdes: String,
    /// Index into `Board::parts`.
    pub part: usize,
    pub lo: [f64; 3],
    pub hi: [f64; 3],
    pub color: [u8; 3],
    pub bottom: bool,
    pub shape: BodyShape,
}

/// What a part looks like, as far as the preview can tell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// A chip with end caps (0603, 0805, SOD-123 ...).
    Chip,
    /// A leaded body lying on the board (axial resistor, diode).
    Axial,
    /// An upright can (electrolytic, disc capacitor).
    Can,
    /// A 5 mm LED.
    Led,
    /// A pin header: plastic body and long pins.
    Header,
    /// A surface-mount IC or transistor with legs.
    SmdIc,
    /// A through-hole IC (DIP).
    Dip,
    /// Anything else: one box.
    Other,
}

fn is_through_hole(fp: &str) -> bool {
    ["axial", "disc", "dip", "to-92", "to-220", "pinheader", "5mm", "do-", "transformer", "radial"].iter().any(|k| fp.contains(k))
}

fn prefix(refdes: &str) -> char {
    refdes.trim_start_matches(|c: char| !c.is_ascii_alphabetic()).chars().next().map(|c| c.to_ascii_uppercase()).unwrap_or('?')
}

/// The kind of part from its reference designator and footprint name.
pub fn part_kind(refdes: &str, footprint: &str) -> Kind {
    let fp = footprint.to_ascii_lowercase();
    let th = is_through_hole(&fp);
    // a pin header is a pin header whatever its designator says (a source or a probe point on a header is V1, TP1 ...)
    if fp.contains("pinheader") {
        return Kind::Header;
    }
    match prefix(refdes) {
        'R' | 'L' | 'C' | 'D' if fp.contains("5mm") => Kind::Led,
        'R' if fp.contains("axial") => Kind::Axial,
        'D' if fp.contains("do-") => Kind::Axial,
        'C' if th => Kind::Can,
        'R' | 'C' | 'L' | 'D' if !th => Kind::Chip,
        'J' | 'P' | 'H' => Kind::Header,
        'U' | 'Q' if fp.contains("dip") => Kind::Dip,
        'U' | 'Q' if !th => Kind::SmdIc,
        _ => Kind::Other,
    }
}

/// Body height (mm above the board) and colour guessed from the reference designator and footprint.
pub fn part_style(refdes: &str, footprint: &str) -> (f64, [u8; 3]) {
    let fp = footprint.to_ascii_lowercase();
    let th = is_through_hole(&fp);
    if fp.contains("pinheader") {
        return (2.5, [24, 24, 26]);
    }
    match prefix(refdes) {
        'R' if th => (2.4, [196, 160, 110]),
        'R' => (0.55, [30, 30, 32]),
        'C' if th => (4.5, [200, 130, 40]),
        'C' => (0.9, [160, 120, 70]),
        'L' => (2.5, [90, 90, 100]),
        'D' if fp.contains("5mm") || fp.contains("led") => (if fp.contains("5mm") { 8.5 } else { 0.8 }, [220, 60, 50]),
        'D' if th => (2.7, [30, 30, 32]),
        'D' => (1.0, [36, 36, 40]),
        'Q' | 'U' if fp.contains("to-220") => (4.5, [40, 40, 44]),
        'Q' if fp.contains("to-92") => (4.5, [32, 32, 36]),
        'Q' => (1.1, [36, 36, 40]),
        'U' if fp.contains("dip") => (3.3, [28, 28, 32]),
        'U' if fp.contains("to-92") => (4.5, [32, 32, 36]),
        'U' if fp.contains("tqfp") || fp.contains("qfn") => (1.2, [30, 30, 34]),
        'U' => (1.6, [30, 30, 34]),
        'J' | 'P' | 'H' => (2.5, [24, 24, 26]),
        'T' => (15.0, [70, 70, 80]),
        'Y' | 'X' => (3.5, [190, 190, 200]),
        _ if th => (3.0, [60, 60, 66]),
        _ => (2.0, [90, 90, 96]),
    }
}

/// Total height above the board including the leads that stick out (pins of a header), for case checks.
pub fn part_total_height(refdes: &str, footprint: &str) -> f64 {
    let (h, _) = part_style(refdes, footprint);
    match part_kind(refdes, footprint) {
        Kind::Header => h + 6.0,
        _ => h,
    }
}

const SILVER: [u8; 3] = [196, 198, 204];
const GOLD: [u8; 3] = [214, 176, 58];

struct Solid {
    /// Offsets along x, y and above the board surface (z), low and high.
    lo: [f64; 3],
    hi: [f64; 3],
    color: [u8; 3],
    shape: BodyShape,
}

fn solid(lo: [f64; 3], hi: [f64; 3], color: [u8; 3], shape: BodyShape) -> Solid {
    Solid { lo, hi, color, shape }
}

/// All the solids of one part, in board x / y and height above the board surface.
fn solids(board: &Board, part: &Part) -> Vec<Solid> {
    let pads: Vec<&Pad> = board.pads.iter().filter(|p| p.refdes == part.refdes).collect();
    if pads.is_empty() {
        return vec![];
    }
    let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
    for p in &pads {
        let (w, h) = p.shape.size();
        lo = Point::new(lo.x.min(p.pos.x - w / 2.0), lo.y.min(p.pos.y - h / 2.0));
        hi = Point::new(hi.x.max(p.pos.x + w / 2.0), hi.y.max(p.pos.y + h / 2.0));
    }
    let (kind, (h, color)) = (part_kind(&part.refdes, &part.footprint), part_style(&part.refdes, &part.footprint));
    let (cx, cy) = ((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
    let (sx, sy) = (hi.x - lo.x, hi.y - lo.y);
    // the long way across the pads
    let along_x = sx >= sy;
    let b = BodyShape::Box;
    match kind {
        Kind::Chip => {
            // body in the middle, a silver cap at each end
            let (len, wid) = if along_x { (sx, sy) } else { (sy, sx) };
            let wid = wid * 0.8;
            let cap = len * 0.2;
            let mk = |a0: f64, a1: f64, c: [u8; 3], hh: f64| {
                if along_x { solid([cx - len / 2.0 + a0, cy - wid / 2.0, 0.0], [cx - len / 2.0 + a1, cy + wid / 2.0, hh], c, b) } else { solid([cx - wid / 2.0, cy - len / 2.0 + a0, 0.0], [cx + wid / 2.0, cy - len / 2.0 + a1, hh], c, b) }
            };
            vec![mk(cap, len - cap, color, h), mk(0.0, cap, SILVER, h), mk(len - cap, len, SILVER, h)]
        }
        Kind::Axial => {
            let (len, dia) = (if along_x { sx } else { sy } * 0.55, h);
            let (a, c) = if along_x { ([cx - len / 2.0, cy - dia / 2.0, 0.3], [cx + len / 2.0, cy + dia / 2.0, 0.3 + dia]) } else { ([cx - dia / 2.0, cy - len / 2.0, 0.3], [cx + dia / 2.0, cy + len / 2.0, 0.3 + dia]) };
            let wire = 0.45;
            let lead = if along_x { solid([lo.x + 0.6, cy - wire / 2.0, 0.3 + dia / 2.0 - wire / 2.0], [hi.x - 0.6, cy + wire / 2.0, 0.3 + dia / 2.0 + wire / 2.0], SILVER, b) } else { solid([cx - wire / 2.0, lo.y + 0.6, 0.3 + dia / 2.0 - wire / 2.0], [cx + wire / 2.0, hi.y - 0.6, 0.3 + dia / 2.0 + wire / 2.0], SILVER, b) };
            vec![lead, solid(a, c, color, BodyShape::Cylinder(if along_x { 0 } else { 1 }))]
        }
        Kind::Can => {
            let dia = (sx.max(sy) * 2.0).max(5.0);
            vec![solid([cx - dia / 2.0, cy - dia / 2.0, 0.0], [cx + dia / 2.0, cy + dia / 2.0, h], color, BodyShape::Cylinder(2))]
        }
        Kind::Led => {
            let dia = 5.0;
            vec![
                solid([cx - dia / 2.0 - 0.3, cy - dia / 2.0 - 0.3, 0.0], [cx + dia / 2.0 + 0.3, cy + dia / 2.0 + 0.3, 1.2], color, BodyShape::Cylinder(2)),
                solid([cx - dia / 2.0, cy - dia / 2.0, 1.2], [cx + dia / 2.0, cy + dia / 2.0, 5.5], color, BodyShape::Cylinder(2)),
                solid([cx - dia * 0.35, cy - dia * 0.35, 5.5], [cx + dia * 0.35, cy + dia * 0.35, h], color, BodyShape::Cylinder(2)),
            ]
        }
        Kind::Header => {
            let mut v = vec![solid([lo.x - 0.5, lo.y - 0.5, 0.0], [hi.x + 0.5, hi.y + 0.5, h], color, b)];
            for p in &pads {
                v.push(solid([p.pos.x - 0.32, p.pos.y - 0.32, 0.0], [p.pos.x + 0.32, p.pos.y + 0.32, h + 6.0], GOLD, b));
            }
            v
        }
        Kind::SmdIc => {
            // legs under the body, the body lifted a little
            let leg_h = 0.25;
            let mut v: Vec<Solid> = pads
                .iter()
                .map(|p| {
                    let (w, hh) = p.shape.size();
                    solid([p.pos.x - w / 2.0, p.pos.y - hh / 2.0, 0.0], [p.pos.x + w / 2.0, p.pos.y + hh / 2.0, leg_h], SILVER, b)
                })
                .collect();
            let pad_len = pads.iter().map(|p| p.shape.size().0.max(p.shape.size().1)).sum::<f64>() / pads.len() as f64;
            let s = (pad_len * 0.35).min(sx.min(sy) * 0.25);
            v.push(solid([lo.x + s, lo.y + s, leg_h], [hi.x - s, hi.y - s, leg_h + h], color, b));
            v
        }
        Kind::Dip => {
            // two rows of pins: the plastic sits between them
            let xs: BTreeSet<i64> = pads.iter().map(|p| (p.pos.x * 100.0).round() as i64).collect();
            let (shx, shy) = if xs.len() <= 2 && sx > 6.0 { (1.4, 0.2) } else { (0.2, 1.4) };
            vec![solid([lo.x + shx, lo.y + shy, 0.0], [hi.x - shx, hi.y - shy, h], color, b)]
        }
        Kind::Other => vec![solid([lo.x - 0.1, lo.y - 0.1, 0.0], [hi.x + 0.1, hi.y + 0.1, h], color, b)],
    }
}

/// The solids of every part that has pads. Bottom-side parts hang below the board.
pub fn part_boxes(board: &Board) -> Vec<PartBox> {
    let mut out = vec![];
    for (i, part) in board.parts.iter().enumerate() {
        let bottom = part.side == Side::Bottom;
        for s in solids(board, part) {
            let (z0, z1) = if bottom { (-s.hi[2], -s.lo[2]) } else { (BOARD_THICKNESS + s.lo[2], BOARD_THICKNESS + s.hi[2]) };
            out.push(PartBox { refdes: part.refdes.clone(), part: i, lo: [s.lo[0], s.lo[1], z0], hi: [s.hi[0], s.hi[1], z1], color: s.color, bottom, shape: s.shape });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(refdes: &str, x: f64, y: f64, w: f64, h: f64, drill: Option<f64>) -> Pad {
        let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w, h }, drill, Side::Top);
        p.refdes = refdes.into();
        p
    }

    fn part(refdes: &str, fp: &str, side: Side) -> Part {
        Part { refdes: refdes.into(), value: "v".into(), footprint: fp.into(), pos: Point::default(), rot: 0, side }
    }

    #[test]
    fn heights_follow_the_kind_of_part() {
        let h = |r: &str, f: &str| part_style(r, f).0;
        assert!(h("R1", "R_0805") < 1.0 && h("R1", "R_Axial_10.16") > 2.0);
        assert!(h("U1", "DIP-14_W7.62") > h("U2", "SOIC-8_P1.27"));
        assert!(h("D1", "LED_5mm") > 8.0 && h("D2", "LED_0805") < 1.0);
        assert!(part_total_height("J1", "PinHeader_1x4") > 8.0, "the pins of a header stick out of the plastic");
        assert_eq!(part_style("Z9", "").0, 2.0, "an unknown kind still gets a body");
    }

    #[test]
    fn kinds_are_told_apart() {
        assert_eq!(part_kind("R1", "R_0805"), Kind::Chip);
        assert_eq!(part_kind("R1", "R_Axial_10.16"), Kind::Axial);
        assert_eq!(part_kind("C1", "C_Disc_5.08"), Kind::Can);
        assert_eq!(part_kind("D1", "LED_5mm"), Kind::Led);
        assert_eq!(part_kind("D2", "D_DO-35_7.62"), Kind::Axial);
        assert_eq!(part_kind("J1", "PinHeader_1x4"), Kind::Header);
        assert_eq!(part_kind("V1", "PinHeader_1x2_PM"), Kind::Header, "a header used as a source symbol is still a header");
        assert_eq!(part_style("V1", "PinHeader_1x2_PM").0, 2.5);
        assert_eq!(part_kind("U1", "SOIC-8_P1.27"), Kind::SmdIc);
        assert_eq!(part_kind("U2", "DIP-14_W7.62"), Kind::Dip);
        assert_eq!(part_kind("T1", "Transformer_EI30"), Kind::Other);
    }

    #[test]
    fn a_chip_has_a_body_and_two_silver_caps_and_bottom_parts_hang_below() {
        let mut b = Board::new("t", 40.0, 25.0);
        for (x, r) in [(10.0, "R1"), (10.0, "R2")] {
            let y = if r == "R1" { 10.0 } else { 20.0 };
            b.pads.push(pad(r, x - 0.9, y, 1.0, 1.2, None));
            b.pads.push(pad(r, x + 0.9, y, 1.0, 1.2, None));
        }
        b.parts.push(part("R1", "R_0805", Side::Top));
        b.parts.push(part("R2", "R_0805", Side::Bottom));
        b.parts.push(part("X9", "", Side::Top));
        let boxes = part_boxes(&b);
        assert_eq!(boxes.len(), 6, "body + 2 caps for each chip; a part without pads has nothing");
        let top: Vec<&PartBox> = boxes.iter().filter(|x| !x.bottom).collect();
        assert!(top.iter().all(|x| x.lo[2] == BOARD_THICKNESS && x.hi[2] > BOARD_THICKNESS && x.part == 0));
        assert_eq!(top.iter().filter(|x| x.color == SILVER).count(), 2);
        assert!(top.iter().map(|x| x.lo[0]).fold(f64::MAX, f64::min) < 9.2 && top.iter().map(|x| x.hi[0]).fold(f64::MIN, f64::max) > 10.8);
        assert!(boxes.iter().filter(|x| x.bottom).all(|x| x.hi[2] == 0.0 && x.lo[2] < 0.0 && x.part == 1));
    }

    #[test]
    fn headers_get_pins_ics_get_legs_and_axial_parts_a_round_body() {
        let mut b = Board::new("t", 40.0, 25.0);
        for k in 0..4 {
            b.pads.push(pad("J1", 5.0 + 2.54 * k as f64, 5.0, 1.7, 1.7, Some(1.0)));
        }
        for k in 0..4 {
            b.pads.push(pad("U1", 20.0 - 2.7, 10.0 + 1.27 * k as f64, 1.55, 0.6, None));
            b.pads.push(pad("U1", 20.0 + 2.7, 10.0 + 1.27 * k as f64, 1.55, 0.6, None));
        }
        b.pads.push(pad("R5", 30.0, 20.0, 1.8, 1.8, Some(0.9)));
        b.pads.push(pad("R5", 40.16 - 2.0, 20.0, 1.8, 1.8, Some(0.9)));
        b.parts.push(part("J1", "PinHeader_1x4", Side::Top));
        b.parts.push(part("U1", "SOIC-8_P1.27", Side::Top));
        b.parts.push(part("R5", "R_Axial_10.16", Side::Top));
        let boxes = part_boxes(&b);
        let of = |p: usize| boxes.iter().filter(|x| x.part == p).collect::<Vec<_>>();
        assert_eq!(of(0).len(), 5, "plastic + 4 pins");
        assert!(of(0).iter().any(|x| x.color == GOLD && x.hi[2] - BOARD_THICKNESS > 8.0));
        assert_eq!(of(1).len(), 9, "8 legs + body");
        assert!(of(1).iter().any(|x| x.color != SILVER && x.lo[2] > BOARD_THICKNESS), "the body sits on its legs");
        assert!(of(2).iter().any(|x| matches!(x.shape, BodyShape::Cylinder(0))), "a resistor lying along x");
    }
}
