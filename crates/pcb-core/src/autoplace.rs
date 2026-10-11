//! First-guess placement: pack the parts into rows, strongly connected parts next to each other,
//! and fit a rectangular outline around them. A starting point to refine by hand, not an optimiser.

use crate::*;

/// Clear space kept around every part's pads (mm), on top of the DRC clearance.
const GAP: f64 = 1.6;
/// Extra room above every part for its reference text (silkscreen), so labels do not run into the neighbours.
const LABEL: f64 = 2.4;
/// Space between the parts and the board edge (mm).
const MARGIN: f64 = 3.0;
/// Part origins land on this grid (mm), the editor's default snap, so hand-moved parts line up with placed ones.
const SNAP: f64 = 0.5;

fn snap(v: f64) -> f64 {
    (v / SNAP).round() * SNAP
}

#[derive(Debug, Default)]
pub struct PlaceReport {
    pub placed: usize,
    pub width: f64,
    pub height: f64,
}

fn extent(board: &Board, refdes: &str) -> Option<(Point, Point)> {
    let mut lo = Point::new(f64::MAX, f64::MAX);
    let mut hi = Point::new(f64::MIN, f64::MIN);
    let mut any = false;
    for p in board.pads.iter().filter(|p| p.refdes == refdes) {
        let (w, h) = p.shape.size();
        lo.x = lo.x.min(p.pos.x - w / 2.0);
        lo.y = lo.y.min(p.pos.y - h / 2.0);
        hi.x = hi.x.max(p.pos.x + w / 2.0);
        hi.y = hi.y.max(p.pos.y + h / 2.0);
        any = true;
    }
    any.then_some((lo, hi))
}

/// Closeness of two parts: shared nets count less the more pins the net has (a ground that
/// touches everything says little about who belongs together).
fn closeness(board: &Board, a: &str, b: &str) -> f64 {
    let mut nets: std::collections::BTreeMap<&str, (bool, bool, usize)> = Default::default();
    for p in board.pads.iter().filter(|p| !p.net.is_empty() && !p.refdes.is_empty()) {
        let e = nets.entry(p.net.as_str()).or_default();
        e.2 += 1;
        e.0 |= p.refdes == a;
        e.1 |= p.refdes == b;
    }
    nets.values().filter(|(x, y, _)| *x && *y).map(|(_, _, n)| 1.0 / (*n as f64 - 1.0).max(1.0)).sum()
}

/// Lay every part out in rows starting at the origin and replace the outline by a rectangle
/// around them. Refuses when tracks or vias exist, since moving parts would break them.
pub fn autoplace(board: &mut Board) -> Result<PlaceReport, String> {
    place(board, true)
}

fn place(board: &mut Board, turn: bool) -> Result<PlaceReport, String> {
    if !board.tracks.is_empty() || !board.vias.is_empty() {
        return Err("配線済みの基板では自動配置できません。先に配線を削除してください (配線は部品の位置に合わせて引かれているため)".into());
    }
    if board.parts.is_empty() {
        return Err("部品がありません。先に「回路図から取り込み」をしてください".into());
    }
    // measure each part at the origin, keeping its rotation
    let mut items: Vec<(String, Point, f64, f64)> = vec![]; // refdes, offset of the pad box, w, h
    for part in board.parts.clone() {
        board.place_part(&part.refdes, Point::new(0.0, 0.0), part.rot);
        let Some((lo, hi)) = extent(board, &part.refdes) else { continue };
        items.push((part.refdes, lo, hi.x - lo.x + GAP, hi.y - lo.y + GAP + LABEL));
    }
    // order: begin with the part that has the most pads, then always the one closest to those placed
    let degree = |r: &str| board.pads.iter().filter(|p| p.refdes == r && !p.net.is_empty()).count();
    let mut order: Vec<usize> = vec![];
    let mut left: Vec<usize> = (0..items.len()).collect();
    left.sort_by(|&a, &b| degree(&items[b].0).cmp(&degree(&items[a].0)).then(items[a].0.cmp(&items[b].0)));
    if left.is_empty() {
        return Err("パッドを持つ部品がありません".into());
    }
    order.push(left.remove(0));
    while !left.is_empty() {
        let score = |i: usize| order.iter().map(|&j| closeness(board, &items[i].0, &items[j].0)).sum::<f64>();
        let mut best = 0;
        for k in 1..left.len() {
            if score(left[k]) > score(left[best]) + 1e-9 {
                best = k;
            }
        }
        order.push(left.remove(best));
    }
    // shelf packing into a roughly 3:2 board
    let area: f64 = items.iter().map(|i| i.2 * i.3).sum();
    let widest = items.iter().map(|i| i.2).fold(0.0, f64::max);
    let row_w = (area * 1.5).sqrt().max(widest);
    let (mut x, mut y, mut row_h, mut used_w) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut spots: Vec<(usize, f64, f64)> = vec![];
    for &i in &order {
        let (_, _, w, h) = &items[i];
        if x > 0.0 && x + w > row_w {
            y += row_h;
            x = 0.0;
            row_h = 0.0;
        }
        spots.push((i, x, y));
        x += w;
        used_w = used_w.max(x);
        row_h = row_h.max(*h);
    }
    let used_h = y + row_h;
    for (i, sx, sy) in &spots {
        let (refdes, lo, ..) = &items[*i];
        let rot = board.parts.iter().find(|p| &p.refdes == refdes).map_or(0, |p| p.rot);
        // the pad box starts GAP/2 inside its cell
        board.place_part(refdes, Point::new(snap(MARGIN + GAP / 2.0 + sx - lo.x), snap(MARGIN + GAP / 2.0 + LABEL + sy - lo.y)), rot);
    }
    if turn {
        turn_to_shorten(board);
    }
    let (w, h) = (used_w + 2.0 * MARGIN, used_h + 2.0 * MARGIN);
    board.set_rect_outline(0.0, 0.0, w.ceil(), h.ceil());
    Ok(PlaceReport { placed: spots.len(), width: w.ceil(), height: h.ceil() })
}

