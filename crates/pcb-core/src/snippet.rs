//! Board snippets: the layout of some parts, with the tracks and vias between them, saved to be used on another board that has the
//! same parts (the same reference designators and footprints). Positions are kept relative to the middle of what was selected.
//!
//! Putting a snippet moves the parts of the board that match (by reference designator, with the same footprint) to the saved places
//! and adds the saved copper. Tracks of the board that end on the pads of a part that moved are taken away first when the snippet brings
//! copper of its own (they would only be stretched between the old and the new place).

use crate::model::*;
use serde::{Deserialize, Serialize};

pub const SNIPPET_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BoardSnippet {
    pub version: u32,
    /// Position, turn and side of each part, relative to the middle of the selection.
    pub parts: Vec<SnippetPart>,
    pub tracks: Vec<Track>,
    pub vias: Vec<Via>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnippetPart {
    pub refdes: String,
    pub footprint: String,
    pub pos: Point,
    pub rot: u8,
    pub side: Side,
}

/// What `apply` did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Applied {
    pub moved: Vec<String>,
    /// Parts of the snippet that the board does not have.
    pub missing: Vec<String>,
    /// Parts the board has with another footprint.
    pub other_footprint: Vec<String>,
    /// Parts that are locked in place.
    pub locked: Vec<String>,
    pub tracks_added: usize,
    pub vias_added: usize,
    /// Tracks of the board taken away because they ended on a part that moved.
    pub tracks_removed: usize,
}

impl BoardSnippet {
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty() && self.tracks.is_empty() && self.vias.is_empty()
    }
}

fn shift(p: Point, d: Point, sign: f64) -> Point {
    Point { x: p.x + sign * d.x, y: p.y + sign * d.y }
}

/// Keep the parts, tracks and vias at these indexes of the board.
pub fn capture(board: &Board, parts: &[usize], tracks: &[usize], vias: &[usize]) -> BoardSnippet {
    let parts: Vec<&Part> = parts.iter().filter_map(|&i| board.parts.get(i)).collect();
    let tracks: Vec<&Track> = tracks.iter().filter_map(|&i| board.tracks.get(i)).collect();
    let vias: Vec<&Via> = vias.iter().filter_map(|&i| board.vias.get(i)).collect();
    let pts: Vec<Point> = parts.iter().map(|p| p.pos).chain(tracks.iter().flat_map(|t| [t.a, t.b])).chain(vias.iter().map(|v| v.pos)).collect();
    if pts.is_empty() {
        return BoardSnippet::default();
    }
    let (x0, x1) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
    let (y0, y1) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
    // the middle on a 0.1 mm step, so the snippet's numbers stay short
    let mid = Point { x: ((x0 + x1) * 5.0).round() / 10.0, y: ((y0 + y1) * 5.0).round() / 10.0 };
    BoardSnippet {
        version: SNIPPET_VERSION,
        parts: parts.iter().map(|p| SnippetPart { refdes: p.refdes.clone(), footprint: p.footprint.clone(), pos: shift(p.pos, mid, -1.0), rot: p.rot, side: p.side }).collect(),
        tracks: tracks.iter().map(|t| Track { a: shift(t.a, mid, -1.0), b: shift(t.b, mid, -1.0), ..(*t).clone() }).collect(),
        vias: vias.iter().map(|v| Via { pos: shift(v.pos, mid, -1.0), ..(*v).clone() }).collect(),
    }
}

