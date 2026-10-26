//! The names of the pins of a pin header, written on the silk beside the pads (what each pin is for: F+, SA, GND ...).
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
        if pads.len() < 2 {
            continue;
        }
        let (first, last) = (pads[0].pos, pads[pads.len() - 1].pos);
        let len = first.dist(last);
        if len < 1e-6 {
            continue;
        }
        let (ax, ay) = ((last.x - first.x) / len, (last.y - first.y) / len);
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
            board.texts.push(SilkText { side: part.side, pos: Point::new(p.pos.x + dx, p.pos.y + dy), height, text: name.clone(), refdes: marker(refdes) });
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
}