fn airwire_length(board: &Board) -> f64 {
    crate::ratsnest::ratsnest(board).iter().map(|w| w.a.dist(w.b)).sum()
}

/// Try turning each part by half a turn (same footprint box, so nothing starts to overlap) and
/// keep the turn when the airwires get shorter. Two passes let neighbours settle.
fn turn_to_shorten(board: &mut Board) {
    let center = |board: &Board, r: &str| extent(board, r).map(|(lo, hi)| Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0));
    for _ in 0..2 {
        let mut better = false;
        for part in board.parts.clone() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netlist::*;

    fn divider() -> Board {
        let part = |r: &str| NetlistPart { refdes: r.into(), value: "1k".into(), footprint: "R_0805".into() };
        let net = |n: &str, pins: &[(&str, &str)]| NetlistNet { name: n.into(), pins: pins.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };
        let mut b = Board::new("t", 10.0, 10.0);
        import(
            &mut b,
            &(1..=6).map(|i| part(&format!("R{i}"))).collect::<Vec<_>>(),
            &[net("A", &[("R1", "1"), ("R2", "1")]), net("B", &[("R2", "2"), ("R3", "1")]), net("C", &[("R3", "2"), ("R4", "1"), ("R5", "1")]), net("GND", &[("R4", "2"), ("R5", "2"), ("R6", "1"), ("R6", "2"), ("R1", "2")])],
        );
        b
    }

    #[test]
    fn parts_land_inside_a_fitted_outline_without_rule_violations() {
        let mut b = divider();
        let rep = autoplace(&mut b).unwrap();
        assert_eq!(rep.placed, 6);
        let (lo, hi) = b.bounds().unwrap();
        assert!(b.pads.iter().all(|p| p.pos.x > lo.x && p.pos.x < hi.x && p.pos.y > lo.y && p.pos.y < hi.y));
        let v = drc::check(&b);
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn the_placement_can_be_routed_completely() {
        let mut b = divider();
        autoplace(&mut b).unwrap();
        let r = autoroute::autoroute(&mut b, &autoroute::RouteOptions::default(), None);
        assert!(r.failed.is_empty(), "{:?}", r.failed);
        assert!(drc::check(&b).is_empty());
    }

    #[test]
    fn connected_parts_sit_closer_than_unrelated_ones() {
        let mut b = divider();
        autoplace(&mut b).unwrap();
        let at = |r: &str| b.parts.iter().find(|p| p.refdes == r).unwrap().pos;
        // R4 and R5 share the net C
        let near = at("R4").dist(at("R5"));
        let far = at("R1").dist(at("R6")).max(at("R2").dist(at("R6")));
        assert!(near <= far, "{near} {far}");
    }

    #[test]
    fn turning_parts_shortens_the_airwires_and_keeps_the_board_clean() {
        let (mut plain, mut turned) = (divider(), divider());
        place(&mut plain, false).unwrap();
        place(&mut turned, true).unwrap();
        let (l0, l1) = (airwire_length(&plain), airwire_length(&turned));
        assert!(l1 < l0, "{l1} vs {l0}");
        assert!(drc::check(&turned).is_empty());
        assert!(turned.parts.iter().any(|p| p.rot == 2));
    }

    #[test]
    fn routed_boards_are_left_alone() {
        let mut b = divider();
        autoplace(&mut b).unwrap();
        autoroute::autoroute(&mut b, &autoroute::RouteOptions::default(), None);
        assert!(autoplace(&mut b).is_err());
    }
}
