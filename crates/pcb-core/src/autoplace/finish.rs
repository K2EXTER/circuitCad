//! Steps after the rows are packed: lining parts up, bypass capacitors next to their pins, and turns that shorten the airwires.

use super::*;

/// Try turning each part by half a turn (same footprint box, so nothing starts to overlap) and
/// keep the turn when the airwires get shorter. Two passes let neighbours settle.
/// Parts whose centres lie within this distance of each other on one axis are lined up on that axis.
pub(super) const ALIGN_TOL: f64 = 2.0;
/// Least distance kept between the pad boxes of two parts when one is moved to line up (mm).
pub(super) const ALIGN_SPACE: f64 = 0.9;

/// Line up the parts: those in the same row (centres within `ALIGN_TOL` in y) get one centre line, those in the same
/// column likewise. A part moves by whole `SNAP` steps and only when it keeps `ALIGN_SPACE` to every other part.
/// Everything is undone when the airwires get longer than 1 % or the DRC finds more than before.
pub(super) fn align_parts(board: &mut Board, keep: &std::collections::BTreeSet<String>) {
    let before = board.clone();
    let (wire, drc) = (airwire_length(board), crate::drc::check(board).len());
    for axis in [1usize, 0] {
        align_axis(board, axis, keep);
    }
    if airwire_length(board) > wire * 1.01 + 0.5 || crate::drc::check(board).len() > drc {
        *board = before;
    }
}

pub(super) fn align_axis(board: &mut Board, axis: usize, keep: &std::collections::BTreeSet<String>) {
    let coord = |p: Point| if axis == 0 { p.x } else { p.y };
    let mut centers: Vec<(String, f64)> = board.parts.iter().filter(|p| !keep.contains(&p.refdes)).filter_map(|p| extent(board, &p.refdes).map(|(lo, hi)| (p.refdes.clone(), coord(Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0))))).collect();
    centers.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let mut i = 0;
    while i < centers.len() {
        let mut j = i + 1;
        while j < centers.len() && centers[j].1 - centers[i].1 <= ALIGN_TOL {
            j += 1;
        }
        // the member in the middle keeps its place; the others come to its line
        if j - i >= 2 {
            let target = centers[(i + j - 1) / 2].1;
            for (refdes, c) in &centers[i..j] {
                let step = snap(target - c);
                if step.abs() < 1e-9 || (target - c - step).abs() > 0.3 {
                    continue;
                }
                let Some(part) = board.parts.iter().find(|p| &p.refdes == refdes).cloned() else { continue };
                let moved = if axis == 0 { Point::new(part.pos.x + step, part.pos.y) } else { Point::new(part.pos.x, part.pos.y + step) };
                board.place_part(refdes, moved, part.rot);
                if crowded(board, refdes) {
                    board.place_part(refdes, part.pos, part.rot);
                }
            }
        }
        i = j;
    }
}

/// `refdes` is closer than `ALIGN_SPACE` to another part (pad box to pad box).
pub(crate) fn crowded(board: &Board, refdes: &str) -> bool {
    let Some((lo, hi)) = extent(board, refdes) else { return false };
    board.parts.iter().filter(|p| p.refdes != refdes).filter_map(|p| extent(board, &p.refdes)).any(|(l, h)| {
        let gap_x = (l.x - hi.x).max(lo.x - h.x);
        let gap_y = (l.y - hi.y).max(lo.y - h.y);
        gap_x.max(gap_y) < ALIGN_SPACE
    })
}

/// Turn a part about the middle of its pads (the pads' box keeps its place, to a grid step).
pub(super) fn turn_about_centre(board: &mut Board, refdes: &str, turns: u8) {
    let center = |board: &Board, r: &str| extent(board, r).map(|(lo, hi)| Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0));
    let (Some(part), Some(c0)) = (board.parts.iter().find(|p| p.refdes == refdes).cloned(), center(board, refdes)) else { return };
    board.place_part(refdes, part.pos, part.rot + turns);
    if let Some(c1) = center(board, refdes) {
        board.place_part(refdes, Point::new(snap(part.pos.x + c0.x - c1.x), snap(part.pos.y + c0.y - c1.y)), part.rot + turns);
    }
}

