//! The last touch of the automatic placement: pin headers are turned, and moved a little, so that their wires cross
//! less. A header's pin order is fixed by the schematic, so only the way it faces and where it sits can untangle the
//! airwires that fan out of it. Called by `autoplace` after the parts have their places.

use crate::autoplace::{crowded, extent, snap};
use crate::model::*;

/// Most airwires of a board for which `orient_connectors` still tries places. Every try counts only the crossings of the moved header's own
/// airwires (see `Tangle`), so this is high: a board of 100-pin chips and many headers (an Arduino Mega) is tried too.
const MAX_TANGLE_WIRES: usize = 2000;

/// The airwires of the nets a header is not on (they stay while the header moves), against which the header's own airwires are counted.
/// A poured net and a ground have none that count (the pour joins them).
struct Tangle {
    fixed: Vec<crate::ratsnest::Airwire>,
    fixed_len: f64,
    own: std::collections::BTreeSet<String>,
}

impl Tangle {
    fn new(board: &Board, refdes: &str) -> Self {
        let poured: std::collections::BTreeSet<&str> = board.zones.iter().map(|z| z.net.as_str()).filter(|n| !n.is_empty()).collect();
        let counts = |n: &str| !n.is_empty() && !poured.contains(n) && !crate::drc::is_ground_net(n);
        let own: std::collections::BTreeSet<String> = board.pads.iter().filter(|p| p.refdes == refdes && counts(&p.net)).map(|p| p.net.clone()).collect();
        let others: std::collections::BTreeSet<&str> = board.pads.iter().map(|p| p.net.as_str()).filter(|n| counts(n) && !own.contains(*n)).collect();
        let fixed = wires(board, &others);
        let fixed_len = fixed.iter().map(|w| w.a.dist(w.b)).sum();
        Tangle { fixed, fixed_len, own }
    }

    /// The crossings of the header's airwires (with the others and among themselves), and the length of all the airwires.
    fn measure(&self, board: &Board) -> (usize, f64) {
        let own: std::collections::BTreeSet<&str> = self.own.iter().map(|n| n.as_str()).collect();
        let moved = wires(board, &own);
        let side = |o: Point, a: Point, b: Point| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
        let cross = |p: &crate::ratsnest::Airwire, q: &crate::ratsnest::Airwire| {
            p.net != q.net && side(p.a, p.b, q.a) * side(p.a, p.b, q.b) < 0.0 && side(q.a, q.b, p.a) * side(q.a, q.b, p.b) < 0.0
        };
        let mut n = 0;
        for (i, p) in moved.iter().enumerate() {
            n += self.fixed.iter().filter(|q| cross(p, q)).count();
            n += moved[i + 1..].iter().filter(|q| cross(p, q)).count();
        }
        (n, self.fixed_len + moved.iter().map(|w| w.a.dist(w.b)).sum::<f64>())
    }
}

/// The airwires of the given nets on a board with no copper yet: the shortest tree over each net's pads.
fn wires(board: &Board, nets: &std::collections::BTreeSet<&str>) -> Vec<crate::ratsnest::Airwire> {
    let mut by_net: std::collections::BTreeMap<&str, Vec<(Point, usize)>> = Default::default();
    for (i, p) in board.pads.iter().enumerate().filter(|(_, p)| nets.contains(p.net.as_str())) {
        by_net.entry(p.net.as_str()).or_default().push((p.pos, i));
    }
    by_net.into_iter().flat_map(|(net, pads)| crate::ratsnest::join_groups(net, &pads)).collect()
}

/// `orient_connectors` tries places up to this many steps of `ORIENT_STEP` mm away in each direction. (Other values
/// gave 34-80 crossings on `arduino_minimal`; 12 steps of 1 mm gave 34 and left the board routable.)
const ORIENT_REACH: i32 = 12;
const ORIENT_STEP: f64 = 1.0;

/// A row of three or more pads on one line (a pin header): its order is fixed by the schematic, so only the way it
/// faces can untangle its wires.
pub(crate) fn is_pad_row(board: &Board, refdes: &str) -> bool {
    let pads: Vec<Point> = board.pads.iter().filter(|p| p.refdes == refdes).map(|p| p.pos).collect();
    pads.len() >= 3 && (pads.iter().all(|p| (p.x - pads[0].x).abs() < 0.01) || pads.iter().all(|p| (p.y - pads[0].y).abs() < 0.01))
}

