//! Changes made by hand to a stripboard / perfboard layout (B5): a part or a block of the board moved by whole holes, a part turned,
//! wire links added, moved and taken away, strips cut and joined again; and a saved layout kept in step with a schematic that changed
//! (the parts taken out go, the new ones are added at the right, the rest stays where the person put it).
//!
//! Every change gives a new layout; whether it still joins what the schematic joins is `strip_check`'s to say, run again after each.

use crate::stripboard::{strip_layout_with, At, Link, StripLayout, StripOptions, StripPart};
use crate::{extract, Schematic};
use std::collections::BTreeMap;

/// The hole `d` rows and `c` columns from `at`; None off the top or the left. The board grows to the right and down.
fn shift(at: At, dr: i32, dc: i32) -> Option<At> {
    let (r, c) = (at.0 as i32 + dr, at.1 as i32 + dc);
    (r >= 0 && c >= 0 && r < u16::MAX as i32 && c < u16::MAX as i32).then_some((r as u16, c as u16))
}

/// The board made big enough for every hole in use (and the cuts). A stripboard never gets smaller than it was (its strips run the
/// whole width); a perfboard fits what is on it, with one free row and column past it as the automatic layout leaves, so it shrinks
/// again when parts move back (the sections' boxes count too).
fn grown(mut l: StripLayout) -> StripLayout {
    let mut far = l.used().into_iter().map(|(a, _)| a).chain(l.cuts.iter().copied()).chain(l.bare.iter().flat_map(|(a, b)| [*a, *b])).fold((0u16, 0u16), |m, a| (m.0.max(a.0), m.1.max(a.1)));
    if l.perf {
        // (a section's box already has its hole of room past its parts: the board ends right after it)
        let boxes = l.sections.iter().fold((0u16, 0u16), |m, (_, _, b)| (m.0.max(b.0 + 1), m.1.max(b.1 + 1)));
        (l.rows, l.cols) = ((far.0 + 2).max(boxes.0), (far.1 + 2).max(boxes.1));
        return l;
    }
    far = (far.0 + 1, far.1 + 1);
    l.rows = l.rows.max(far.0);
    l.cols = l.cols.max(far.1);
    l
}

/// The sections' boxes after a hand edit (`before` → `after`) follow their parts: a section whose parts all moved by the same rows
/// and columns has its box moved with them; one whose parts moved otherwise gets a box round its parts' leads, a hole of room on each
/// side. A section whose parts did not move keeps its box. The parts of a section are those of the requirements table's group
/// (`stripboard::sections`).
pub fn sections_followed(s: &Schematic, before: &StripLayout, after: StripLayout) -> StripLayout {
    if after.sections.is_empty() {
        return after;
    }
    let c = crate::breadboard::read(s);
    let groups = crate::stripboard::sections(s, &c);
    let leads = |l: &StripLayout, r: &str| -> Vec<At> { l.parts.iter().find(|p| p.refdes == r).map(|p| p.leads.iter().map(|x| x.1).collect()).unwrap_or_default() };
    let mut out = after.clone();
    for (name, a, b) in out.sections.iter_mut() {
        let Some((_, members)) = groups.iter().find(|(n, _)| n == name) else { continue };
        let moves: Vec<(Vec<At>, Vec<At>)> = members.iter().map(|r| (leads(before, r), leads(&after, r))).filter(|(x, y)| !x.is_empty() && x.len() == y.len()).collect();
        if moves.iter().all(|(x, y)| x == y) {
            continue;
        }
        let offsets: std::collections::BTreeSet<(i32, i32)> = moves.iter().map(|(x, y)| (y[0].0 as i32 - x[0].0 as i32, y[0].1 as i32 - x[0].1 as i32)).collect();
        let rigid = moves.iter().all(|(x, y)| x.iter().zip(y).all(|(p, q)| (q.0 as i32 - p.0 as i32, q.1 as i32 - p.1 as i32) == (y[0].0 as i32 - x[0].0 as i32, y[0].1 as i32 - x[0].1 as i32)));
        if offsets.len() == 1 && rigid {
            let (dr, dc) = *offsets.iter().next().unwrap();
            if let (Some(na), Some(nb)) = (shift(*a, dr, dc), shift(*b, dr, dc)) {
                (*a, *b) = (na, nb);
                continue;
            }
        }
        let pts: Vec<At> = moves.iter().flat_map(|(_, y)| y.iter().copied()).collect();
        let (r0, r1) = (pts.iter().map(|p| p.0).min().unwrap_or(0), pts.iter().map(|p| p.0).max().unwrap_or(0));
        let (c0, c1) = (pts.iter().map(|p| p.1).min().unwrap_or(0), pts.iter().map(|p| p.1).max().unwrap_or(0));
        (*a, *b) = ((r0.saturating_sub(1), c0.saturating_sub(1)), (r1 + 1, c1 + 1));
    }
    grown(out)
}

/// The layout without the links and the bare wires drawn by hand that lie wholly in one of the boxes `areas` (corners, inclusive);
/// None when there is none.
pub fn wires_in_boxes_removed(l: &StripLayout, areas: &[(At, At)]) -> Option<StripLayout> {
    let inside = |h: At| areas.iter().any(|(a, b)| h.0 >= a.0.min(b.0) && h.0 <= a.0.max(b.0) && h.1 >= a.1.min(b.1) && h.1 <= a.1.max(b.1));
    let mut out = l.clone();
    out.links.retain(|k| !(inside(k.a) && inside(k.b)));
    out.bare.retain(|(a, b)| !(inside(*a) && inside(*b)));
    (out.links.len() + out.bare.len() < l.links.len() + l.bare.len()).then(|| grown(out))
}

/// The bare wire drawn by hand nearest point `p` (in holes, rows and columns as fractions), within `reach` holes of it.
pub fn bare_near(l: &StripLayout, p: (f32, f32), reach: f32) -> Option<usize> {
    let dist = |(a, b): &(At, At)| {
        let (ar, ac, br, bc) = (a.0 as f32, a.1 as f32, b.0 as f32, b.1 as f32);
        let (vr, vc) = (br - ar, bc - ac);
        let t = (((p.0 - ar) * vr + (p.1 - ac) * vc) / (vr * vr + vc * vc).max(1e-6)).clamp(0.0, 1.0);
        (p.0 - (ar + vr * t)).hypot(p.1 - (ac + vc * t))
    };
    l.bare.iter().enumerate().map(|(k, w)| (k, dist(w))).filter(|(_, d)| *d <= reach).min_by(|x, y| x.1.total_cmp(&y.1)).map(|(k, _)| k)
}

/// The layout without bare wire `k` drawn by hand.
pub fn without_bare(l: &StripLayout, k: usize) -> StripLayout {
    let mut out = l.clone();
    if k < out.bare.len() {
        out.bare.remove(k);
    }
    grown(out)
}

/// Part `refdes` moved `dr` rows and `dc` columns; None when it is not there or would leave the board at the top or the left.
pub fn part_moved(l: &StripLayout, refdes: &str, dr: i32, dc: i32) -> Option<StripLayout> {
    let mut out = l.clone();
    let p = out.parts.iter_mut().find(|p| p.refdes == refdes)?;
    for (_, at) in &mut p.leads {
        *at = shift(*at, dr, dc)?;
    }
    Some(grown(out))
}

/// Everything within the rows and columns from `a` to `b` (both corners in): the parts whose leads are all inside, the link ends,
/// the cuts and the wires from off the board, moved `dr` rows and `dc` columns. A link with one end outside stretches. None when
/// something would leave the board at the top or the left.
pub fn area_moved(l: &StripLayout, a: At, b: At, dr: i32, dc: i32) -> Option<StripLayout> {
    let inside = |at: At| at.0 >= a.0.min(b.0) && at.0 <= a.0.max(b.0) && at.1 >= a.1.min(b.1) && at.1 <= a.1.max(b.1);
    region_moved(l, &inside, dr, dc)
}

/// The parts `refdes` moved `dr` rows and `dc` columns as one, with what lies among them (`Picked`): the links, cuts, bare wires and
/// off-board wires within a hole of the box round their leads, or in one of the boxes `areas` drawn to pick them, go too (a link with
/// one end outside stretches); the parts not picked stay, even inside those boxes, and so do the link ends on their leads. None when
/// nothing is picked or something would leave the board at the top or the left.
pub fn picked_moved(l: &StripLayout, refdes: &[String], areas: &[(At, At)], dr: i32, dc: i32) -> Option<StripLayout> {
    let p = Picked::of(l, refdes, areas)?;
    region_moved(l, &|at| p.inside(at), dr, dc)
}

