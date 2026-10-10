//! Rough 3D shapes for the board preview: a body box for every part, sized from its pads and a
//! height guessed from the footprint. They are there to show how the board will look and to spot
//! parts that collide with a case; they are not real component models.

use crate::model::*;

/// FR-4 thickness used for the preview (mm). The usual 2-layer board is 1.6 mm.
pub const BOARD_THICKNESS: f64 = 1.6;

/// A part's body: `lo`..`hi` in board coordinates (z = 0 is the bottom face, z = `BOARD_THICKNESS` the top).
#[derive(Clone, Debug, PartialEq)]
pub struct PartBox {
    pub refdes: String,
    pub lo: [f64; 3],
    pub hi: [f64; 3],
    pub color: [u8; 3],
    pub bottom: bool,
}

/// Body height (mm above the board) and colour guessed from the reference designator and footprint.
pub fn part_style(refdes: &str, footprint: &str) -> (f64, [u8; 3]) {
    let fp = footprint.to_ascii_lowercase();
    let prefix = refdes.trim_start_matches(|c: char| !c.is_ascii_alphabetic()).chars().next().map(|c| c.to_ascii_uppercase()).unwrap_or('?');
    let through_hole = fp.contains("axial") || fp.contains("disc") || fp.contains("dip") || fp.contains("to-92") || fp.contains("to-220") || fp.contains("pinheader") || fp.contains("5mm") || fp.contains("do-");
    match prefix {
        'R' if through_hole => (2.4, [196, 160, 110]),
        'R' => (0.55, [30, 30, 32]),
        'C' if through_hole => (4.5, [200, 130, 40]),
        'C' => (0.9, [160, 120, 70]),
        'L' => (2.5, [90, 90, 100]),
        'D' if fp.contains("5mm") || fp.contains("led") => (if fp.contains("5mm") { 8.5 } else { 0.8 }, [220, 60, 50]),
        'D' if through_hole => (2.7, [30, 30, 32]),
        'D' => (1.0, [36, 36, 40]),
        'Q' | 'U' if fp.contains("to-220") => (4.5, [40, 40, 44]),
        'Q' if fp.contains("to-92") => (4.5, [32, 32, 36]),
        'Q' => (1.1, [36, 36, 40]),
        'U' if fp.contains("dip") => (3.3, [28, 28, 32]),
        'U' if fp.contains("to-92") => (4.5, [32, 32, 36]),
        'U' if fp.contains("tqfp") || fp.contains("qfn") => (1.2, [30, 30, 34]),
        'U' => (1.6, [30, 30, 34]),
        'J' | 'P' | 'H' => (8.5, [24, 24, 26]),
        'T' => (15.0, [70, 70, 80]),
        'Y' | 'X' => (3.5, [190, 190, 200]),
        _ if through_hole => (3.0, [60, 60, 66]),
        _ => (2.0, [90, 90, 96]),
    }
}

/// One box per part that has pads, spanning its pads (plus a little margin).
pub fn part_boxes(board: &Board) -> Vec<PartBox> {
    let mut out = vec![];
    for part in &board.parts {
        let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
        let mut any = false;
        for p in board.pads.iter().filter(|p| p.refdes == part.refdes) {
            let (w, h) = p.shape.size();
            lo = Point::new(lo.x.min(p.pos.x - w / 2.0), lo.y.min(p.pos.y - h / 2.0));
            hi = Point::new(hi.x.max(p.pos.x + w / 2.0), hi.y.max(p.pos.y + h / 2.0));
            any = true;
        }
        if !any {
            continue;
        }
        // pads sit at the edges of a chip: its body is about as wide as the pads' span, a bit narrower across
        let (margin, (h, color)) = (0.1, part_style(&part.refdes, &part.footprint));
        let bottom = part.side == Side::Bottom;
        let (z0, z1) = if bottom { (-h, 0.0) } else { (BOARD_THICKNESS, BOARD_THICKNESS + h) };
        out.push(PartBox { refdes: part.refdes.clone(), lo: [lo.x - margin, lo.y - margin, z0], hi: [hi.x + margin, hi.y + margin, z1], color, bottom });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_follow_the_kind_of_part() {
        let h = |r: &str, f: &str| part_style(r, f).0;
        assert!(h("R1", "R_0805") < 1.0 && h("R1", "R_Axial_10.16") > 2.0);
        assert!(h("U1", "DIP-14_W7.62") > h("U2", "SOIC-8_P1.27"));
        assert!(h("J1", "PinHeader_1x4") > 8.0 && h("D1", "LED_5mm") > 8.0 && h("D2", "LED_0805") < 1.0);
        assert_eq!(part_style("Z9", "").0, 2.0, "an unknown kind still gets a body");
    }

    #[test]
    fn a_box_spans_the_pads_and_bottom_parts_hang_below() {
        let mut b = Board::new("t", 40.0, 25.0);
        for (x, r) in [(10.0, "R1"), (10.0, "R2")] {
            for dx in [-0.9, 0.9] {
                let mut p = Pad::new(Point::new(x + dx, if r == "R1" { 10.0 } else { 20.0 }), PadShape::Rect { w: 1.0, h: 1.2 }, None, Side::Top);
                p.refdes = r.into();
                b.pads.push(p);
            }
        }
        b.parts.push(Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(10.0, 10.0), rot: 0, side: Side::Top });
        b.parts.push(Part { refdes: "R2".into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(10.0, 20.0), rot: 0, side: Side::Bottom });
        b.parts.push(Part { refdes: "X9".into(), value: String::new(), footprint: String::new(), pos: Point::new(1.0, 1.0), rot: 0, side: Side::Top });
        let boxes = part_boxes(&b);
        assert_eq!(boxes.len(), 2, "a part without pads has no body");
        let (r1, r2) = (&boxes[0], &boxes[1]);
        assert!(r1.lo[0] < 9.0 && r1.hi[0] > 11.0 && r1.lo[2] == BOARD_THICKNESS && r1.hi[2] > BOARD_THICKNESS);
        assert!(r2.bottom && r2.hi[2] == 0.0 && r2.lo[2] < 0.0);
    }
}