/// Turn each pin header, and move it within a few millimetres, when that makes fewer airwires of different nets cross,
/// without longer airwires (5 % + 1 mm) and without crowding a neighbour or leaving `bounds`. The headers
/// of a board are what its wires fan out to, and a header that faces the wrong way crosses everything.
/// Undone as a whole when the DRC finds more than before.
pub(crate) fn orient_connectors(board: &mut Board, bounds: (Point, Point)) {
    // every try counts the crossings again: a board with this many airwires is left as it is
    if crate::ratsnest::ratsnest(board).len() > MAX_TANGLE_WIRES {
        return;
    }
    let before = board.clone();
    let drc = crate::drc::check(board).len();
    let center = |board: &Board, r: &str| extent(board, r).map(|(lo, hi)| Point::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0));
    let inside = |board: &Board, r: &str| extent(board, r).is_some_and(|(lo, hi)| lo.x >= bounds.0.x - 1e-6 && lo.y >= bounds.0.y - 1e-6 && hi.x <= bounds.1.x + 1e-6 && hi.y <= bounds.1.y + 1e-6);
    for _ in 0..2 {
        let mut changed = false;
        for part in board.parts.clone() {
            // (a header a person locked stays as it is)
            if !is_pad_row(board, &part.refdes) || board.is_locked(&part.refdes) {
                continue;
            }
            let tangle = Tangle::new(board, &part.refdes);
            let cur = tangle.measure(board);
            let Some(c0) = center(board, &part.refdes) else { continue };
            let mut best: Option<(Point, u8, usize)> = None;
            for turn in 0..4u8 {
                let rot = (part.rot + turn) % 4;
                board.place_part(&part.refdes, part.pos, rot);
                let Some(c1) = center(board, &part.refdes) else { continue };
                let base = Point::new(snap(part.pos.x + c0.x - c1.x), snap(part.pos.y + c0.y - c1.y));
                for dx in -ORIENT_REACH..=ORIENT_REACH {
                    for dy in -ORIENT_REACH..=ORIENT_REACH {
                        if turn == 0 && dx == 0 && dy == 0 {
                            continue;
                        }
                        let pos = Point::new(base.x + dx as f64 * ORIENT_STEP, base.y + dy as f64 * ORIENT_STEP);
                        board.place_part(&part.refdes, pos, rot);
                        // (the crossings first: they are quick, and most places are no better)
                        let (x, len) = tangle.measure(board);
                        if x >= best.map_or(cur.0, |b| b.2) || len > cur.1 * 1.05 + 1.0 {
                            continue;
                        }
                        if crowded(board, &part.refdes) || !inside(board, &part.refdes) {
                            continue;
                        }
                        best = Some((pos, rot, x));
                    }
                }
            }
            match best {
                Some((pos, rot, _)) => {
                    board.place_part(&part.refdes, pos, rot);
                    changed = true;
                }
                None => board.place_part(&part.refdes, part.pos, part.rot),
            }
        }
        if !changed {
            break;
        }
    }
    if crate::drc::check(board).len() > drc {
        *board = before;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autoplace::autoplace;
    use crate::drc;
    use crate::netlist::*;

    /// Two headers wired in opposite order, and a third that takes one pin of each.
    fn headers() -> Board {
        let mut b = Board::new("hdr", 60.0, 40.0);
        let part = |r: &str| NetlistPart { refdes: r.into(), value: "H".into(), footprint: "PinHeader_1x4".into() };
        let parts = [part("J1"), part("J2"), part("J3")];
        let mut nets: Vec<NetlistNet> = (1..=4).map(|k| NetlistNet { name: format!("N{k}"), pins: vec![("J1".into(), k.to_string()), ("J2".into(), (5 - k).to_string())] }).collect();
        nets.push(NetlistNet { name: "X".into(), pins: vec![("J3".into(), "1".into()), ("J1".into(), "1".into())] });
        crate::netlist::import(&mut b, &parts, &nets);
        b
    }

    #[test]
    fn headers_face_so_that_their_wires_cross_less() {
        let mut b = headers();
        let report = autoplace(&mut b).unwrap();
        assert_eq!(report.crossings, crate::ratsnest::crossings(&b));
        assert!(report.crossings <= 2, "{}", report.crossings);
        assert!(drc::check(&b).is_empty());
    }

    #[test]
    fn a_header_is_turned_on_a_board_of_many_airwires_too() {
        // two headers wired in opposite order (their wires cross) and, far from them, 260 resistors joined in pairs pad to pad (260 airwires
        // that cross nothing): more airwires than the old limit of 250, but each try counts only the moved header's own wires
        let mut b = Board::new("many", 800.0, 100.0);
        let part = |r: &str, f: &str| NetlistPart { refdes: r.into(), value: "1".into(), footprint: f.into() };
        let mut parts = vec![part("J1", "PinHeader_1x4"), part("J2", "PinHeader_1x4")];
        let mut nets: Vec<NetlistNet> = (1..=4).map(|k| NetlistNet { name: format!("N{k}"), pins: vec![("J1".into(), k.to_string()), ("J2".into(), (5 - k).to_string())] }).collect();
        for k in 1..=260 {
            parts.push(part(&format!("R{k}"), "R_0805"));
            if k % 2 == 0 {
                for pin in ["1", "2"] {
                    nets.push(NetlistNet { name: format!("A{k}_{pin}"), pins: vec![(format!("R{}", k - 1), pin.into()), (format!("R{k}"), pin.into())] });
                }
            }
        }
        crate::netlist::import(&mut b, &parts, &nets);
        b.place_part("J1", Point::new(10.0, 10.0), 0);
        b.place_part("J2", Point::new(30.0, 10.0), 0);
        for k in 1..=260 {
            b.place_part(&format!("R{k}"), Point::new(5.0 + 5.0 * (k % 130) as f64, if k <= 130 { 60.0 } else { 80.0 }), 0);
        }
        assert!(crate::ratsnest::ratsnest(&b).len() > 250);
        let before = crate::ratsnest::crossings(&b);
        assert!(before > 0);
        orient_connectors(&mut b, (Point::new(5.0, 5.0), Point::new(795.0, 95.0)));
        assert_eq!(crate::ratsnest::crossings(&b), 0, "was {before}");
        // a locked header is left as it is
        b.place_part("J2", Point::new(30.0, 10.0), 0);
        b.locked_parts.push("J2".into());
        b.locked_parts.push("J1".into());
        b.place_part("J1", Point::new(10.0, 10.0), 0);
        orient_connectors(&mut b, (Point::new(5.0, 5.0), Point::new(795.0, 95.0)));
        assert_eq!(crate::ratsnest::crossings(&b), before);
    }
}