/// The parts `refdes` turned as one about the box round their leads, with what lies among them (`Picked`), as `area_turned` turns a
/// box: half a turn on a stripboard, a quarter turn clockwise on a perfboard. None when nothing is picked.
pub fn picked_turned(l: &StripLayout, refdes: &[String], areas: &[(At, At)]) -> Option<StripLayout> {
    let p = Picked::of(l, refdes, areas)?;
    Some(region_turned(l, p.frame, &|at| p.inside(at)))
}

/// The parts with a lead in the rows and columns from `a` to `b`, or whose leads' box crosses it (a box drawn "touching": a chip whose
/// pins are on both sides of it).
pub fn parts_touching_box(l: &StripLayout, a: At, b: At) -> Vec<String> {
    let (r0, r1, c0, c1) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
    l.parts
        .iter()
        .filter(|p| !p.leads.is_empty())
        .filter(|p| {
            let (pr0, pr1, pc0, pc1) = part_box(p);
            pr0 <= r1 && pr1 >= r0 && pc0 <= c1 && pc1 >= c0
        })
        .map(|p| p.refdes.clone())
        .collect()
}

/// What a pick of parts carries along: the box round their leads and one hole more, and the boxes drawn to pick them, less the holes of
/// the leads of the parts not picked. `frame` is the box round all of it (what a turn turns about).
struct Picked {
    frame: (u16, u16, u16, u16),
    boxes: Vec<(u16, u16, u16, u16)>,
    others: std::collections::BTreeSet<At>,
}

impl Picked {
    fn of(l: &StripLayout, refdes: &[String], areas: &[(At, At)]) -> Option<Picked> {
        let picked: Vec<&StripPart> = l.parts.iter().filter(|p| refdes.contains(&p.refdes) && !p.leads.is_empty()).collect();
        let first = part_box(picked.first()?);
        let (r0, r1, c0, c1) = picked.iter().map(|p| part_box(p)).fold(first, |f, b| (f.0.min(b.0), f.1.max(b.1), f.2.min(b.2), f.3.max(b.3)));
        // one hole round the leads: the cuts beside a chip's end pins and the short links off its pins belong to it
        let mut boxes = vec![(r0.saturating_sub(1), r1 + 1, c0.saturating_sub(1), c1 + 1)];
        boxes.extend(areas.iter().map(|(a, b)| (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1))));
        let frame = boxes.iter().fold(boxes[0], |f, b| (f.0.min(b.0), f.1.max(b.1), f.2.min(b.2), f.3.max(b.3)));
        let others = l.parts.iter().filter(|p| !refdes.contains(&p.refdes)).flat_map(|p| p.leads.iter().map(|x| x.1)).collect();
        Some(Picked { frame, boxes, others })
    }

    fn inside(&self, at: At) -> bool {
        self.boxes.iter().any(|&(r0, r1, c0, c1)| at.0 >= r0 && at.0 <= r1 && at.1 >= c0 && at.1 <= c1) && !self.others.contains(&at)
    }
}

/// Everything on the holes `inside` says moved `dr` rows and `dc` columns (`area_moved` with any shape of region).
fn region_moved(l: &StripLayout, inside: &dyn Fn(At) -> bool, dr: i32, dc: i32) -> Option<StripLayout> {
    let mut out = l.clone();
    for p in &mut out.parts {
        if p.leads.iter().all(|(_, at)| inside(*at)) {
            for (_, at) in &mut p.leads {
                *at = shift(*at, dr, dc)?;
            }
        }
    }
    for k in &mut out.links {
        for end in [&mut k.a, &mut k.b] {
            if inside(*end) {
                *end = shift(*end, dr, dc)?;
            }
        }
    }
    for (_, pins) in &mut out.off {
        for (_, _, at) in pins {
            if inside(*at) {
                *at = shift(*at, dr, dc)?;
            }
        }
    }
    for w in &mut out.bare {
        let ends = (if inside(w.0) { shift(w.0, dr, dc)? } else { w.0 }, if inside(w.1) { shift(w.1, dr, dc)? } else { w.1 });
        // a bare wire keeps its kind (straight, or at 45 degrees): with one end inside and a move that would change it, it stays
        if same_way(*w, ends) {
            *w = ends;
        }
    }
    let mut cuts = std::collections::BTreeSet::new();
    for c in &l.cuts {
        cuts.insert(if inside(*c) { shift(*c, dr, dc)? } else { *c });
    }
    out.cuts = cuts;
    Some(grown(out))
}

/// Everything within the rows and columns from `a` to `b` (both corners in) turned half a turn about the box's middle: the parts
/// whose leads are all inside, the link ends (a link with one end outside stretches), the wires from off the board, the cuts, and
/// the bare wires (one with an end outside stays where it was unless it stays straight). Half a turn keeps the strips along the rows,
/// so what was joined along a strip stays joined; a quarter turn would stand a chip's pins across them. On a perfboard (no strips)
/// a quarter turn clockwise (`turned_box` is where the box then is).
pub fn area_turned(l: &StripLayout, a: At, b: At) -> StripLayout {
    let frame = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
    let (r0, r1, c0, c1) = frame;
    region_turned(l, frame, &|at: At| at.0 >= r0 && at.0 <= r1 && at.1 >= c0 && at.1 <= c1)
}

/// Everything on the holes `inside` says turned about the box `frame` (first row, last row, first column, last column), as
/// `area_turned` turns a box.
fn region_turned(l: &StripLayout, frame: (u16, u16, u16, u16), inside: &dyn Fn(At) -> bool) -> StripLayout {
    let (r0, r1, c0, c1) = frame;
    // a perfboard turns a quarter turn clockwise about the box's top-left corner (the box then is as tall as it was wide)
    let turn = |at: At| match (inside(at), l.perf) {
        (false, _) => at,
        (true, false) => (r0 + r1 - at.0, c0 + c1 - at.1),
        (true, true) => (r0 + (at.1 - c0), c0 + (r1 - at.0)),
    };
    let mut out = l.clone();
    for p in &mut out.parts {
        if p.leads.iter().all(|(_, at)| inside(*at)) {
            for (_, at) in &mut p.leads {
                *at = turn(*at);
            }
        }
    }
    for k in &mut out.links {
        k.a = turn(k.a);
        k.b = turn(k.b);
    }
    for (_, pins) in &mut out.off {
        for (_, _, at) in pins {
            *at = turn(*at);
        }
    }
    out.cuts = l.cuts.iter().map(|c| turn(*c)).collect();
    for w in &mut out.bare {
        let ends = (turn(w.0), turn(w.1));
        if same_way(*w, ends) {
            *w = ends;
        }
    }
    out.names = l.names.iter().map(|(at, n)| (turn(*at), n.clone())).collect();
    grown(out)
}

/// Where a box is after `area_turned`: the same on a stripboard, as tall as it was wide on a perfboard.
pub fn turned_box(l: &StripLayout, a: At, b: At) -> (At, At) {
    let (r0, r1, c0, c1) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
    if l.perf {
        ((r0, c0), (r0 + (c1 - c0), c0 + (r1 - r0)))
    } else {
        (a, b)
    }
}

/// Part `refdes` turned: a two-lead part a quarter turn about its first lead (standing ↔ lying), a chip or a row of leads half a
/// turn about its middle (pin 1 to the other end), or on a perfboard (no strips) a quarter turn clockwise. None when it is not there
/// or would leave the board.
pub fn part_turned(l: &StripLayout, refdes: &str) -> Option<StripLayout> {
    let mut out = l.clone();
    let p = out.parts.iter_mut().find(|p| p.refdes == refdes)?;
    if p.leads.len() == 2 {
        let (a, b) = (p.leads[0].1, p.leads[1].1);
        let (dr, dc) = (b.0 as i32 - a.0 as i32, b.1 as i32 - a.1 as i32);
        // a quarter turn: (dr, dc) -> (dc, -dr)
        p.leads[1].1 = shift(a, dc, -dr)?;
    } else {
        let (r0, r1) = (p.leads.iter().map(|(_, a)| a.0).min()?, p.leads.iter().map(|(_, a)| a.0).max()?);
        let (c0, c1) = (p.leads.iter().map(|(_, a)| a.1).min()?, p.leads.iter().map(|(_, a)| a.1).max()?);
        for (_, at) in &mut p.leads {
            // a perfboard has no strips to keep along: a quarter turn clockwise about the top-left corner
            *at = if l.perf { (r0 + (at.1 - c0), c0 + (r1 - at.0)) } else { (r0 + r1 - at.0, c0 + c1 - at.1) };
        }
    }
    Some(grown(out))
}

