//! Turning parts: which of the four ways round a part sits (0, 90, 180, 270 degrees) decides how long its airwires are and how many
//! of them cross. For every part that has no copper on its pads yet, the other three turns are tried with everything else where it
//! is, and the one with the shortest airwires and the fewest crossings is kept. A pin keeps its net when a part turns, so any turn
//! is the same circuit; only the place of the pads changes. A turn is not tried when the part would land on another part, on copper
//! that is there, or off the board.
//!
//! Parts the user locked, and parts with a track or a via on a pad, stay as they are.

use crate::geom::inside_polygon;
use crate::model::*;
use crate::ratsnest::{join_groups, ratsnest_in, Airwire};
use crate::swap::{cross_count, pad_has_copper};
use std::collections::{BTreeMap, BTreeSet};

/// A crossing of two airwires is worth this many mm of airwire (the same as in `swap`).
const CROSSING_COST: f64 = 3.0;
/// A turn must win at least this much (mm) to be taken: a part is not turned for nothing.
const MIN_GAIN: f64 = 0.5;
/// How many times the parts are gone through (a turn changes what the others gain from theirs).
const PASSES: usize = 2;

/// What `run` decided for one part.
#[derive(Clone, Debug, PartialEq)]
pub struct TurnPlan {
    pub refdes: String,
    pub from: u8,
    pub to: u8,
    /// Airwire length (mm) and crossings over the nets of the part, before and after.
    pub length: (f64, f64),
    pub crossings: (usize, usize),
}

/// What `run` did over the board.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrientReport {
    pub turned: Vec<TurnPlan>,
    /// Parts looked at (free to turn).
    pub tried: usize,
}

/// The airwires between pads, and the copper group of every pad.
fn pad_wires(board: &Board) -> (Vec<Airwire>, Vec<usize>) {
    let conn = crate::drc::connectivity(board);
    let groups: Vec<usize> = (0..board.pads.len()).map(|i| conn.pad_group(i)).collect();
    (ratsnest_in(board, &groups), groups)
}

fn free_to_turn(board: &Board, refdes: &str) -> bool {
    if board.locked_parts.iter().any(|r| r == refdes) {
        return false;
    }
    let pads: Vec<&Pad> = board.pads.iter().filter(|p| p.refdes == refdes).collect();
    !pads.is_empty() && pads.iter().all(|p| !pad_has_copper(board, p)) && pads.iter().any(|p| !p.net.is_empty())
}

fn rect_of(p: &Pad) -> (f64, f64, f64, f64) {
    let (w, h) = p.shape.size();
    (p.pos.x - w / 2.0, p.pos.y - h / 2.0, p.pos.x + w / 2.0, p.pos.y + h / 2.0)
}

/// Does the segment come within `d` of the rectangle?
fn seg_near_rect(a: Point, b: Point, r: (f64, f64, f64, f64), d: f64) -> bool {
    let (x0, y0, x1, y1) = (r.0 - d, r.1 - d, r.2 + d, r.3 + d);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    for (p, q) in [(-dx, a.x - x0), (dx, x1 - a.x), (-dy, a.y - y0), (dy, y1 - a.y)] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    t0 <= t1
}

type Boxes = BTreeMap<String, (Point, Point)>;

/// Can the part stand with these pads (and this body box): on the board, off the copper that is there, off the other parts it did
/// not already touch?
fn fits(board: &Board, refdes: &str, pads: &[Pad], bbox: Option<(Point, Point)>, overlapping_now: &BTreeSet<String>, boxes: &Boxes) -> bool {
    let rules = &board.rules;
    for p in pads {
        let r = rect_of(p);
        if board.outline.len() >= 3 && [(r.0, r.1), (r.2, r.1), (r.2, r.3), (r.0, r.3)].iter().any(|(x, y)| !inside_polygon(&board.outline, Point::new(*x, *y))) {
            return false;
        }
        for q in board.pads.iter().filter(|q| q.refdes != refdes && (q.drill.is_some() || p.drill.is_some() || q.side == p.side)) {
            let s = rect_of(q);
            let gap = 0.15f64.max(rules.min_clearance);
            if r.0 < s.2 + gap && s.0 < r.2 + gap && r.1 < s.3 + gap && s.1 < r.3 + gap {
                return false;
            }
        }
        for t in board.tracks.iter().filter(|t| p.drill.is_some() || t.side == p.side) {
            if seg_near_rect(t.a, t.b, r, rules.min_clearance + t.width / 2.0) {
                return false;
            }
        }
        for v in &board.vias {
            let (cx, cy) = (v.pos.x.clamp(r.0, r.2), v.pos.y.clamp(r.1, r.3));
            if (v.pos.x - cx).hypot(v.pos.y - cy) < v.diameter / 2.0 + rules.min_clearance {
                return false;
            }
        }
        for h in &board.holes {
            let (cx, cy) = (h.pos.x.clamp(r.0, r.2), h.pos.y.clamp(r.1, r.3));
            if (h.pos.x - cx).hypot(h.pos.y - cy) < h.drill / 2.0 + rules.min_clearance {
                return false;
            }
        }
    }
    // the body must not run into a part it did not already touch
    if let Some((lo, hi)) = bbox {
        for (r, (a, b)) in boxes.iter().filter(|(r, _)| r.as_str() != refdes) {
            if lo.x < b.x && a.x < hi.x && lo.y < b.y && a.y < hi.y && !overlapping_now.contains(r) {
                return false;
            }
        }
    }
    true
}

