//! The names of the pins of a pin header, written on the silk beside the pads (what each pin is for: F+, SA, GND ...).
//!
//! A part of one pad (a wire pad of a measuring instrument) gets its name right over its own outline box, centred, on the side that faces the
//! middle of the board, and its designator is dropped: the name says which pad it is, and one label per box cannot be taken for another's.
//!
//! The names come from the schematic (`Component::pin_names`). Each pad of the part gets a text just outside its row, on the
//! side that faces the middle of the board (so the text is over free board, not over the board's edge). The texts are marked
//! (`SilkText::refdes` is "REF#pin"), so they are made again, not added twice, when the command is run again.

use crate::model::*;

/// The marker of the silk texts made here.
fn marker(refdes: &str) -> String {
    format!("{refdes}#pin")
}

/// Write the names of the pins of the parts in `names` ((refdes, names of pin 1, 2 ...)) on the silk. The texts made before are
/// taken away first, for every part in the list. Returns how many texts were put. `height` is the height of the letters (mm).
pub fn write_pin_labels(board: &mut Board, names: &[(String, Vec<String>)], height: f64) -> usize {
    let centre = board
        .bounds()
        .map(|(lo, hi)| Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0))
        .unwrap_or_default();
    let mut put = 0;
    for (refdes, list) in names {
        board.texts.retain(|t| t.refdes != marker(refdes));
        let Some(part) = board.parts.iter().find(|p| &p.refdes == refdes).cloned() else { continue };
        let mut pads: Vec<&Pad> = board.pads.iter().filter(|p| &p.refdes == refdes).collect();
        // pin 1, 2, 3 ... in order along the row
        pads.sort_by_key(|p| p.pin.parse::<usize>().unwrap_or(usize::MAX));
        if pads.is_empty() {
            continue;
        }
        // a part of one pad: the name over its own box, the designator out
        if pads.len() == 1 {
            let p = pads[0];
            let Some(name) = list.first().filter(|s| !s.is_empty()) else { continue };
            let silk: Vec<&SilkLine> = board.silk.iter().filter(|l| &l.refdes == refdes).collect();
            let (lo, hi) = if silk.is_empty() {
                let (w, h) = p.shape.size();
                (Point::new(p.pos.x - w / 2.0, p.pos.y - h / 2.0), Point::new(p.pos.x + w / 2.0, p.pos.y + h / 2.0))
            } else {
                silk.iter().fold((Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN)), |(lo, hi), l| {
                    (Point::new(lo.x.min(l.a.x).min(l.b.x), lo.y.min(l.a.y).min(l.b.y)), Point::new(hi.x.max(l.a.x).max(l.b.x), hi.y.max(l.a.y).max(l.b.y)))
                })
            };
            let gap = 0.3;
            let x = (lo.x + hi.x) / 2.0;
            let (y_up, y_down) = (hi.y + gap + height / 2.0, lo.y - gap - height / 2.0);
            // above or below the box: the side where the text touches the least (other pads, the silk of other parts, the board's edge); on a tie
            // the side that faces the middle of the board
            let half_w = crate::strokefont::text_width(name, height) / 2.0 + 0.1;
            let hits = |y: f64| -> usize {
                let (x0, x1, y0, y1) = (x - half_w, x + half_w, y - height / 2.0 - 0.1, y + height / 2.0 + 0.1);
                let pad_hits = board.pads.iter().filter(|q| &q.refdes != refdes).filter(|q| {
                    let (w, h) = q.shape.size();
                    q.pos.x - w / 2.0 < x1 && q.pos.x + w / 2.0 > x0 && q.pos.y - h / 2.0 < y1 && q.pos.y + h / 2.0 > y0
                }).count();
                let silk_hits = board.silk.iter().filter(|l| &l.refdes != refdes).filter(|l| l.a.x.min(l.b.x) < x1 && l.a.x.max(l.b.x) > x0 && l.a.y.min(l.b.y) < y1 && l.a.y.max(l.b.y) > y0).count();
                let text_hits = board.texts.iter().filter(|t| &t.refdes != refdes && t.refdes != marker(refdes)).filter(|t| {
                    let hw = crate::strokefont::text_width(&t.text, t.height) / 2.0;
                    t.pos.x - hw < x1 && t.pos.x + hw > x0 && t.pos.y - t.height / 2.0 < y1 && t.pos.y + t.height / 2.0 > y0
                }).count();
                let outside = board.bounds().is_some_and(|(blo, bhi)| x0 < blo.x + 0.3 || x1 > bhi.x - 0.3 || y0 < blo.y + 0.3 || y1 > bhi.y - 0.3);
                pad_hits * 4 + silk_hits + text_hits * 2 + if outside { 100 } else { 0 }
            };
            let (h_up, h_down) = (hits(y_up), hits(y_down));
            let up = if h_up == h_down { centre.y >= p.pos.y } else { h_up < h_down };
            let y = if up { y_up } else { y_down };
            board.texts.retain(|t| &t.refdes != refdes);
            board.texts.push(SilkText { side: part.side, pos: Point::new(x, y), height, text: name.clone(), refdes: marker(refdes), rot: 0 });
            put += 1;
            continue;
        }
        let (first, last) = (pads[0].pos, pads[pads.len() - 1].pos);
        let len = first.dist(last);
        // a part of one pad has no row: the text goes above or below it, on the side of the middle of the board
        let (ax, ay) = if len < 1e-6 { (1.0, 0.0) } else { ((last.x - first.x) / len, (last.y - first.y) / len) };
        // the row is along x (the text goes above or below the pads) or along y (to the side of them)
        let along_x = ax.abs() >= ay.abs();
        let mid = Point::new((first.x + last.x) / 2.0, (first.y + last.y) / 2.0);
        let (px, py) = (-ay, ax);
        let sign = if (centre.x - mid.x) * px + (centre.y - mid.y) * py >= 0.0 { 1.0 } else { -1.0 };
        for p in pads {
            let Some(name) = p.pin.parse::<usize>().ok().and_then(|k| list.get(k.wrapping_sub(1))).filter(|s| !s.is_empty()) else { continue };
            let (w, h) = p.shape.size();
            let text_w = name.chars().count() as f64 * height * 0.75;
            let off = if along_x { h.max(w) / 2.0 + 0.4 + height / 2.0 } else { w.max(h) / 2.0 + 0.4 + text_w / 2.0 };
            let (dx, dy) = if along_x { (0.0, sign * py.signum() * off) } else { (sign * px.signum() * off, 0.0) };
            board.texts.push(SilkText { side: part.side, pos: Point::new(p.pos.x + dx, p.pos.y + dy), height, text: name.clone(), refdes: marker(refdes), rot: 0 });
            put += 1;
        }
    }
    put
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board_with_header() -> Board {
        let mut b = Board::new("t", 40.0, 30.0);
        b.parts.push(Part { refdes: "J3".into(), value: "".into(), footprint: "PinHeader_1x3".into(), pos: Point::new(5.0, 10.0), rot: 0, side: Side::Top });
        for k in 0..3 {
            let mut p = Pad::new(Point::new(5.0, 10.0 + 2.54 * k as f64), PadShape::Circle { d: 1.7 }, Some(1.0), Side::Top);
            p.refdes = "J3".into();
            p.pin = (k + 1).to_string();
            b.pads.push(p);
        }
        b
    }

    #[test]
    fn each_named_pin_gets_a_text_beside_its_pad_on_the_side_of_the_board() {
        let mut b = board_with_header();
        let n = write_pin_labels(&mut b, &[("J3".into(), vec!["F+".into(), String::new(), "GND".into()])], 1.0);
        assert_eq!(n, 2, "the pin without a name gets none");
        assert_eq!(b.texts.len(), 2);
        // the header is near the left edge: the names are on the right of the row, over the board
        assert!(b.texts.iter().all(|t| t.pos.x > 5.0 + 0.85), "{:?}", b.texts.iter().map(|t| t.pos).collect::<Vec<_>>());
        assert!((b.texts[0].pos.y - 10.0).abs() < 1e-9 && (b.texts[1].pos.y - 15.08).abs() < 1e-9);
        // run again: replaced, not added twice
        write_pin_labels(&mut b, &[("J3".into(), vec!["A".into(), "B".into(), "C".into()])], 1.0);
        assert_eq!(b.texts.len(), 3);
    }

    #[test]
    fn a_part_of_one_pad_gets_its_name_on_the_side_of_the_board() {
        let mut b = Board::new("t", 40.0, 30.0);
        b.parts.push(Part { refdes: "J3A".into(), value: "".into(), footprint: "WirePad_1x1".into(), pos: Point::new(20.0, 3.0), rot: 0, side: Side::Top });
        let mut p = Pad::new(Point::new(20.0, 3.0), PadShape::Circle { d: 2.2 }, Some(1.2), Side::Top);
        p.refdes = "J3A".into();
        p.pin = "1".into();
        b.pads.push(p);
        assert_eq!(write_pin_labels(&mut b, &[("J3A".into(), vec!["F+".into()])], 1.0), 1);
        assert!(b.texts[0].pos.y > 3.0 + 1.1 && (b.texts[0].pos.x - 20.0).abs() < 1e-9);
    }

    #[test]
    fn a_wire_pad_gets_its_name_centred_over_its_own_box_and_loses_its_designator() {
        let mut b = Board::new("t", 40.0, 30.0);
        b.parts.push(Part { refdes: "J3C".into(), value: "".into(), footprint: "WirePad_1x1".into(), pos: Point::new(15.78, 4.1), rot: 0, side: Side::Top });
        let mut p = Pad::new(Point::new(15.78, 4.1), PadShape::Circle { d: 1.8 }, Some(0.8), Side::Top);
        p.refdes = "J3C".into();
        p.pin = "1".into();
        b.pads.push(p);
        // the box round the pad (2.4 mm), and the designator below it
        let corners = [Point::new(14.58, 2.9), Point::new(16.98, 2.9), Point::new(16.98, 5.3), Point::new(14.58, 5.3)];
        for k in 0..4 {
            b.silk.push(SilkLine { side: Side::Top, a: corners[k], b: corners[(k + 1) % 4], width: 0.15, refdes: "J3C".into() });
        }
        b.texts.push(SilkText { side: Side::Top, pos: Point::new(14.9, 1.59), height: 1.0, text: "J3C".into(), refdes: "J3C".into(), rot: 0 });
        assert_eq!(write_pin_labels(&mut b, &[("J3C".into(), vec!["SH".into()])], 1.0), 1);
        assert_eq!(b.texts.len(), 1, "the designator is gone: {:?}", b.texts.iter().map(|t| &t.text).collect::<Vec<_>>());
        let t = &b.texts[0];
        assert_eq!(t.text, "SH");
        assert!((t.pos.x - 15.78).abs() < 1e-9, "centred over the box");
        assert!(t.pos.y > 5.3 + 0.25, "just outside the box, on the side of the board: {}", t.pos.y);
    }
}
