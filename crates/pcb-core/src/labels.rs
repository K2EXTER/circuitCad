//! Part designators (R1, C3 ...) on the silkscreen: keep them off the copper. A letter over a track or a pad is cut away by the
//! solder mask opening, or printed on copper that the maker does not print on: either way it is hard to read. For every part
//! whose designator the user has not placed, the spots around the part (above, below, beside, a little further out) are tried
//! and the one that touches the least copper, other texts and the outline is taken. A designator that already clears everything stays
//! where it is. What the user placed by hand (`Board.label_offsets`) is never touched.

use crate::geom::inside_polygon;
use crate::model::*;

/// What `avoid_overlaps` did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LabelReport {
    /// Designators moved to a clearer spot.
    pub moved: usize,
    /// Designators that still touch something at the best spot there is.
    pub still_touching: usize,
}

/// An axis-aligned box: centre and half sizes.
#[derive(Clone, Copy)]
struct Rect {
    c: Point,
    hw: f64,
    hh: f64,
}

impl Rect {
    fn grown(self, m: f64) -> Rect {
        Rect { c: self.c, hw: self.hw + m, hh: self.hh + m }
    }

    fn contains(&self, p: Point) -> bool {
        (p.x - self.c.x).abs() <= self.hw && (p.y - self.c.y).abs() <= self.hh
    }

    fn overlaps(&self, o: &Rect) -> bool {
        (self.c.x - o.c.x).abs() < self.hw + o.hw && (self.c.y - o.c.y).abs() < self.hh + o.hh
    }

    fn corners(&self) -> [Point; 4] {
        [
            Point::new(self.c.x - self.hw, self.c.y - self.hh),
            Point::new(self.c.x + self.hw, self.c.y - self.hh),
            Point::new(self.c.x + self.hw, self.c.y + self.hh),
            Point::new(self.c.x - self.hw, self.c.y + self.hh),
        ]
    }

    /// Does the segment `a`-`b` touch the box?
    fn hit_by_segment(&self, a: Point, b: Point) -> bool {
        if self.contains(a) || self.contains(b) {
            return true;
        }
        let k = self.corners();
        (0..4).any(|i| segments_cross(a, b, k[i], k[(i + 1) % 4]))
    }
}

fn cross(o: Point, a: Point, b: Point) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

fn segments_cross(a: Point, b: Point, c: Point, d: Point) -> bool {
    let (d1, d2, d3, d4) = (cross(c, d, a), cross(c, d, b), cross(a, b, c), cross(a, b, d));
    ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0))
}

/// What a designator of side `side` must keep clear of, as boxes grown by the width of the line or the mask opening.
struct Obstacles {
    /// Copper that shows on the side of the text: (a, b, half width) tracks, and boxes for pads, vias and holes.
    tracks: Vec<(Point, Point, f64)>,
    boxes: Vec<Rect>,
    /// The body outlines of the parts (silk lines), by part.
    silk: Vec<(String, Point, Point)>,
}

fn obstacles(board: &Board, side: Side) -> Obstacles {
    let tracks = board.tracks.iter().filter(|t| t.side == side).map(|t| (t.a, t.b, t.width / 2.0)).collect();
    let mut boxes = vec![];
    for p in board.pads.iter().filter(|p| p.drill.is_some() || p.side == side) {
        let (w, h) = p.shape.size();
        boxes.push(Rect { c: p.pos, hw: w / 2.0 + board.rules.mask_expansion, hh: h / 2.0 + board.rules.mask_expansion });
    }
    for v in &board.vias {
        boxes.push(Rect { c: v.pos, hw: v.diameter / 2.0, hh: v.diameter / 2.0 });
    }
    for h in &board.holes {
        let (a, b) = h.ends();
        boxes.push(Rect { c: Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0), hw: (a.x - b.x).abs() / 2.0 + h.drill / 2.0, hh: (a.y - b.y).abs() / 2.0 + h.drill / 2.0 });
    }
    let silk = board.silk.iter().filter(|s| s.side == side && !s.refdes.is_empty()).map(|s| (s.refdes.clone(), s.a, s.b)).collect();
    Obstacles { tracks, boxes, silk }
}

/// The box of the part's pads and silk lines.
pub(crate) fn body_box(board: &Board, refdes: &str) -> Option<(Point, Point)> {
    let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
    let mut any = false;
    let mut add = |p: Point, mx: f64, my: f64| {
        lo = Point::new(lo.x.min(p.x - mx), lo.y.min(p.y - my));
        hi = Point::new(hi.x.max(p.x + mx), hi.y.max(p.y + my));
        any = true;
    };
    for p in board.pads.iter().filter(|p| p.refdes == refdes) {
        let (w, h) = p.shape.size();
        add(p.pos, w / 2.0, h / 2.0);
    }
    for s in board.silk.iter().filter(|s| s.refdes == refdes) {
        add(s.a, 0.0, 0.0);
        add(s.b, 0.0, 0.0);
    }
    any.then_some((lo, hi))
}

/// How badly a designator box at `r` sits: the number of things it touches (copper counts the most).
fn touches(board: &Board, ob: &Obstacles, r: Rect, refdes: &str, others: &[(String, Rect)]) -> u32 {
    let mut n = 0;
    for (a, b, hw) in &ob.tracks {
        if r.grown(*hw).hit_by_segment(*a, *b) {
            n += 3;
        }
    }
    n += 3 * ob.boxes.iter().filter(|b| b.overlaps(&r)).count() as u32;
    n += 2 * others.iter().filter(|(name, o)| name != refdes && o.grown(0.25).overlaps(&r)).count() as u32;
    // the outlines of the other parts
    n += ob.silk.iter().filter(|(name, a, b)| name != refdes && r.hit_by_segment(*a, *b)).count() as u32;
    // off the board: worse than any amount of copper (a text off the board is not printed at all)
    if board.outline.len() >= 3 && r.corners().iter().any(|c| !inside_polygon(&board.outline, *c)) {
        n += 20;
    }
    n
}