/// The turn of `refdes` that wins most, if any wins (nothing is changed).
fn plan_part(board: &Board, refdes: &str, wires_all: &[Airwire], groups: &[usize]) -> Option<TurnPlan> {
    let part = board.parts.iter().find(|p| p.refdes == refdes)?.clone();
    let mine: BTreeSet<usize> = (0..board.pads.len()).filter(|&i| board.pads[i].refdes == refdes).collect();
    let nets: BTreeSet<&str> = mine.iter().map(|&i| board.pads[i].net.as_str()).filter(|n| !n.is_empty()).collect();
    if nets.is_empty() {
        return None;
    }
    let others: Vec<Airwire> = wires_all.iter().filter(|w| !nets.contains(w.net.as_str())).cloned().collect();
    // the points of the touched nets that this part does not decide
    let mut fixed: BTreeMap<&str, Vec<(Point, usize)>> = BTreeMap::new();
    for (i, p) in board.pads.iter().enumerate() {
        if !mine.contains(&i) && nets.contains(p.net.as_str()) {
            fixed.entry(p.net.as_str()).or_default().push((p.pos, groups[i]));
        }
    }
    let boxes: Boxes = board.parts.iter().filter_map(|q| crate::labels::body_box(board, &q.refdes).map(|b| (q.refdes.clone(), b))).collect();
    let overlapping_now: BTreeSet<String> = match boxes.get(refdes) {
        Some((lo, hi)) => boxes.iter().filter(|(r, (a, b))| r.as_str() != refdes && lo.x < b.x && a.x < hi.x && lo.y < b.y && a.y < hi.y).map(|(r, _)| r.clone()).collect(),
        None => BTreeSet::new(),
    };
    let cost = |pads: &[Pad]| -> (f64, usize) {
        let mut by_net = fixed.clone();
        let mut g = 2_000_000;
        for p in pads.iter().filter(|p| !p.net.is_empty()) {
            by_net.entry(p.net.as_str()).or_default().push((p.pos, g));
            g += 1;
        }
        let wires: Vec<Airwire> = by_net.iter().flat_map(|(n, pts)| join_groups(n, pts)).collect();
        (wires.iter().map(|w| w.a.dist(w.b)).sum(), cross_count(&wires, &others))
    };
    let score = |c: (f64, usize)| c.0 + CROSSING_COST * c.1 as f64;
    let now = cost(&mine.iter().map(|&i| board.pads[i].clone()).collect::<Vec<_>>());
    let mut trial = board.clone();
    let mut best: Option<(u8, (f64, usize))> = None;
    for rot in (0..4u8).filter(|r| *r != part.rot) {
        trial.place_part(refdes, part.pos, rot);
        let pads: Vec<Pad> = trial.pads.iter().filter(|p| p.refdes == refdes).cloned().collect();
        let bbox = crate::labels::body_box(&trial, refdes);
        if !fits(board, refdes, &pads, bbox, &overlapping_now, &boxes) {
            continue;
        }
        let c = cost(&pads);
        if score(c) < score(best.map_or(now, |b| b.1)) - 1e-9 {
            best = Some((rot, c));
        }
    }
    let (rot, c) = best?;
    (score(c) < score(now) - MIN_GAIN).then_some(TurnPlan { refdes: refdes.to_string(), from: part.rot, to: rot, length: (now.0, c.0), crossings: (now.1, c.1) })
}