/// Put the snippet on the board with the middle of the selection at `center`.
pub fn apply(board: &mut Board, s: &BoardSnippet, center: Point) -> Applied {
    let mut done = Applied::default();
    let mut matched: Vec<(&SnippetPart, Part)> = vec![];
    for sp in &s.parts {
        match board.parts.iter().find(|p| p.refdes == sp.refdes) {
            None => done.missing.push(sp.refdes.clone()),
            Some(p) if p.footprint != sp.footprint => done.other_footprint.push(sp.refdes.clone()),
            Some(_) if board.is_locked(&sp.refdes) => done.locked.push(sp.refdes.clone()),
            Some(p) => matched.push((sp, p.clone())),
        }
    }
    // the tracks that end on the pads of the parts that move, at the places they have now
    if !s.tracks.is_empty() {
        let pads: Vec<Point> = board.pads.iter().filter(|p| matched.iter().any(|(sp, _)| sp.refdes == p.refdes)).map(|p| p.pos).collect();
        let near = |a: Point| pads.iter().any(|p| (p.x - a.x).abs() < 1e-3 && (p.y - a.y).abs() < 1e-3);
        let before = board.tracks.len();
        board.tracks.retain(|t| !(near(t.a) || near(t.b)));
        done.tracks_removed = before - board.tracks.len();
    }
    for (sp, old) in matched {
        let part = Part { pos: shift(sp.pos, center, 1.0), rot: sp.rot % 4, side: sp.side, ..old };
        if board.set_part(part).is_ok() {
            done.moved.push(sp.refdes.clone());
        }
    }
    for t in &s.tracks {
        board.tracks.push(Track { a: shift(t.a, center, 1.0), b: shift(t.b, center, 1.0), ..t.clone() });
        done.tracks_added += 1;
    }
    for v in &s.vias {
        board.vias.push(Via { pos: shift(v.pos, center, 1.0), ..v.clone() });
        done.vias_added += 1;
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(r: &str, x: f64, y: f64) -> Part {
        Part { refdes: r.into(), value: "100n".into(), footprint: "C_0603".into(), pos: Point::new(x, y), rot: 0, side: Side::Top }
    }

    fn board() -> Board {
        let mut b = Board::new("t", 80.0, 60.0);
        b.set_part(part("C1", 10.0, 10.0)).unwrap();
        b.set_part(part("C2", 20.0, 10.0)).unwrap();
        b
    }

    #[test]
    fn a_layout_moves_to_another_board_with_the_same_parts() {
        let mut a = board();
        let p1 = a.pads.iter().find(|p| p.refdes == "C1" && p.pin == "2").unwrap().pos;
        let p2 = a.pads.iter().find(|p| p.refdes == "C2" && p.pin == "1").unwrap().pos;
        a.tracks.push(Track { side: Side::Top, a: p1, b: p2, width: 0.25 });
        let snip = capture(&a, &[0, 1], &[0], &[]);
        assert_eq!(snip.parts.len(), 2);

        // the other board has the parts somewhere else, and an old track from C1
        let mut b = Board::new("u", 80.0, 60.0);
        b.set_part(part("C1", 50.0, 40.0)).unwrap();
        b.set_part(part("C2", 55.0, 45.0)).unwrap();
        let q1 = b.pads.iter().find(|p| p.refdes == "C1" && p.pin == "2").unwrap().pos;
        b.tracks.push(Track { side: Side::Top, a: q1, b: Point::new(q1.x, q1.y + 3.0), width: 0.2 });
        let done = apply(&mut b, &snip, Point::new(40.0, 30.0));
        assert_eq!(done.moved, vec!["C1", "C2"]);
        assert_eq!((done.tracks_added, done.tracks_removed), (1, 1));
        // the layout is the same, shifted by the difference of the middles (a: 15,10 -> b: 40,30)
        let c1 = b.parts.iter().find(|p| p.refdes == "C1").unwrap().pos;
        let c2 = b.parts.iter().find(|p| p.refdes == "C2").unwrap().pos;
        assert!(((c2.x - c1.x) - 10.0).abs() < 1e-9 && (c2.y - c1.y).abs() < 1e-9, "{c1:?} {c2:?}");
        assert!((c1.x - 35.0).abs() < 1e-9 && (c1.y - 30.0).abs() < 1e-9, "{c1:?}");
        // the saved track joins the two pads where they now are
        let n1 = b.pads.iter().find(|p| p.refdes == "C1" && p.pin == "2").unwrap().pos;
        let n2 = b.pads.iter().find(|p| p.refdes == "C2" && p.pin == "1").unwrap().pos;
        assert_eq!(b.tracks.len(), 1);
        assert!((b.tracks[0].a.x - n1.x).abs() < 1e-6 && (b.tracks[0].b.x - n2.x).abs() < 1e-6, "{:?} {n1:?} {n2:?}", b.tracks[0]);
    }

    #[test]
    fn what_cannot_be_matched_is_left_alone_and_reported() {
        let a = board();
        let snip = capture(&a, &[0, 1], &[], &[]);
        let mut b = Board::new("u", 80.0, 60.0);
        b.set_part(part("C1", 5.0, 5.0)).unwrap();
        b.set_part(Part { footprint: "C_0805".into(), ..part("C2", 9.0, 9.0) }).unwrap();
        b.locked_parts = vec!["C1".into()];
        let done = apply(&mut b, &snip, Point::new(30.0, 30.0));
        assert!(done.moved.is_empty());
        assert_eq!((done.locked.as_slice(), done.other_footprint.as_slice()), (&["C1".to_string()][..], &["C2".to_string()][..]));
        let c1 = b.parts.iter().find(|p| p.refdes == "C1").unwrap().pos;
        assert_eq!((c1.x, c1.y), (5.0, 5.0), "a locked part stays");
        let mut c = Board::new("v", 80.0, 60.0);
        c.set_part(part("C9", 1.0, 1.0)).unwrap();
        assert_eq!(apply(&mut c, &snip, Point::new(0.0, 0.0)).missing, vec!["C1", "C2"]);
    }

    #[test]
    fn the_snippet_goes_through_json() {
        let a = board();
        let snip = capture(&a, &[0], &[], &[]);
        let back: BoardSnippet = serde_json::from_str(&serde_json::to_string(&snip).unwrap()).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&snip).unwrap());
        assert!(capture(&a, &[], &[], &[]).is_empty());
    }
}