/// The copies of a structure stand in their grid; what each copy is joined to outside it (the ports) lies where the rest of the board put
/// it. Here the role of every part is turned, in all the copies at once, to the way that makes the airwires of the whole board shortest:
/// a resistor whose pin goes to the IC turns that pin to the IC. A turn is dropped when a part of a copy would stand too close to another.
pub(super) fn turn_copies_toward_ports(board: &mut Board, groups: &[Vec<Vec<String>>]) {
    for copies in groups {
        for role in 0..copies[0].len() {
            let before = airwire_length(board);
            let mut best: Option<(f64, Board)> = None;
            for turns in 1..4u8 {
                let mut trial = board.clone();
                for copy in copies {
                    turn_about_centre(&mut trial, &copy[role], turns);
                }
                if copies.iter().any(|c| crowded(&trial, &c[role])) {
                    continue;
                }
                let len = airwire_length(&trial);
                if len < before - 0.5 && best.as_ref().is_none_or(|b| len < b.0) {
                    best = Some((len, trial));
                }
            }
            if let Some((_, b)) = best {
                *board = b;
            }
        }
    }
}

/// A bypass capacitor farther than this (mm) from the pin it serves is moved to the pin.
pub(super) const BYPASS_NEAR: f64 = 2.5;
/// A part with this many pads or fewer can be a satellite of an IC (see `ic_blocks`; a crystal counts only the pads not on a ground).
pub(super) const SATELLITE_PADS: usize = 3;
/// How far (mm) from the pin a place is looked for, and the distance between the places tried.
pub(super) const BYPASS_REACH: f64 = 6.0;
pub(super) const BYPASS_STEP: f64 = 1.0;

