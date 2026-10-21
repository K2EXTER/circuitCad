//! The last touch of the automatic placement: pin headers are turned, and moved a little, so that their wires cross
//! less. A header's pin order is fixed by the schematic, so only the way it faces and where it sits can untangle the
//! airwires that fan out of it. Called by `autoplace` after the parts have their places.

use crate::autoplace::{airwire_length, crowded, extent, snap};
use crate::model::*;

/// Most airwires of a board for which `orient_connectors` still tries places (it counts the crossings for every try).
const MAX_TANGLE_WIRES: usize = 250;

/// `orient_connectors` tries places up to this many steps of `ORIENT_STEP` mm away in each direction. (Other values
/// gave 34-80 crossings on `arduino_minimal`; 12 steps of 1 mm gave 34 and left the board routable.)
const ORIENT_REACH: i32 = 12;
const ORIENT_STEP: f64 = 1.0;

/// A row of three or more pads on one line (a pin header): its order is fixed by the schematic, so only the way it
/// faces can untangle its wires.
fn is_pad_row(board: &Board, refdes: &str) -> bool {
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
            if !is_pad_row(board, &part.refdes) {
                continue;
            }
            let cur = (crate::ratsnest::crossings(board), airwire_length(board));
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
                        if crowded(board, &part.refdes) || !inside(board, &part.refdes) {
                            continue;
                        }
                        let x = crate::ratsnest::crossings(board);
                        if x < best.map_or(cur.0, |b| b.2) && airwire_length(board) <= cur.1 * 1.05 + 1.0 {
                            best = Some((pos, rot, x));
                        }
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
}