/// How `parts_aligned` lines the parts up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripAlign {
    /// Left ends on the leftmost one's.
    Left,
    /// Right ends on the rightmost one's.
    Right,
    /// Top ends on the uppermost one's.
    Top,
    /// Bottom ends on the lowest one's.
    Bottom,
    /// In one row from the leftmost one's place, in the order they stand, `gap` holes between, tops lined up.
    Row,
    /// In one column from the uppermost one's place, in the order they stand, `gap` holes between, left ends lined up.
    Column,
    /// In one row (or column) in the order of the leads they are wired to on the other parts, its middle across from theirs: under a
    /// driver chip, its relays in the order of its outputs. A row (at the uppermost one's row) when those leads lie along a row, else
    /// a column (at the leftmost one's column).
    NetOrder,
}

/// The parts whose leads all lie within the rows and columns from `a` to `b` (both corners in), as `area_moved` takes them.
pub fn parts_in_box(l: &StripLayout, a: At, b: At) -> Vec<String> {
    let inside = |at: At| at.0 >= a.0.min(b.0) && at.0 <= a.0.max(b.0) && at.1 >= a.1.min(b.1) && at.1 <= a.1.max(b.1);
    l.parts.iter().filter(|p| !p.leads.is_empty() && p.leads.iter().all(|(_, at)| inside(*at))).map(|p| p.refdes.clone()).collect()
}

/// A part's corners: (first row, last row, first column, last column) of its leads.
fn part_box(p: &StripPart) -> (u16, u16, u16, u16) {
    let rows = p.leads.iter().map(|(_, a)| a.0);
    let cols = p.leads.iter().map(|(_, a)| a.1);
    (rows.clone().min().unwrap_or(0), rows.max().unwrap_or(0), cols.clone().min().unwrap_or(0), cols.max().unwrap_or(0))
}

/// The parts `refdes` moved as `how` says (whole parts, by whole holes; the wires stay where they are, for "固定した部品のまわりを
/// 並べ直す" to draw again). On a perfboard the parts moved are locked where they go, and only the locked parts are in the way (the
/// rest is laid out again round them). An error that says what is in the way, or what to pick, when nothing can move: fewer than two
/// parts, a lead onto another part's or an off-board wire's hole, a chip onto another part.
pub fn parts_aligned(s: &Schematic, l: &StripLayout, refdes: &[String], how: StripAlign, gap: u16) -> Result<StripLayout, String> {
    let picked: Vec<&StripPart> = l.parts.iter().filter(|p| refdes.contains(&p.refdes) && !p.leads.is_empty()).collect();
    if picked.len() < 2 {
        return Err("揃える部品を 2 つ以上、四角で囲んで選んでください".to_string());
    }
    let boxes: Vec<(u16, u16, u16, u16)> = picked.iter().map(|p| part_box(p)).collect();
    let (top, bottom) = (boxes.iter().map(|b| b.0).min().unwrap_or(0), boxes.iter().map(|b| b.1).max().unwrap_or(0));
    let (left, right) = (boxes.iter().map(|b| b.2).min().unwrap_or(0), boxes.iter().map(|b| b.3).max().unwrap_or(0));
    // one after another along a row or down a column, in the order given, from `from` (else the first part's place)
    let line = |order: Vec<usize>, across: bool, from: Option<i32>| -> Vec<(i32, i32)> {
        let mut moves = vec![(0, 0); picked.len()];
        let mut at = from.unwrap_or(if across { left } else { top } as i32).max(0);
        for i in order {
            let b = boxes[i];
            if across {
                moves[i] = (top as i32 - b.0 as i32, at - b.2 as i32);
                at += (b.3 - b.2) as i32 + 1 + gap as i32;
            } else {
                moves[i] = (at - b.0 as i32, left as i32 - b.2 as i32);
                at += (b.1 - b.0) as i32 + 1 + gap as i32;
            }
        }
        moves
    };
    let by = |key: &dyn Fn(usize) -> (f64, f64)| -> Vec<usize> {
        let mut v: Vec<usize> = (0..picked.len()).collect();
        v.sort_by(|a, b| key(*a).partial_cmp(&key(*b)).unwrap_or(std::cmp::Ordering::Equal));
        v
    };
    let moves: Vec<(i32, i32)> = match how {
        StripAlign::Left => boxes.iter().map(|b| (0, left as i32 - b.2 as i32)).collect(),
        StripAlign::Right => boxes.iter().map(|b| (0, right as i32 - b.3 as i32)).collect(),
        StripAlign::Top => boxes.iter().map(|b| (top as i32 - b.0 as i32, 0)).collect(),
        StripAlign::Bottom => boxes.iter().map(|b| (bottom as i32 - b.1 as i32, 0)).collect(),
        StripAlign::Row => line(by(&|i| (boxes[i].2 as f64, boxes[i].0 as f64)), true, None),
        StripAlign::Column => line(by(&|i| (boxes[i].0 as f64, boxes[i].2 as f64)), false, None),
        StripAlign::NetOrder => {
            // each part's place: the middle of the leads it is wired to on the chips and headers not picked (else on any part not
            // picked: a resistor's other lead goes to its LED, which says nothing of the order), by nets of few leads only (a supply
            // or ground runs everywhere and tells nothing); its own middle when there are none
            let nets = pin_nets(s);
            let mut at_net: [BTreeMap<&String, Vec<At>>; 2] = [BTreeMap::new(), BTreeMap::new()];
            for p in l.parts.iter().filter(|p| !refdes.contains(&p.refdes)) {
                for (pin, a) in &p.leads {
                    if let Some(n) = nets.get(&(p.refdes.clone(), pin.clone())) {
                        at_net[0].entry(n).or_default().push(*a);
                        if p.leads.len() > 2 {
                            at_net[1].entry(n).or_default().push(*a);
                        }
                    }
                }
            }
            let few = |n: &String| !crate::breadboard::is_ground_name(n) && nets.values().filter(|m| *m == n).count() <= 4;
            let key: Vec<(f64, f64)> = picked
                .iter()
                .zip(&boxes)
                .map(|(p, b)| {
                    let mine: Vec<&String> = p.leads.iter().filter_map(|(pin, _)| nets.get(&(p.refdes.clone(), pin.clone()))).filter(|n| few(n)).collect();
                    let leads_on = |m: &BTreeMap<&String, Vec<At>>| mine.iter().filter_map(|n| m.get(n)).flatten().copied().collect::<Vec<At>>();
                    let chips = leads_on(&at_net[1]);
                    let to = if chips.is_empty() { leads_on(&at_net[0]) } else { chips };
                    if to.is_empty() {
                        ((b.0 + b.1) as f64 / 2.0, (b.2 + b.3) as f64 / 2.0)
                    } else {
                        (to.iter().map(|a| a.0 as f64).sum::<f64>() / to.len() as f64, to.iter().map(|a| a.1 as f64).sum::<f64>() / to.len() as f64)
                    }
                })
                .collect();
            let spread = |f: fn(&(f64, f64)) -> f64| key.iter().map(f).fold(f64::MIN, f64::max) - key.iter().map(f).fold(f64::MAX, f64::min);
            let across = spread(|k| k.1) >= spread(|k| k.0);
            // the line's middle under (or beside) the middle of the leads they are wired to
            let long: i32 = boxes.iter().map(|b| if across { (b.3 - b.2) as i32 + 1 } else { (b.1 - b.0) as i32 + 1 }).sum::<i32>() + gap as i32 * (picked.len() as i32 - 1);
            let mid = key.iter().map(|k| if across { k.1 } else { k.0 }).sum::<f64>() / key.len() as f64;
            // (kept within the board where it fits: the line pushed back from the right or bottom edge)
            let room = if across { l.cols } else { l.rows } as i32;
            let start = ((mid - long as f64 / 2.0).round() as i32).min(room - long).max(0);
            line(by(&|i| if across { (key[i].1, key[i].0) } else { (key[i].0, key[i].1) }), across, Some(start))
        }
    };
    let mut out = l.clone();
    for (p, (dr, dc)) in picked.iter().zip(&moves) {
        out = part_moved(&out, &p.refdes, *dr, *dc).ok_or_else(|| format!("{} が基板の上か左の端の外に出ます", p.refdes))?;
    }
    // nothing in the way: a lead on a hole another part's lead or an off-board wire has, a chip over another part. On a perfboard
    // only the locked parts count: the others are laid out again round the parts lined up (`perfboard::perf_layout_around`)
    let others: Vec<&StripPart> = out.parts.iter().filter(|p| !refdes.contains(&p.refdes) && (!out.perf || out.locked.contains(&p.refdes))).collect();
    let moved: Vec<&StripPart> = out.parts.iter().filter(|p| refdes.contains(&p.refdes)).collect();
    let within = |a: At, b: (u16, u16, u16, u16)| a.0 >= b.0 && a.0 <= b.1 && a.1 >= b.2 && a.1 <= b.3;
    let overlap = |a: (u16, u16, u16, u16), b: (u16, u16, u16, u16)| a.0 <= b.1 && b.0 <= a.1 && a.2 <= b.3 && b.2 <= a.3;
    let off_holes: Vec<At> = out.off.iter().flat_map(|(_, w)| w.iter().map(|x| x.2)).collect();
    for (i, p) in moved.iter().enumerate() {
        let pb = part_box(p);
        for q in others.iter().copied().chain(moved.iter().copied().skip(i + 1)) {
            let qb = part_box(q);
            let lead_on = p.leads.iter().any(|(_, a)| q.leads.iter().any(|(_, b)| a == b));
            let body_on = (p.leads.len() > 2 && q.leads.iter().any(|(_, b)| within(*b, pb)))
                || (q.leads.len() > 2 && p.leads.iter().any(|(_, a)| within(*a, qb)))
                || (p.leads.len() > 2 && q.leads.len() > 2 && overlap(pb, qb));
            if lead_on || body_on {
                return Err(format!("{} を動かす先に {} があります。先に {} をどけるか、揃える部品を選び直してください", p.refdes, q.refdes, q.refdes));
            }
        }
        if p.leads.iter().any(|(_, a)| off_holes.contains(a)) {
            return Err(format!("{} を動かす先に、基板の外からの線の穴があります", p.refdes));
        }
    }
    if out.perf {
        out.locked.extend(picked.iter().map(|p| p.refdes.clone()));
    }
    Ok(out)
}

