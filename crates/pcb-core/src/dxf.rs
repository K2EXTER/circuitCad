//! A DXF (ASCII, the R12 flavour every CAD reads) of the board's mechanical side, for the person who designs the case: the outline,
//! the holes, the drills of the pads, and the place and size of every part. Units are millimetres.
//!
//! The board's y axis points up, as the DXF's does, so the coordinates are written as they are and the drawing looks as the board
//! does from the top. Layers: `BOARD_OUTLINE`, `HOLES` (mounting holes and slots), `PAD_DRILLS` (the drills of the pads),
//! `PARTS` (the box round each part's pads) and `PART_NAMES` (the references).

use crate::model::*;
use std::fmt::Write;

struct Dxf {
    body: String,
}

impl Dxf {
    fn pair(&mut self, code: i32, v: impl std::fmt::Display) {
        let _ = writeln!(self.body, "{code}\n{v}");
    }

    fn start(&mut self, kind: &str, layer: &str) {
        self.pair(0, kind);
        self.pair(8, layer);
    }

    fn pt(&mut self, i: i32, x: f64, y: f64) {
        self.pair(10 + i, format!("{x:.4}"));
        self.pair(20 + i, format!("{y:.4}"));
        self.pair(30 + i, "0.0");
    }

    fn line(&mut self, layer: &str, a: Point, b: Point) {
        self.start("LINE", layer);
        self.pt(0, a.x, a.y);
        self.pt(1, b.x, b.y);
    }

    fn circle(&mut self, layer: &str, c: Point, d: f64) {
        self.start("CIRCLE", layer);
        self.pt(0, c.x, c.y);
        self.pair(40, format!("{:.4}", d / 2.0));
    }

    /// An arc counter-clockwise from `from` to `to` degrees, in the DXF's own (y up) angles, round the board point `c`.
    fn arc(&mut self, layer: &str, c: Point, r: f64, from: f64, to: f64) {
        self.start("ARC", layer);
        self.pt(0, c.x, c.y);
        self.pair(40, format!("{r:.4}"));
        self.pair(50, format!("{from:.3}"));
        self.pair(51, format!("{to:.3}"));
    }

    /// A slot `len` long (end to end) and `width` wide, along x or (`vertical`) along y.
    fn slot(&mut self, layer: &str, c: Point, len: f64, width: f64, vertical: bool) {
        let r = width / 2.0;
        let half = ((len - width) / 2.0).max(0.0);
        if vertical {
            let (up, down) = (Point::new(c.x, c.y + half), Point::new(c.x, c.y - half));
            self.line(layer, Point::new(c.x - r, up.y), Point::new(c.x - r, down.y));
            self.line(layer, Point::new(c.x + r, up.y), Point::new(c.x + r, down.y));
            self.arc(layer, up, r, 0.0, 180.0);
            self.arc(layer, down, r, 180.0, 360.0);
        } else {
            let (l, rt) = (Point::new(c.x - half, c.y), Point::new(c.x + half, c.y));
            self.line(layer, Point::new(l.x, c.y - r), Point::new(rt.x, c.y - r));
            self.line(layer, Point::new(l.x, c.y + r), Point::new(rt.x, c.y + r));
            self.arc(layer, l, r, 90.0, 270.0);
            self.arc(layer, rt, r, 270.0, 90.0);
        }
    }

    fn text(&mut self, layer: &str, at: Point, height: f64, s: &str) {
        self.start("TEXT", layer);
        self.pt(0, at.x, at.y);
        self.pair(40, format!("{height:.3}"));
        self.pair(1, s);
    }
}

/// (layer, AutoCAD colour number)
const LAYERS: [(&str, i32); 5] = [("BOARD_OUTLINE", 7), ("HOLES", 1), ("PAD_DRILLS", 3), ("PARTS", 5), ("PART_NAMES", 2)];

