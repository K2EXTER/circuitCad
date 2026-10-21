//! Pads of one part that share a net and sit side by side (the two LX pins of a regulator, the paired VIN or OUT pins of a
//! power IC) are joined by a short strap of copper before the router starts. The router then sees one piece of copper
//! and leads one trunk away from it, instead of two parallel tracks that run to the same place and leave a slot between
//! them. It is what one does by hand.

use crate::drc;
use crate::model::*;

/// Pads further apart than this (edge to edge, mm) are not joined.
const MAX_GAP: f64 = 2.5;

/// Offset (mm) the two pad centres may differ by across the strap and still count as lined up.
const LINED_UP: f64 = 0.05;

/// Join every pair of neighbouring surface pads of one part that are on one net, lined up, and no further apart than
/// `MAX_GAP`, by a strap from centre to centre. The strap is as wide as the narrower pad's short side, so it does not
/// reach out of either pad. A strap is kept only when the DRC finds nothing new; nets a pour carries are left alone.
/// Returns how many straps were put in.
pub fn bridge_pads(board: &mut Board) -> usize {
    let poured: Vec<String> = board.zones.iter().map(|z| z.net.clone()).filter(|n| !n.is_empty()).collect();
    let groups = drc::pad_groups(board);
    // candidates: (pad a, pad b) of the same part, net and face
    let mut pairs: Vec<(usize, usize)> = vec![];
    for (i, a) in board.pads.iter().enumerate() {
        if a.refdes.is_empty() || a.net.is_empty() || a.drill.is_some() || drc::is_ground_net(&a.net) || poured.contains(&a.net) {
            continue;
        }
        for (j, b) in board.pads.iter().enumerate().skip(i + 1) {
            if b.refdes != a.refdes || b.net != a.net || b.side != a.side || b.drill.is_some() || groups[i] == groups[j] {
                continue;
            }
            pairs.push((i, j));
        }
    }
    let mut put = 0;
    for (i, j) in pairs {
        let Some((a, b, w)) = strap(board, i, j) else { continue };
        // nothing but the pair itself may sit between them: a pad of this part's next pin on the line is a pad to join first
        if between(board, i, j) {
            continue;
        }
        let before = drc::check(board).len();
        let mut trial = board.clone();
        trial.tracks.push(Track { side: board.pads[i].side, a, b, width: w.max(board.rules.min_track_width) });
        if drc::check(&trial).len() <= before && trial_joins(&trial, i, j) {
            *board = trial;
            put += 1;
        }
    }
    put
}

/// The ends and width of the strap between pads `i` and `j`, if they are lined up and close enough.
fn strap(board: &Board, i: usize, j: usize) -> Option<(Point, Point, f64)> {
    let (a, b) = (&board.pads[i], &board.pads[j]);
    let (wa, ha) = a.shape.size();
    let (wb, hb) = b.shape.size();
    let (dx, dy) = ((b.pos.x - a.pos.x).abs(), (b.pos.y - a.pos.y).abs());
    // along y (same x) or along x (same y)
    let (gap, perp, along) = if dx < LINED_UP {
        (dy - (ha + hb) / 2.0, wa.min(wb), ha.min(hb))
    } else if dy < LINED_UP {
        (dx - (wa + wb) / 2.0, ha.min(hb), wa.min(wb))
    } else {
        return None;
    };
    if gap <= 0.0 || gap > MAX_GAP {
        return None;
    }
    Some((a.pos, b.pos, perp.min(along)))
}

/// Is another pad of the same part on the straight line from pad `i` to pad `j`?
fn between(board: &Board, i: usize, j: usize) -> bool {
    let (a, b) = (board.pads[i].pos, board.pads[j].pos);
    board.pads.iter().enumerate().any(|(k, p)| {
        k != i && k != j && p.refdes == board.pads[i].refdes && p.side == board.pads[i].side && drc::point_seg(p.pos, a, b) < p.shape.size().0.min(p.shape.size().1) / 2.0
    })
}

fn trial_joins(board: &Board, i: usize, j: usize) -> bool {
    let g = drc::pad_groups(board);
    g[i] == g[j]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(x: f64, y: f64, w: f64, h: f64, pin: &str, net: &str) -> Pad {
        let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w, h }, None, Side::Top);
        p.refdes = "U1".into();
        p.pin = pin.into();
        p.net = net.into();
        p
    }

    fn board(pads: Vec<Pad>) -> Board {
        let mut b = Board::new("t", 30.0, 30.0);
        b.pads = pads;
        b
    }

    #[test]
    fn two_neighbouring_pads_of_one_net_are_strapped_without_leaving_the_pads() {
        // VIN LX LX VIN down a row, 1.8 mm apart; the LX pads are 1.2 x 0.6
        let mut b = board(vec![pad(10.0, 10.0, 1.2, 0.6, "1", "VIN"), pad(10.0, 11.8, 1.2, 0.6, "2", "LX"), pad(10.0, 13.6, 1.2, 0.6, "3", "LX"), pad(10.0, 15.4, 1.2, 0.6, "4", "VIN")]);
        assert_eq!(bridge_pads(&mut b), 1);
        let t = &b.tracks[0];
        assert!((t.width - 0.6).abs() < 1e-9, "as wide as the pads' short side: {}", t.width);
        assert_eq!((t.a.y, t.b.y), (11.8, 13.6));
        assert!(drc::check(&b).is_empty());
        assert_eq!(crate::ratsnest::ratsnest(&b).iter().filter(|w| w.net == "LX").count(), 0, "the LX pads are one piece of copper");
    }

    #[test]
    fn pads_that_are_not_lined_up_far_apart_or_poured_are_left_alone() {
        let mut b = board(vec![pad(10.0, 10.0, 1.2, 0.6, "1", "LX"), pad(10.4, 11.8, 1.2, 0.6, "2", "LX")]);
        assert_eq!(bridge_pads(&mut b), 0, "not lined up");
        let mut b = board(vec![pad(10.0, 10.0, 1.2, 0.6, "1", "LX"), pad(10.0, 16.0, 1.2, 0.6, "2", "LX")]);
        assert_eq!(bridge_pads(&mut b), 0, "too far apart");
        let mut b = board(vec![pad(10.0, 10.0, 1.2, 0.6, "1", "GND"), pad(10.0, 11.8, 1.2, 0.6, "2", "GND")]);
        assert_eq!(bridge_pads(&mut b), 0, "ground is a pour's business");
    }

    #[test]
    fn a_pad_of_the_same_part_in_between_is_not_jumped() {
        let mut b = board(vec![pad(10.0, 10.0, 1.2, 0.6, "1", "LX"), pad(10.0, 11.8, 1.2, 0.6, "2", "X"), pad(10.0, 13.6, 1.2, 0.6, "3", "LX")]);
        assert_eq!(bridge_pads(&mut b), 0);
    }
}