/// The schematic net of each part's pin: (refdes, pin) → net.
fn pin_nets(s: &Schematic) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    for n in extract(&s.flat()) {
        for p in n.pins {
            out.insert((p.refdes, p.pin), n.name.clone());
        }
    }
    out
}

/// The net a hole is on: the schematic net of a lead or an off-board wire on the same piece of strip (the colour of a link drawn
/// there). Empty when nothing on the piece has a net.
pub fn net_at(s: &Schematic, l: &StripLayout, at: At) -> String {
    let pieces = l.pieces();
    let Some(&(r, c0, c1)) = pieces.iter().find(|(r, a, b)| *r == at.0 && at.1 >= *a && at.1 <= *b) else { return String::new() };
    let on = |h: At| h.0 == r && h.1 >= c0 && h.1 <= c1;
    let nets = pin_nets(s);
    for p in &l.parts {
        for (pin, h) in &p.leads {
            if on(*h)
                && let Some(n) = nets.get(&(p.refdes.clone(), pin.clone()))
            {
                return n.clone();
            }
        }
    }
    l.off.iter().flat_map(|(_, v)| v.iter()).find(|(_, n, h)| on(*h) && !n.is_empty()).map(|(_, n, _)| n.clone()).unwrap_or_default()
}

/// The layout with a link from `a` to `b` (coloured by the net at either end).
pub fn with_link(s: &Schematic, l: &StripLayout, a: At, b: At) -> StripLayout {
    let mut out = l.clone();
    let net = Some(net_at(s, l, a)).filter(|n| !n.is_empty()).unwrap_or_else(|| net_at(s, l, b));
    out.links.push(Link { a, b, net });
    grown(out)
}

/// Whether a bare wire moved from `was` to `now` is still the kind it was: straight along a row or a column, or at 45 degrees.
fn same_way(was: (At, At), now: (At, At)) -> bool {
    let straight = |(a, b): (At, At)| a.0 == b.0 || a.1 == b.1;
    crate::stripboard::bare_way(now.0, now.1) && straight(was) == straight(now)
}

/// The layout with a bare wire drawn by hand on the copper side from `a` to `b` (a perfboard): along a row or a column, or at 45
/// degrees; Err with why not otherwise, or when it would cross a wire slanting the other way between the holes (bare wires that
/// cross there touch: a short no hole shows).
pub fn with_bare(l: &StripLayout, a: At, b: At) -> Result<StripLayout, &'static str> {
    let (dr, dc) = (b.0 as i32 - a.0 as i32, b.1 as i32 - a.1 as i32);
    if a == b {
        return Err("同じ穴です。別の穴をクリックします");
    }
    if dr != 0 && dc != 0 && dr.abs() != dc.abs() {
        return Err("すずめっき線は、同じ行・同じ列か、ななめ 45° の穴へ引きます (途中の穴の真ん中を通る向き)");
    }
    let mine = crate::stripboard::bare_crossings(a, b);
    let crosses = l.bare.iter().flat_map(|(x, y)| crate::stripboard::bare_crossings(*x, *y)).any(|(m, s)| mine.iter().any(|(n, t)| *n == m && *t != s));
    if crosses {
        return Err("ほかのななめのすずめっき線と、穴と穴の間で交わります (裸の線どうしが触れてショートします)。別の道を通します");
    }
    let mut out = l.clone();
    out.bare.push((a, b));
    Ok(grown(out))
}

/// The layout without the bare wires drawn by hand that run over hole `at` (ends included); None when there is none.
pub fn without_bare_at(l: &StripLayout, at: At) -> Option<StripLayout> {
    let on = |(a, b): &(At, At)| crate::stripboard::bare_holes(*a, *b).contains(&at);
    let mut out = l.clone();
    out.bare.retain(|w| !on(w));
    (out.bare.len() < l.bare.len()).then_some(out)
}

/// The layout without link `k`.
pub fn without_link(l: &StripLayout, k: usize) -> StripLayout {
    let mut out = l.clone();
    if k < out.links.len() {
        out.links.remove(k);
    }
    out
}

/// End `end` (0 = a, 1 = b) of link `k` moved to `to`, its colour from where it now ends.
pub fn link_end_moved(s: &Schematic, l: &StripLayout, k: usize, end: usize, to: At) -> StripLayout {
    let mut out = l.clone();
    let Some(link) = out.links.get_mut(k) else { return out };
    if end == 0 {
        link.a = to;
    } else {
        link.b = to;
    }
    let (a, b) = (link.a, link.b);
    out.links[k].net = Some(net_at(s, l, a)).filter(|n| !n.is_empty()).unwrap_or_else(|| net_at(s, l, b));
    grown(out)
}

/// The strip cut at `at`, or joined again when it was cut.
pub fn cut_toggled(l: &StripLayout, at: At) -> StripLayout {
    let mut out = l.clone();
    if !out.cuts.remove(&at) {
        out.cuts.insert(at);
    }
    grown(out)
}

/// The saved layout when the schematic has the same parts on the board (with the same pins) and off it as the layout, each part's
/// way on brought up to date (a chip the layout put on the copper side stays there); None when a part came or went.
fn same_parts(s: &Schematic, saved: &StripLayout) -> Option<StripLayout> {
    let c = crate::breadboard::read(s);
    let sorted = |mut v: Vec<String>| {
        v.sort();
        v
    };
    if c.on.len() != saved.parts.len() || c.off.len() != saved.off.len() {
        return None;
    }
    for (r, _, pins) in &c.on {
        let p = saved.parts.iter().find(|p| &p.refdes == r)?;
        if sorted(pins.iter().map(|x| x.0.clone()).collect()) != sorted(p.leads.iter().map(|x| x.0.clone()).collect()) {
            return None;
        }
    }
    for (r, _) in &c.off {
        saved.off.iter().find(|(x, _)| x == r)?;
    }
    let mut out = saved.clone();
    for p in &mut out.parts {
        if let Some(m) = c.mounts.get(&p.refdes) {
            // (the automatic layout's chips on the copper side are leaded parts to the schematic)
            let auto_back = matches!(p.mount, crate::mount::Mount::Back { .. }) && *m == crate::mount::Mount::Leaded;
            if !auto_back {
                p.mount = m.clone();
            }
        }
    }
    Some(out)
}