/// The DXF text of the board.
pub fn mechanical(board: &Board) -> String {
    let mut d = Dxf { body: String::new() };
    let n = board.outline.len();
    for i in 0..n {
        d.line("BOARD_OUTLINE", board.outline[i], board.outline[(i + 1) % n]);
    }
    for h in &board.holes {
        if h.slot > h.drill {
            d.slot("HOLES", h.pos, h.slot, h.drill, h.slot_v);
        } else {
            d.circle("HOLES", h.pos, h.drill);
        }
    }
    for p in board.pads.iter().filter(|p| p.drill.is_some_and(|x| x > 0.0)) {
        let dr = p.drill.unwrap_or(0.0);
        if p.slot > dr {
            d.slot("PAD_DRILLS", p.pos, p.slot, dr, p.slot_v);
        } else {
            d.circle("PAD_DRILLS", p.pos, dr);
        }
    }
    for part in &board.parts {
        let mut bb: Option<(Point, Point)> = None;
        for p in board.pads.iter().filter(|p| p.refdes == part.refdes) {
            let (w, h) = p.shape.size();
            let (lo, hi) = (Point::new(p.pos.x - w / 2.0, p.pos.y - h / 2.0), Point::new(p.pos.x + w / 2.0, p.pos.y + h / 2.0));
            bb = Some(match bb {
                None => (lo, hi),
                Some((a, b)) => (Point::new(a.x.min(lo.x), a.y.min(lo.y)), Point::new(b.x.max(hi.x), b.y.max(hi.y))),
            });
        }
        let Some((lo, hi)) = bb else { continue };
        let corners = [lo, Point::new(hi.x, lo.y), hi, Point::new(lo.x, hi.y)];
        for i in 0..4 {
            d.line("PARTS", corners[i], corners[(i + 1) % 4]);
        }
        d.text("PART_NAMES", Point::new(lo.x, hi.y + 0.3), 0.8, &part.refdes);
    }

    let mut out = String::new();
    let body = std::mem::take(&mut d.body);
    let mut put = |code: i32, v: &str| {
        let _ = writeln!(out, "{code}\n{v}");
    };
    put(0, "SECTION");
    put(2, "HEADER");
    put(9, "$ACADVER");
    put(1, "AC1009");
    put(0, "ENDSEC");
    put(0, "SECTION");
    put(2, "TABLES");
    put(0, "TABLE");
    put(2, "LAYER");
    put(70, &LAYERS.len().to_string());
    for (name, color) in LAYERS {
        put(0, "LAYER");
        put(2, name);
        put(70, "0");
        put(62, &color.to_string());
        put(6, "CONTINUOUS");
    }
    put(0, "ENDTAB");
    put(0, "ENDSEC");
    put(0, "SECTION");
    put(2, "ENTITIES");
    drop(put);
    out.push_str(&body);
    let mut put = |code: i32, v: &str| {
        let _ = writeln!(out, "{code}
{v}");
    };
    put(0, "ENDSEC");
    put(0, "EOF");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(s: &str, kind: &str) -> usize {
        s.lines().collect::<Vec<_>>().windows(2).filter(|w| w[0] == "0" && w[1] == kind).count()
    }

    #[test]
    fn outline_holes_and_parts_are_written_as_they_are() {
        let mut b = Board::new("demo", 30.0, 20.0);
        b.holes.push(Hole::new(Point::new(2.0, 2.0), 3.2));
        let mut pad = Pad::new(Point::new(10.0, 5.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top);
        pad.refdes = "J1".into();
        b.pads.push(pad);
        b.parts.push(Part { refdes: "J1".into(), value: String::new(), footprint: String::new(), pos: Point::new(10.0, 5.0), rot: 0, side: Side::Top });
        let s = mechanical(&b);
        assert!(s.starts_with("0\nSECTION"));
        assert!(s.trim_end().ends_with("0\nEOF"));
        assert_eq!(count(&s, "LINE"), 4 + 4, "four outline sides and the box of J1");
        assert_eq!(count(&s, "CIRCLE"), 2, "the mounting hole and the pad's drill");
        assert_eq!(count(&s, "TEXT"), 1);
        assert!(s.contains("\n40\n1.6000\n"), "the 3.2 mm hole has radius 1.6");
        assert!(s.contains("\n20\n2.0000\n"), "y is kept: both the board and the DXF point up");
    }

    #[test]
    fn a_slot_is_two_lines_and_two_arcs() {
        let mut b = Board::new("demo", 30.0, 20.0);
        let mut h = Hole::new(Point::new(5.0, 5.0), 2.0);
        h.slot = 6.0;
        b.holes.push(h);
        let s = mechanical(&b);
        assert_eq!(count(&s, "ARC"), 2);
        assert_eq!(count(&s, "LINE"), 4 + 2);
        assert_eq!(count(&s, "CIRCLE"), 0);
    }
}