/// The bypass capacitors that the placement left far from their IC are put next to the supply pin, with the pad on that supply towards the
/// pin (and the other pad towards the IC's pin on the other rail, when it has one): each one goes to the best free place on the grid within
/// `BYPASS_REACH` of the pin, on the board (`inside`), not too close to another part. Parts in `keep` (copies in a grid) are not moved.
pub(super) fn attach_bypass_caps(board: &mut Board, inside: (Point, Point), keep: &std::collections::BTreeSet<String>) {
    use crate::drc::{is_ground_net, is_negative_supply_net, is_supply_net};
    let rail = |n: &str| is_ground_net(n) || is_supply_net(n) || is_negative_supply_net(n);
    let supply = |n: &str| is_supply_net(n) || is_negative_supply_net(n);
    let caps: Vec<String> = board.parts.iter().filter(|p| p.refdes.starts_with('C') && !keep.contains(&p.refdes)).map(|p| p.refdes.clone()).collect();
    for cap in caps {
        let cap_pads: Vec<Pad> = board.pads.iter().filter(|p| p.refdes == cap).cloned().collect();
        // the supply pad of the capacitor and the pin of an IC that it serves: the nearest pair
        let mut best: Option<(f64, Pad, Pad)> = None;
        for cp in cap_pads.iter().filter(|p| supply(&p.net) && cap_pads.iter().any(|q| q.net != p.net && rail(&q.net))) {
            for ic in board.parts.iter().filter(|p| p.refdes.starts_with('U') && bypass_ic(board, &cap).is_none_or(|b| b == p.refdes)) {
                let ic_pads: Vec<&Pad> = board.pads.iter().filter(|p| p.refdes == ic.refdes).collect();
                if ic_pads.len() < 5 {
                    continue;
                }
                for ip in ic_pads.iter().filter(|p| p.net == cp.net) {
                    let d = cp.pos.dist(ip.pos);
                    if best.as_ref().is_none_or(|b| d < b.0) {
                        best = Some((d, cp.clone(), (*ip).clone()));
                    }
                }
            }
        }
        let Some((now, cp, pin)) = best else { continue };
        if now <= BYPASS_NEAR {
            continue;
        }
        // the IC's pin on the other net of the capacitor (a negative rail: its 0 V pin), if there is one: the other pad faces it
        let other_net = cap_pads.iter().find(|q| q.net != cp.net && rail(&q.net)).map(|q| q.net.clone());
        let other_pin: Option<Pad> = other_net.and_then(|n| board.pads.iter().filter(|p| p.refdes == pin.refdes && p.net == n).min_by(|a, b| a.pos.dist(pin.pos).total_cmp(&b.pos.dist(pin.pos))).cloned());
        let Some(part) = board.parts.iter().find(|p| p.refdes == cap).cloned() else { continue };
        let boxes: Vec<(Point, Point)> = board.parts.iter().filter(|p| p.refdes != cap).filter_map(|p| extent(board, &p.refdes)).collect();
        let cost_of = |pads: &[Pad]| -> f64 {
            let a = pads.iter().find(|q| q.pin == cp.pin).map_or(f64::MAX, |q| q.pos.dist(pin.pos));
            let b = other_pin.as_ref().map_or(0.0, |op| pads.iter().find(|q| q.pin != cp.pin && q.net != cp.net).map_or(0.0, |q| q.pos.dist(op.pos)) * 0.5);
            a + b
        };
        let mut scratch = board.clone();
        let mut best_at: Option<(f64, Point, u8)> = None;
        let zones = escape_zones(board, &pin.refdes, &rail);
        // (places 1 mm apart: finer ones cost much and gain little)
        let steps = (BYPASS_REACH / BYPASS_STEP) as i32;
        // the first pass keeps the way out of the IC's signal pins free; when there is no such place, the second does not
        for strict in [true, false] {
        if best_at.is_some() {
            break;
        }
        for ix in -steps..=steps {
            for iy in -steps..=steps {
                let target = Point::new(pin.pos.x + ix as f64 * BYPASS_STEP, pin.pos.y + iy as f64 * BYPASS_STEP);
                if target.dist(pin.pos) > BYPASS_REACH || target.x < inside.0.x || target.y < inside.0.y || target.x > inside.1.x || target.y > inside.1.y {
                    continue;
                }
                for rot in 0..4u8 {
                    // the capacitor turned, its pads' box centred on the target
                    scratch.place_part(&cap, Point::new(0.0, 0.0), rot);
                    let Some((lo, hi)) = extent(&scratch, &cap) else { continue };
                    let at = Point::new(snap(target.x - (lo.x + hi.x) / 2.0), snap(target.y - (lo.y + hi.y) / 2.0));
                    scratch.place_part(&cap, at, rot);
                    let Some((lo, hi)) = extent(&scratch, &cap) else { continue };
                    if lo.x < inside.0.x || lo.y < inside.0.y || hi.x > inside.1.x || hi.y > inside.1.y {
                        continue;
                    }
                    if boxes.iter().any(|(l, h)| (l.x - hi.x).max(lo.x - h.x).max((l.y - hi.y).max(lo.y - h.y)) < ALIGN_SPACE) {
                        continue;
                    }
                    if strict && in_zones(&zones, lo, hi) {
                        continue;
                    }
                    let pads: Vec<Pad> = scratch.pads.iter().filter(|q| q.refdes == cap).cloned().collect();
                    let c = cost_of(&pads);
                    if best_at.as_ref().is_none_or(|b| c < b.0 - 1e-9) {
                        best_at = Some((c, at, rot));
                    }
                }
            }
        }
        }
        if let Some((c, at, rot)) = best_at
            && c < cost_of(&cap_pads) - 1.0
        {
            board.place_part(&cap, at, rot);
        } else {
            let _ = part;
        }
    }
}

pub(super) fn turn_to_shorten(board: &mut Board, keep: &std::collections::BTreeSet<String>) {
    let center = |board: &Board, r: &str| extent(board, r).map(|(lo, hi)| Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0));
    for _ in 0..2 {
        let mut better = false;
        for part in board.parts.clone().into_iter().filter(|p| !keep.contains(&p.refdes)) {
            let before = airwire_length(board);
            let Some(c0) = center(board, &part.refdes) else { continue };
            board.place_part(&part.refdes, part.pos, part.rot + 2);
            if let Some(c1) = center(board, &part.refdes) {
                board.place_part(&part.refdes, Point::new(snap(part.pos.x + c0.x - c1.x), snap(part.pos.y + c0.y - c1.y)), part.rot + 2);
            }
            if airwire_length(board) < before - 1e-6 {
                better = true;
            } else {
                board.place_part(&part.refdes, part.pos, part.rot);
            }
        }
        if !better {
            break;
        }
    }
}