/// A saved layout in step with the schematic: the parts no longer in it (or whose pins changed) go, the parts new to it are laid out
/// as the automatic layout would and put to the right of the board (a column between, cut on every row, and three spare holes
/// either side), with the automatic layout's cuts among them, and every other part, link and cut stays where it was. The wires the
/// new parts need are drawn: each line of the ratsnest that ends at a new part, between the free holes nearest its ends on their
/// pieces of strip; a piece with no free hole leaves its net apart, which the check names.
pub fn synced(s: &Schematic, saved: &StripLayout, opt: StripOptions) -> StripLayout {
    // the same parts with the same pins and the same parts off the board (a hand edit of the layout, nothing new in the schematic):
    // the saved layout as it is, the ways on from the schematic, without making the automatic layout (a perfboard's takes up to
    // 1.5 s, and it was made after every wire drawn by hand)
    if let Some(out) = same_parts(s, saved) {
        return out;
    }
    let auto = if saved.perf { crate::perfboard::perf_layout_with(s, opt) } else { strip_layout_with(s, opt) };
    let mut out = saved.clone();
    let pins = |leads: &[(String, At)]| {
        let mut v: Vec<&String> = leads.iter().map(|(p, _)| p).collect();
        v.sort();
        v.into_iter().cloned().collect::<Vec<_>>()
    };
    out.parts.retain(|p| auto.parts.iter().any(|a| a.refdes == p.refdes && pins(&a.leads) == pins(&p.leads)));
    for p in &mut out.parts {
        // a footprint changed (a SOP for a DIP): the socket or the adapter follows the schematic
        if let Some(a) = auto.parts.iter().find(|a| a.refdes == p.refdes) {
            p.mount = a.mount.clone();
        }
    }
    out.off.retain(|(r, v)| auto.off.iter().any(|(a, w)| a == r && w.len() == v.len()));
    let new_parts: Vec<_> = auto.parts.iter().filter(|a| !out.parts.iter().any(|p| p.refdes == a.refdes)).cloned().collect();
    let new_off: Vec<_> = auto.off.iter().filter(|(a, _)| !out.off.iter().any(|(r, _)| r == a)).cloned().collect();
    if new_parts.is_empty() && new_off.is_empty() {
        return out;
    }
    // the new ones keep their places among themselves, moved right of everything on the board
    let left = new_parts.iter().flat_map(|p| p.leads.iter().map(|(_, a)| a.1)).chain(new_off.iter().flat_map(|(_, v)| v.iter().map(|(_, _, a)| a.1))).min().unwrap_or(0);
    // a column between the board and the new parts (cut on every row), then three spare holes: the pieces of the new parts' strips
    // need free holes for the ends of their wires
    const SPARE: i32 = 3;
    let dc = out.cols as i32 + 1 + SPARE - left as i32;
    let gap = out.cols;
    let span = new_parts.iter().flat_map(|p| p.leads.iter().map(|(_, a)| a.1)).fold((u16::MAX, 0u16), |m, c| (m.0.min(c), m.1.max(c)));
    let mut new_holes: Vec<At> = vec![];
    for mut p in new_parts {
        for (_, at) in &mut p.leads {
            at.1 = (at.1 as i32 + dc) as u16;
            new_holes.push(*at);
        }
        out.parts.push(p);
    }
    for (r, mut v) in new_off {
        for (_, _, at) in &mut v {
            at.1 = (at.1 as i32 + dc) as u16;
            new_holes.push(*at);
        }
        out.off.push((r, v));
    }
    // a perfboard has no strips to cut: the new parts' lines are wired round what the person made (`wire_added`)
    if out.perf {
        let mut out = grown(out);
        let nets = pin_nets(s);
        let mut hole_nets: BTreeMap<At, String> = BTreeMap::new();
        for p in &out.parts {
            for (pin, a) in &p.leads {
                if let Some(n) = nets.get(&(p.refdes.clone(), pin.clone())) {
                    hole_nets.insert(*a, n.clone());
                }
            }
        }
        for (_, v) in &out.off {
            for (_, n, a) in v {
                hole_nets.insert(*a, n.clone());
            }
        }
        let pairs: Vec<(String, At, At)> = crate::stripboard::strip_unjoined(s, &out).into_iter().filter(|(_, a, b)| new_holes.contains(a) || new_holes.contains(b)).collect();
        crate::perfboard::wire_added(&mut out, &hole_nets, &pairs);
        return grown(out);
    }
    // the new parts' strips cut off from the board's (a column between), and cut as the automatic layout cuts them (a chip's pins
    // each on a piece of their own)
    let rows = out.rows.max(auto.rows);
    for r in 0..rows {
        out.cuts.insert((r, gap));
    }
    if span.0 <= span.1 {
        for c in auto.cuts.iter().filter(|c| c.1 as i32 + dc > gap as i32 && c.1 <= span.1 + 1) {
            if let Some(at) = shift(*c, 0, dc) {
                out.cuts.insert(at);
            }
        }
        out.cols = out.cols.max((span.1 as i32 + dc + 1 + SPARE) as u16);
    }
    let mut out = grown(out);
    // the wires the new parts need: each line of the ratsnest that ends at a new part, between the free holes nearest its two ends
    // on their pieces of strip (`free_hole`)
    let lines: Vec<(String, At, At)> = crate::stripboard::strip_unjoined(s, &out).into_iter().filter(|(_, a, b)| new_holes.contains(a) || new_holes.contains(b)).collect();
    join_lines(s, &mut out, lines);
    out
}

/// Joins each of `lines` (net, one end, the other: lines of the ratsnest) on a stripboard by a wire between the free holes nearest its
/// two ends on their pieces of strip (`free_hole`; an end with no room: another hole of the net, the nearest first; a wire from off the
/// board with no room at its end goes on to the other end's piece instead). A line with no room at either end is left (the check says).
pub(crate) fn join_lines(s: &Schematic, out: &mut StripLayout, lines: Vec<(String, At, At)>) {
    let mut taken: std::collections::BTreeSet<At> = out.used().into_iter().map(|(a, _)| a).collect();
    let nets = pin_nets(s);
    let holes_of = |out: &StripLayout, net: &str| -> Vec<At> {
        let leads = out.parts.iter().flat_map(|p| p.leads.iter().map(move |(pin, a)| ((p.refdes.clone(), pin.clone()), *a)));
        let offs = out.off.iter().flat_map(|(r, v)| v.iter().map(move |(pin, _, a)| ((r.clone(), pin.clone()), *a)));
        leads.chain(offs).filter(|(k, _)| nets.get(k).is_some_and(|n| n == net)).map(|(_, a)| a).collect()
    };
    for (net, a, b) in lines {
        // an end with no room on its piece: another hole of the net instead, the nearest first (not one on the other end's piece)
        let end_near = |out: &mut StripLayout, at: At, other: At, taken: &std::collections::BTreeSet<At>| -> Option<At> {
            if let Some(h) = free_hole(out, at, taken) {
                return Some(h);
            }
            let pieces = out.pieces();
            let piece_of = |h: At| pieces.iter().position(|(r, x, y)| *r == h.0 && h.1 >= *x && h.1 <= *y);
            let mut alt: Vec<At> = holes_of(out, &net).into_iter().filter(|h| *h != at && piece_of(*h) != piece_of(other)).collect();
            alt.sort_by_key(|h| (h.0 as i32 - at.0 as i32).abs() + (h.1 as i32 - at.1 as i32).abs());
            if let Some(h) = alt.into_iter().find_map(|h| free_hole(out, h, taken)) {
                return Some(h);
            }
            // last: any free hole already joined to the end (through a wire to a strip with room), the nearest
            let groups = out.hole_groups();
            let g = groups.get(&at)?;
            let og = groups.get(&other);
            groups
                .iter()
                .filter(|(h, x)| *x == g && Some(*x) != og && !taken.contains(*h) && !out.cuts.contains(*h))
                .map(|(h, _)| *h)
                .min_by_key(|h| (h.0 as i32 - at.0 as i32).abs() + (h.1 as i32 - at.1 as i32).abs())
        };
        // a wire from off the board with no room at its end: the wire goes on to the other end's piece instead
        let off_end = |out: &StripLayout, at: At| out.off.iter().enumerate().find_map(|(i, (_, v))| v.iter().position(|w| w.2 == at).map(|k| (i, k)));
        let Some(ha) = end_near(out, a, b, &taken) else {
            if let Some((i, k)) = off_end(out, a)
                && let Some(hb) = end_near(out, b, a, &taken)
            {
                out.off[i].1[k].2 = hb;
                taken.insert(hb);
            }
            continue;
        };
        taken.insert(ha);
        let Some(hb) = end_near(out, b, a, &taken) else {
            taken.remove(&ha);
            if let Some((i, k)) = off_end(out, b) {
                out.off[i].1[k].2 = ha;
                taken.insert(ha);
            }
            continue;
        };
        taken.insert(hb);
        out.links.push(Link { a: ha, b: hb, net });
    }
}