/// Turn the parts that gain from it (those in `only`, or all that are free; only parts with at least `min_pads` pads). `dry`: only report.
pub fn run(board: &mut Board, only: Option<&[String]>, min_pads: usize, dry: bool) -> OrientReport {
    let mut rep = OrientReport::default();
    let mut work = board.clone();
    for _ in 0..PASSES {
        let refs: Vec<String> = work.parts.iter().map(|p| p.refdes.clone()).filter(|r| only.is_none_or(|o| o.contains(r)) && work.pads.iter().filter(|p| &p.refdes == r).count() >= min_pads && free_to_turn(&work, r)).collect();
        rep.tried = rep.tried.max(refs.len());
        let mut changed = false;
        let (mut wires, mut groups) = pad_wires(&work);
        for r in refs {
            let Some(plan) = plan_part(&work, &r, &wires, &groups) else { continue };
            let pos = work.parts.iter().find(|p| p.refdes == r).map_or(Point::default(), |p| p.pos);
            work.place_part(&r, pos, plan.to);
            // the airwires are worked out again: this turn changes what the next part gains
            (wires, groups) = pad_wires(&work);
            match rep.turned.iter_mut().find(|t| t.refdes == r) {
                Some(t) => {
                    t.to = plan.to;
                    t.length.1 = plan.length.1;
                    t.crossings.1 = plan.crossings.1;
                }
                None => rep.turned.push(plan),
            }
            changed = true;
        }
        if !changed {
            break;
        }
    }
    // a part that went round and came back is no change
    rep.turned.retain(|t| t.from != t.to);
    if !dry {
        for t in &rep.turned {
            if let Some(pos) = board.parts.iter().find(|p| p.refdes == t.refdes).map(|p| p.pos) {
                board.place_part(&t.refdes, pos, t.to);
            }
        }
    }
    rep
}

#[cfg(test)]
mod tests {
    use super::*;

    /// U1 (two pads side by side: A on the left, B on the right) and two resistors that want A above and B below.
    fn board() -> Board {
        let mut b = Board::new("t", 60.0, 40.0);
        for (r, x, y, na, nb) in [("U1", 20.0, 20.0, "A", "B"), ("R1", 45.0, 12.0, "A", "X"), ("R2", 45.0, 28.0, "B", "Y")] {
            b.set_part(Part { refdes: r.into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(x, y), rot: 0, side: Side::Top }).unwrap();
            for p in b.pads.iter_mut().filter(|p| p.refdes == r) {
                p.net = if p.pin == "1" { na.into() } else { nb.into() };
            }
        }
        b
    }

    #[test]
    fn a_part_turns_when_that_shortens_and_uncrosses_its_airwires() {
        let mut b = board();
        let before: Vec<Pad> = b.pads.iter().filter(|p| p.refdes == "U1").cloned().collect();
        assert!(before[0].pos.x < before[1].pos.x);
        let r = run(&mut b, Some(&["U1".to_string()]), 0, false);
        let u1 = r.turned.iter().find(|t| t.refdes == "U1").unwrap_or_else(|| panic!("{r:?}"));
        assert!(u1.length.1 < u1.length.0 && u1.crossings.1 <= u1.crossings.0, "{u1:?}");
        assert_ne!(b.parts.iter().find(|p| p.refdes == "U1").unwrap().rot, 0);
        // every pin keeps its net
        for p in b.pads.iter().filter(|p| p.refdes == "U1") {
            assert_eq!(p.net, if p.pin == "1" { "A" } else { "B" });
        }
        // a second run finds nothing more
        assert!(run(&mut b, Some(&["U1".to_string()]), 0, false).turned.is_empty());
    }

    #[test]
    fn a_dry_run_changes_nothing_and_locked_or_wired_parts_stay() {
        let mut b = board();
        let rot = |b: &Board| b.parts.iter().map(|p| p.rot).collect::<Vec<_>>();
        let before = rot(&b);
        assert!(!run(&mut b, None, 0, true).turned.is_empty());
        assert_eq!(rot(&b), before);
        b.locked_parts.push("U1".into());
        assert!(run(&mut b, None, 0, false).turned.iter().all(|t| t.refdes != "U1"));
        b.locked_parts.clear();
        let pad = b.pads.iter().find(|p| p.refdes == "U1" && p.pin == "1").unwrap().pos;
        b.tracks.push(Track { side: Side::Top, a: pad, b: Point::new(pad.x - 3.0, pad.y), width: 0.25 });
        assert!(run(&mut b, None, 0, false).turned.iter().all(|t| t.refdes != "U1"));
    }

    #[test]
    fn a_turn_that_would_land_on_another_part_is_not_taken() {
        let mut b = board();
        // parts right where U1's pads would go when it turns a quarter
        b.set_part(Part { refdes: "R3".into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(20.0, 22.0), rot: 1, side: Side::Top }).unwrap();
        b.set_part(Part { refdes: "R4".into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(20.0, 18.0), rot: 1, side: Side::Top }).unwrap();
        assert!(run(&mut b, Some(&["U1".to_string()]), 0, false).turned.is_empty());
    }
}