/// Move the designators that touch copper (or each other) to the clearest spot around their part. Returns how many were moved and
/// how many still touch something.
pub fn avoid_overlaps(board: &mut Board) -> LabelReport {
    let mut report = LabelReport::default();
    // every text as it is, for the "touches another text" count (kept up to date as texts move)
    let boxes_of = |board: &Board| -> Vec<(String, Rect)> {
        board.texts.iter().filter(|t| !t.refdes.is_empty()).map(|t| (t.refdes.clone(), Rect { c: t.pos, hw: t.size().0 / 2.0, hh: t.size().1 / 2.0 })).collect()
    };
    let refs: Vec<String> = board.texts.iter().filter(|t| !t.refdes.is_empty() && !board.label_offsets.contains_key(&t.refdes)).map(|t| t.refdes.clone()).collect();
    for refdes in refs {
        let Some(k) = board.texts.iter().position(|t| t.refdes == refdes) else { continue };
        let t = board.texts[k].clone();
        let Some((lo, hi)) = body_box(board, &refdes) else { continue };
        let ob = obstacles(board, t.side);
        let others = boxes_of(board);
        let (tw, th) = t.size();
        let (hw, hh) = (tw / 2.0, th / 2.0);
        let gap = 0.3;
        let (cx, cy) = ((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
        // where the text may go: where it is, then above and below at two distances, then beside the part
        let spots = [
            t.pos,
            Point::new(cx, hi.y + gap + hh + 0.2),
            Point::new(cx, lo.y - gap - hh - 0.2),
            Point::new(cx, hi.y + gap + hh + 0.2 + th * 1.1),
            Point::new(cx, lo.y - gap - hh - 0.2 - th * 1.1),
            Point::new(hi.x + gap + hw, cy),
            Point::new(lo.x - gap - hw, cy),
        ];
        let score = |p: Point| touches(board, &ob, Rect { c: p, hw, hh }, &refdes, &others);
        let here = score(t.pos);
        if here == 0 {
            continue;
        }
        // the first of the best (a tie keeps the spot that comes earlier in the list)
        let mut best = (here, t.pos);
        for p in spots.iter().skip(1) {
            let s = score(*p);
            if s < best.0 {
                best = (s, *p);
            }
        }
        if best.0 < here {
            let (dx, dy) = (best.1.x - t.pos.x, best.1.y - t.pos.y);
            board.texts[k].pos = best.1;
            board.label_offsets.insert(refdes.clone(), Point::new(dx, dy));
            report.moved += 1;
        }
        if best.0 > 0 {
            report.still_touching += 1;
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board_with_part() -> Board {
        let mut b = Board::new("t", 40.0, 30.0);
        b.set_part(Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(20.0, 15.0), rot: 0, side: Side::Top }).unwrap();
        b
    }

    #[test]
    fn a_designator_that_touches_nothing_stays_where_it_is() {
        let mut b = board_with_part();
        let before = b.texts[0].pos;
        assert_eq!(avoid_overlaps(&mut b), LabelReport::default());
        assert_eq!(b.texts[0].pos, before);
        assert!(b.label_offsets.is_empty());
    }

    #[test]
    fn a_track_across_the_designator_moves_it_to_a_clear_spot() {
        let mut b = board_with_part();
        let at = b.texts[0].pos;
        // a track right through the text
        b.tracks.push(Track { side: Side::Top, a: Point::new(at.x - 3.0, at.y), b: Point::new(at.x + 3.0, at.y), width: 0.25 });
        let r = avoid_overlaps(&mut b);
        assert_eq!((r.moved, r.still_touching), (1, 0), "{r:?}");
        let p = b.texts[0].pos;
        let (tw, th) = (crate::strokefont::text_width("R1", 1.0), 1.0);
        let rect = Rect { c: p, hw: tw / 2.0, hh: th / 2.0 };
        assert!(!rect.grown(0.125).hit_by_segment(Point::new(at.x - 3.0, at.y), Point::new(at.x + 3.0, at.y)), "{p:?}");
        // the spot is kept as the offset, so it follows the part
        assert!(b.label_offsets.contains_key("R1"));
    }

    #[test]
    fn a_designator_the_user_placed_is_left_alone() {
        let mut b = board_with_part();
        let at = b.texts[0].pos;
        b.tracks.push(Track { side: Side::Top, a: Point::new(at.x - 3.0, at.y), b: Point::new(at.x + 3.0, at.y), width: 0.25 });
        b.label_offsets.insert("R1".into(), Point::new(0.0, 0.0));
        assert_eq!(avoid_overlaps(&mut b), LabelReport::default());
        assert_eq!(b.texts[0].pos, at);
    }

    #[test]
    fn copper_of_the_other_side_does_not_count() {
        let mut b = board_with_part();
        let at = b.texts[0].pos;
        b.tracks.push(Track { side: Side::Bottom, a: Point::new(at.x - 3.0, at.y), b: Point::new(at.x + 3.0, at.y), width: 0.25 });
        assert_eq!(avoid_overlaps(&mut b), LabelReport::default());
    }
}