/// The free hole nearest `at` on its piece of strip, for a wire's end. A piece with none is made longer (nothing more is joined): a
/// cut at its end moves a hole out when that hole is free, or goes when the piece beyond has nothing on it, or the board grows a
/// column at its right edge.
fn free_hole(out: &mut StripLayout, at: At, taken: &std::collections::BTreeSet<At>) -> Option<At> {
    for _ in 0..6 {
        let pieces = out.pieces();
        let piece_of = |h: At| pieces.iter().copied().find(|(r, x, y)| *r == h.0 && h.1 >= *x && h.1 <= *y);
        let (r, x, y) = piece_of(at)?;
        let mut cols: Vec<u16> = (x..=y).collect();
        cols.sort_by_key(|c| c.abs_diff(at.1));
        if let Some(h) = cols.into_iter().map(|c| (r, c)).find(|h| !taken.contains(h)) {
            return Some(h);
        }
        let empty = |p: Option<(u16, u16, u16)>| p.is_some_and(|(r, x, y)| (x..=y).all(|c| !taken.contains(&(r, c))));
        // a cut moved one hole out, onto a free hole of the next piece (a row shared by nets: each stretch as short as its ends)
        if out.cuts.contains(&(r, y + 1)) && y + 2 < out.cols && !taken.contains(&(r, y + 2)) && !out.cuts.contains(&(r, y + 2)) {
            out.cuts.remove(&(r, y + 1));
            out.cuts.insert((r, y + 2));
            continue;
        }
        if x >= 2 && out.cuts.contains(&(r, x - 1)) && !taken.contains(&(r, x - 2)) && !out.cuts.contains(&(r, x - 2)) {
            out.cuts.remove(&(r, x - 1));
            out.cuts.insert((r, x - 2));
            continue;
        }
        if y + 1 >= out.cols {
            out.cols += 1;
        } else if out.cuts.contains(&(r, y + 1)) && (y + 2 >= out.cols || empty(piece_of((r, y + 2)))) {
            out.cuts.remove(&(r, y + 1));
        } else if x > 0 && out.cuts.contains(&(r, x - 1)) && (x < 2 || empty(piece_of((r, x - 2)))) {
            out.cuts.remove(&(r, x - 1));
        } else {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stripboard::{strip_check, strip_layout};

    /// Picked parts lined up: the shift register's eight resistors in the order of the outputs they are wired to (on the
    /// perfboard: locked where they go); three in one row; fewer than two parts,
    /// or two onto the same holes, is an error that says so.
    #[test]
    fn picked_parts_line_up() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/shift_register_leds.schjson");
        let s: Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let l = crate::perfboard::perf_layout_with(&s, StripOptions::default());
        let rs: Vec<String> = (1..=8).map(|i| format!("R{i}")).collect();
        let nets = pin_nets(&s);
        let at = |l: &StripLayout, r: &str| l.parts.iter().find(|p| p.refdes == r).map(part_box).unwrap();
        // the hole of the U1 pin each resistor is wired to
        let u1_pin = |l: &StripLayout, r: &str| -> At {
            let mine: Vec<&String> = nets.iter().filter(|(k, _)| k.0 == r).map(|(_, n)| n).collect();
            let u1 = l.parts.iter().find(|p| p.refdes == "U1").unwrap();
            u1.leads.iter().find(|(pin, _)| nets.get(&("U1".to_string(), pin.clone())).is_some_and(|n| mine.contains(&n))).unwrap().1
        };
        let m = parts_aligned(&s, &l, &rs, StripAlign::NetOrder, 1).unwrap();
        let mut order = rs.clone();
        let across = rs.iter().map(|r| at(&m, r).0).collect::<std::collections::BTreeSet<_>>().len() == 1;
        let along = |b: (u16, u16, u16, u16)| if across { b.2 } else { b.0 };
        order.sort_by_key(|r| along(at(&m, r)));
        let pins: Vec<At> = order.iter().map(|r| u1_pin(&m, r)).collect();
        let key = |a: &At| if across { a.1 } else { a.0 };
        assert!(pins.windows(2).all(|w| key(&w[0]) <= key(&w[1])) || pins.windows(2).all(|w| key(&w[0]) >= key(&w[1])), "{order:?} {pins:?}");
        assert!(rs.iter().all(|r| m.locked.contains(r)));
        // (the board laid out round them: the app's test `picked_parts_line_up_and_the_rest_is_laid_out_round_them`)
        // tops on one row
        let m = parts_aligned(&s, &l, &rs[..3], StripAlign::Row, 1).unwrap();
        assert_eq!(rs[..3].iter().map(|r| at(&m, r).0).collect::<std::collections::BTreeSet<_>>().len(), 1, "one row");
        assert!(parts_aligned(&s, &l, &rs[..1], StripAlign::Row, 1).unwrap_err().contains("2 つ以上"));
        // two parts lined up onto the same holes: in the way, nothing moved
        let (a, b) = (at(&l, "R1"), at(&l, "R2"));
        let side = part_moved(&l, "R2", a.0 as i32 - b.0 as i32, a.2 as i32 + 9 - b.2 as i32).unwrap();
        let err = parts_aligned(&s, &side, &["R1".to_string(), "R2".to_string()], StripAlign::Left, 1).unwrap_err();
        assert!(err.contains("R2 があります") || err.contains("R1 があります"), "{err}");
    }

    /// All the parts picked and moved together keep the board joined as drawn (the links among them go too); a pick of some leaves
    /// the others where they were even inside the box round the pick; turning a pick of all keeps the stripboard joined; a box that
    /// only crosses a chip still touches it.
    #[test]
    fn picked_parts_move_and_turn_as_one() {
        let s = blinker();
        let l = crate::stripboard::strip_layout(&s);
        let all: Vec<String> = l.parts.iter().map(|p| p.refdes.clone()).collect();
        // all of them, picked with a box round the whole board: as moving the board's box
        let whole = [((0, 0), (l.rows - 1, l.cols - 1))];
        let moved = picked_moved(&l, &all, &whole, 2, 3).unwrap();
        assert_eq!(moved, area_moved(&l, whole[0].0, whole[0].1, 2, 3).unwrap());
        assert!(crate::stripboard::strip_check(&s, &moved).is_empty(), "{:?}", crate::stripboard::strip_check(&s, &moved));
        let turned = picked_turned(&l, &all, &whole).unwrap();
        assert!(crate::stripboard::strip_check(&s, &turned).is_empty(), "half a turn keeps the strips: {:?}", crate::stripboard::strip_check(&s, &turned));
        // the chip and one other part: the parts not picked stay
        let chip = l.parts.iter().find(|p| p.leads.len() == 8).unwrap().refdes.clone();
        let other = l.parts.iter().find(|p| p.refdes != chip).unwrap().refdes.clone();
        let pick = vec![chip.clone(), other.clone()];
        let m = picked_moved(&l, &pick, &[], 0, 1).unwrap();
        for p in &l.parts {
            let after = &m.parts.iter().find(|q| q.refdes == p.refdes).unwrap().leads;
            let shifted = p.leads.iter().zip(after).all(|(a, b)| b.1 == (a.1 .0, a.1 .1 + 1));
            assert_eq!(shifted, pick.contains(&p.refdes), "{}: {:?} -> {:?}", p.refdes, p.leads, after);
        }
        assert!(picked_moved(&l, &[], &whole, 0, 1).is_none(), "nothing picked");
        // a box on one row through the middle of the chip touches it but does not enclose it
        let (r0, r1, c0, _) = part_box(l.parts.iter().find(|p| p.refdes == chip).unwrap());
        let mid = (r0 + r1) / 2;
        assert!(parts_touching_box(&l, (mid, c0), (mid, c0)).contains(&chip));
        assert!(!parts_in_box(&l, (mid, c0), (mid, c0)).contains(&chip));
    }

    fn blinker() -> Schematic {
        serde_json::from_str(include_str!("../../../docs/examples/ne555_blinker.schjson")).unwrap()
    }

    /// Moving the whole board by an area keeps it joined as drawn; moving one chip off its cut pieces does not, and the check says so.
    #[test]
    fn moves_keep_or_break_the_joins_and_the_check_follows() {
        let s = blinker();
        let l = strip_layout(&s);
        let whole = area_moved(&l, (0, 0), (l.rows - 1, l.cols - 1), 2, 3).unwrap();
        assert!(strip_check(&s, &whole).is_empty(), "{:?}", strip_check(&s, &whole));
        assert!(whole.used().iter().all(|(a, _)| a.0 < whole.rows && a.1 < whole.cols), "the board grows with what is on it");
        assert!(whole.rows >= l.rows && whole.cols >= l.cols);
        let chip = l.parts.iter().find(|p| p.leads.len() == 8).unwrap().refdes.clone();
        let off = part_moved(&l, &chip, 0, 1).unwrap();
        assert!(!strip_check(&s, &off).is_empty(), "a chip moved off its pieces is caught");
        assert!(part_moved(&l, &chip, -100, 0).is_none(), "not off the top");
        // turning a chip twice puts it back
        let back = part_turned(&part_turned(&l, &chip).unwrap(), &chip).unwrap();
        assert_eq!(back.parts, l.parts);
    }

    /// A box turned half a turn: the whole board of every sample stays joined as drawn (the strips stay along the rows), turned twice
    /// it is as it was, and an empty box changes nothing. A box that cuts through a part leaves it where it was.
    #[test]
    fn a_box_turns_half_a_turn() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        for name in ["ne555_blinker", "seg7_counter", "traffic_light", "current_sensors"] {
            let s: Schematic = serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{name}.schjson"))).unwrap()).unwrap();
            let l = strip_layout(&s);
            let (a, b) = ((0, 0), (l.rows - 1, l.cols - 1));
            let once = area_turned(&l, a, b);
            assert!(strip_check(&s, &once).is_empty(), "{name}: {:?}", strip_check(&s, &once));
            assert_ne!(once.parts, l.parts, "{name}: turned");
            assert_eq!(area_turned(&once, a, b), l, "{name}: twice is as it was");
        }
        let s = blinker();
        let l = strip_layout(&s);
        assert_eq!(area_turned(&l, (l.rows + 5, 0), (l.rows + 6, 3)), l, "nothing in the box");
        let chip = l.parts.iter().find(|p| p.leads.len() == 8).unwrap();
        let top = chip.leads.iter().map(|x| x.1 .0).min().unwrap();
        let half = area_turned(&l, (0, 0), (top, l.cols - 1));
        assert_eq!(half.parts.iter().find(|p| p.refdes == chip.refdes), Some(chip), "a part half in the box stays");
    }

    /// On a perfboard (no strips) a chip and a box turn a quarter turn: the chip's two rows of pins go from columns to rows, the box
    /// is as tall as it was wide, and four turns bring each back as it was.
    #[test]
    fn a_perfboard_turns_a_quarter_turn() {
        let s = blinker();
        let l = crate::perfboard::perf_layout_with(&s, StripOptions::default());
        let chip = l.parts.iter().find(|p| p.leads.len() == 8).unwrap().refdes.clone();
        let cols = |l: &StripLayout| l.parts.iter().find(|p| p.refdes == chip).unwrap().leads.iter().map(|x| x.1 .1).collect::<std::collections::BTreeSet<u16>>().len();
        // upright two columns of pins, turned four (the layout may have it either way)
        let once = part_turned(&l, &chip).unwrap();
        let mut both = [cols(&l), cols(&once)];
        both.sort();
        assert_eq!(both, [2, 4], "a quarter turn lays the two rows of pins along the rows or the columns");
        let mut back = once;
        for _ in 0..3 {
            back = part_turned(&back, &chip).unwrap();
        }
        assert_eq!(back.parts, l.parts);
        let (a, b) = ((0, 0), (l.rows - 1, l.cols - 1));
        assert_eq!(turned_box(&l, a, b), ((0, 0), (l.cols - 1, l.rows - 1)));
        let mut m = l.clone();
        let mut bx = (a, b);
        for _ in 0..4 {
            let next = turned_box(&m, bx.0, bx.1);
            m = area_turned(&m, bx.0, bx.1);
            bx = next;
            assert!(strip_check(&s, &m).is_empty(), "{:?}", strip_check(&s, &m));
        }
        assert_eq!((m.parts, m.bare, m.links), (l.parts.clone(), l.bare.clone(), l.links.clone()), "four turns: as it was");
    }

    #[test]
    fn links_and_cuts_by_hand() {
        let s = blinker();
        let l = strip_layout(&s);
        // a link taken away leaves a net apart; drawn again between the same holes, it is whole again with its net's colour
        let k = l.links[0].clone();
        let open = without_link(&l, 0);
        assert!(!strip_check(&s, &open).is_empty());
        let again = with_link(&s, &open, k.a, k.b);
        assert!(strip_check(&s, &again).is_empty(), "{:?}", strip_check(&s, &again));
        assert_eq!(again.links.last().unwrap().net, k.net);
        // a cut joined again shorts two pins' pieces; cutting it again puts it back
        let c = *l.cuts.iter().next().unwrap();
        let joined = cut_toggled(&l, c);
        assert_eq!(cut_toggled(&joined, c).cuts, l.cuts);
        // an end moved to another hole and back
        let moved = link_end_moved(&s, &l, 0, 1, (k.b.0, k.b.1 + 1));
        assert_eq!(link_end_moved(&s, &moved, 0, 1, k.b).links, l.links);
    }

    /// Bare wires drawn by hand on a perfboard's copper side: one in place of a link joins the net again; one whose way runs over
    /// another net's wire touches it (a short, as on the board). A wire is straight along a row or a column (or at 45 degrees:
    /// `bare_wires_slant`), a right click on it
    /// takes it away, it is kept with the layout, and it is drawn among the perfboard's wires.
    #[test]
    fn bare_wires_by_hand() {
        let s = blinker();
        let l = strip_layout(&s);
        let straight: Vec<usize> = (0..l.links.len()).filter(|k| l.links[*k].a.1 == l.links[*k].b.1).collect();
        let bare_for = |k: usize| with_bare(&without_link(&l, k), l.links[k].a, l.links[k].b).unwrap();
        let clean = straight.iter().copied().find(|k| strip_check(&s, &bare_for(*k)).is_empty()).expect("a link a bare wire can take the place of");
        assert!(!strip_check(&s, &without_link(&l, clean)).is_empty(), "the link alone joined the net");
        let crossing = straight.iter().copied().find(|k| strip_check(&s, &bare_for(*k)).iter().any(|p| p.starts_with("ショート"))).expect("a bare wire over another net's wire");
        let spans = l.perf_spans();
        let k = &l.links[crossing];
        assert!((k.a.0.min(k.b.0) + 1..k.a.0.max(k.b.0)).any(|r| spans.iter().any(|(p, q)| p.0 == r && k.a.1 >= p.1 && k.a.1 <= q.1)), "it runs over a wire");
        // a row, a column or 45 degrees only; taken away by a hole on it; saved; drawn
        assert!(with_bare(&l, (0, 0), (1, 2)).is_err() && with_bare(&l, (0, 0), (0, 0)).is_err());
        assert!(with_bare(&l, (0, 0), (2, 2)).is_ok() && with_bare(&l, (2, 0), (0, 2)).is_ok());
        let m = bare_for(clean);
        let mid = l.links[clean].a;
        assert_eq!(without_bare_at(&m, mid).unwrap().bare, vec![]);
        assert!(without_bare_at(&l, mid).is_none());
        let back: StripLayout = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back, m);
        assert!(!serde_json::to_string(&l).unwrap().contains("bare"), "nothing written without them");
        assert!(crate::strip_print::perf_wires(&m).contains(&m.bare[0]));
        // a block moved with both ends inside moves the wire; one end inside and a sideways move leaves it straight where it was
        let (a, b) = m.bare[0];
        let all = area_moved(&m, (0, 0), (m.rows, m.cols), 0, 1).unwrap();
        assert_eq!(all.bare[0], ((a.0, a.1 + 1), (b.0, b.1 + 1)));
        let one = area_moved(&m, a, a, 0, 1).unwrap();
        assert_eq!(one.bare[0], (a, b));
    }

    /// A part added to the schematic lands right of the saved board (its strips cut off from the board's, and the wires its nets need
    /// drawn), the moved parts stay where they were, a part taken out goes.
    #[test]
    fn a_saved_layout_follows_the_schematic() {
        let s = blinker();
        let auto = strip_layout(&s);
        let first = auto.parts.iter().find(|p| p.leads.len() == 2).unwrap().refdes.clone();
        let saved = part_moved(&auto, &first, 0, 1).unwrap();
        assert_eq!(synced(&s, &saved, StripOptions::default()), saved, "nothing changed: nothing to do");
        let mut more = s.clone();
        let k = more.add(crate::Kind::Resistor, (100, 100), 0);
        let added = more.components[k].refdes.clone();
        let l = synced(&more, &saved, StripOptions::default());
        let r = l.parts.iter().find(|p| p.refdes == added).expect("the new resistor");
        assert!(r.leads.iter().all(|(_, a)| a.1 > saved.cols), "{:?} right of {} columns", r.leads, saved.cols);
        assert_eq!(l.parts.iter().find(|p| p.refdes == first), saved.parts.iter().find(|p| p.refdes == first), "a moved part stays");
        let mut fewer = s.clone();
        fewer.components.retain(|c| c.refdes != first);
        assert!(synced(&fewer, &saved, StripOptions::default()).parts.iter().all(|p| p.refdes != first));
        // each part of a sample added to a board laid out without it: cut off from the board's strips, and wired to its nets (the
        // active filter's input source moves its wire on when its piece has no room; the counter's rows are shared by nets, so the
        // cuts between them move out a hole)
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        for name in ["ne555_blinker", "active_filter", "seg7_counter"] {
            let s: Schematic = serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{name}.schjson"))).unwrap()).unwrap();
            for c in &s.components {
                let mut without = s.clone();
                without.components.retain(|x| x.refdes != c.refdes);
                let before = strip_layout(&without);
                if before.parts.len() == strip_layout(&s).parts.len() {
                    continue;
                }
                let l = synced(&s, &before, StripOptions::default());
                assert!(strip_check(&s, &l).is_empty(), "{name}: {} added: {:?}", c.refdes, strip_check(&s, &l));
                // on every row a cut between the board's last hole in use and the new parts, unless the new part's piece was grown
                // onto the board's strip of its own net (a piece with no room for a wire's end: the blinker's D1 onto R3's LEDA strip,
                // on a board with the 555 turned half a turn)
                let used = before.used();
                let cut = |r: u16| {
                    let last = used.iter().filter(|(a, _)| a.0 == r).map(|(a, _)| a.1).max();
                    let same = last.is_some_and(|x| {
                        let n = crate::stripboard::net_at(&s, &l, (r, x));
                        n.is_some() && n == crate::stripboard::net_at(&s, &l, (r, before.cols))
                    });
                    same || l.cuts.iter().any(|c| c.0 == r && last.is_none_or(|x| c.1 > x) && c.1 <= before.cols)
                };
                assert!((0..before.rows).all(cut), "{name}: a column cut between ({} added)", c.refdes);
            }
        }
    }

    #[test]
    fn a_saved_perfboard_wires_the_parts_added() {
        // a part of a sample (the chip and three two-lead parts: each layout takes a while) added to a perfboard laid out without
        // it: put right of the board and wired to its nets round the copper already there (bare wires, a link where they cannot
        // pass); the rest stays as it was
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
        let opt = StripOptions::default();
        let mut n_left = 0;
        for name in ["ne555_blinker"] {
            let s: Schematic = serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{name}.schjson"))).unwrap()).unwrap();
            for c in s.components.iter().filter(|c| ["U1", "R2", "C1", "D1"].contains(&c.refdes.as_str())) {
                let mut without = s.clone();
                without.components.retain(|x| x.refdes != c.refdes);
                let before = crate::perfboard::perf_layout_with(&without, opt);
                let l = synced(&s, &before, opt);
                assert!(before.bare.iter().all(|w| l.bare.contains(w)) && before.links.iter().all(|k| l.links.contains(k)), "{name}: the wires there stay");
                // a line may be left only at a lead the wires there close in on all sides (the 555's OUT when its resistor is added:
                // alone on its net, the automatic layout wired other nets round it): the check names it
                let mut closed: std::collections::BTreeSet<At> = before.used().into_iter().map(|(a, _)| a).collect();
                for (a, b) in &before.bare {
                    for r in a.0.min(b.0)..=a.0.max(b.0) {
                        for cc in a.1.min(b.1)..=a.1.max(b.1) {
                            closed.insert((r, cc));
                        }
                    }
                }
                let shut = |h: At| [(0i32, 1i32), (1, 0), (0, -1), (-1, 0)].iter().all(|(dr, dc)| {
                    let (y, x) = (h.0 as i32 + dr, h.1 as i32 + dc);
                    y < 0 || x < 0 || closed.contains(&(y as u16, x as u16))
                });
                let left = crate::stripboard::strip_unjoined(&s, &l);
                assert!(left.iter().all(|(_, a, b)| shut(*a) || shut(*b)), "{name}: {} added: {:?}", c.refdes, strip_check(&s, &l));
                if left.is_empty() {
                    assert!(strip_check(&s, &l).is_empty(), "{name}: {} added: {:?}", c.refdes, strip_check(&s, &l));
                    assert!(l.bare.len() > before.bare.len(), "{name}: {} wired", c.refdes);
                }
                n_left += left.len();
            }
        }
        assert!(n_left <= 1, "{n_left} lines left");
    }

    /// Edits by hand on a perfboard with sections: a section's parts moved together take its box with them; a part moved out of its
    /// section stretches the box round it; moved back, the board shrinks again (it grew only to hold the part); Delete over a box takes
    /// the wires wholly inside it; a bare wire is found under the pointer.
    #[test]
    fn hand_edits_keep_sections_and_size_in_step() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/active_filter.schjson");
        let mut s: Schematic = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let group = |name: &str, parts: &[&str]| design_spec::PlaceGroup { name: name.into(), parts: parts.iter().map(|p| p.to_string()).collect(), section: true, ..Default::default() };
        s.requirements.groups = vec![group("前段", &["U1", "R1", "R2", "C1", "C2"]), group("後段", &["U2", "R3", "R4", "C3", "C4"])];
        let l = crate::perfboard::perf_layout_with(&s, StripOptions::default());
        let bx = |l: &StripLayout, n: &str| l.sections.iter().find(|x| x.0 == n).map(|x| (x.1, x.2)).unwrap();
        // the whole first section moved 2 down and 3 right: its box with it, the other's left alone
        let first: Vec<String> = ["U1", "R1", "R2", "C1", "C2"].map(String::from).to_vec();
        let moved = sections_followed(&s, &l, picked_moved(&l, &first, &[], 2, 3).unwrap());
        let ((a0, b0), (a1, b1)) = (bx(&l, "前段"), bx(&moved, "前段"));
        assert_eq!((a1, b1), ((a0.0 + 2, a0.1 + 3), (b0.0 + 2, b0.1 + 3)));
        assert_eq!(bx(&moved, "後段"), bx(&l, "後段"));
        // one part of the second section taken far right: the box reaches it
        let far = sections_followed(&s, &l, part_moved(&l, "R3", 0, 30).unwrap());
        let r3 = far.parts.iter().find(|p| p.refdes == "R3").unwrap().leads.iter().map(|x| x.1 .1).max().unwrap();
        assert!(bx(&far, "後段").1 .1 > r3, "{:?} {r3}", bx(&far, "後段"));
        assert!(far.cols > l.cols);
        // and back: the board is no wider than before
        let back = sections_followed(&s, &far, part_moved(&far, "R3", 0, -30).unwrap());
        assert!(back.cols <= l.cols, "{} > {}", back.cols, l.cols);
        // Delete over the whole board: every wire on top and every bare wire goes
        let all = [((0, 0), (l.rows, l.cols))];
        let bare = wires_in_boxes_removed(&l, &all).unwrap();
        assert!(bare.links.is_empty() && bare.bare.is_empty());
        assert!(wires_in_boxes_removed(&bare, &all).is_none());
        // a bare wire under the pointer (its middle)
        if let Some((a, b)) = l.bare.first() {
            let mid = ((a.0 + b.0) as f32 / 2.0, (a.1 + b.1) as f32 / 2.0);
            assert!(bare_near(&l, mid, 0.3).is_some_and(|k| { let w = l.bare[k]; (w.0 .0 + w.1 .0) as f32 / 2.0 == mid.0 || (w.0 .1 + w.1 .1) as f32 / 2.0 == mid.1 }));
            assert_eq!(without_bare(&l, 0).bare.len(), l.bare.len() - 1);
        }
    }

    /// A bare wire at 45 degrees runs through the middle of each hole on its way: it joins what is on them (a lead, another wire),
    /// as a straight one. Two slanting the other way cross either on a hole (they touch there: drawn and joined) or between four
    /// holes (they would touch where no hole shows: the second is not drawn).
    #[test]
    fn bare_wires_slant() {
        use crate::stripboard::{bare_holes, bare_way};
        assert_eq!(bare_holes((1, 1), (4, 4)), vec![(1, 1), (2, 2), (3, 3), (4, 4)]);
        assert_eq!(bare_holes((4, 1), (1, 4)), vec![(4, 1), (3, 2), (2, 3), (1, 4)]);
        assert!(bare_way((0, 0), (3, 3)) && bare_way((0, 3), (0, 0)) && !bare_way((0, 0), (1, 3)));
        let s = blinker();
        let mut l = strip_layout(&s);
        l.perf = true;
        l.bare.clear();
        let l = with_bare(&l, (10, 10), (13, 13)).unwrap();
        // the other slant crossing at (11.5, 11.5), between the holes: refused
        assert!(with_bare(&l, (13, 10), (10, 13)).is_err());
        // the other slant crossing on the hole (11, 11): drawn, and the two touch there
        let m = with_bare(&l, (12, 10), (10, 12)).unwrap();
        assert!(m.bare_joins().iter().any(|(a, b)| *a == (12, 10) && *b == (11, 11)), "{:?}", m.bare_joins());
        // one along a row through a hole of it touches it too
        let m = with_bare(&l, (11, 8), (11, 14)).unwrap();
        assert!(m.bare_joins().iter().any(|(a, b)| *a == (11, 8) && *b == (11, 11)), "{:?}", m.bare_joins());
        // taken away by a hole on its way
        assert!(without_bare_at(&l, (12, 12)).is_some_and(|x| x.bare.is_empty()));
        // moved as a block it keeps its slant; turned it slants the other way
        let all = area_moved(&l, (0, 0), (l.rows, l.cols), 1, 1).unwrap();
        assert_eq!(all.bare, vec![((11, 11), (14, 14))]);
    }
}
