//! Unrouted connections ("airwires"): for every net, the minimum set of straight lines
//! that would join its still-disconnected copper groups.

use crate::model::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Airwire {
    pub net: String,
    pub a: Point,
    pub b: Point,
}

/// How many pairs of airwires of different nets cross each other: the tangle one sees on an unrouted board.
/// Ground nets are left out (a pour carries them).
pub fn crossings(board: &Board) -> usize {
    let wires: Vec<Airwire> = ratsnest(board).into_iter().filter(|w| !crate::drc::is_ground_net(&w.net)).collect();
    let side = |o: Point, a: Point, b: Point| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut n = 0;
    for (i, p) in wires.iter().enumerate() {
        for q in &wires[i + 1..] {
            if p.net == q.net {
                continue;
            }
            let (d1, d2) = (side(p.a, p.b, q.a), side(p.a, p.b, q.b));
            let (d3, d4) = (side(q.a, q.b, p.a), side(q.a, q.b, p.b));
            if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
                n += 1;
            }
        }
    }
    n
}

pub fn ratsnest(board: &Board) -> Vec<Airwire> {
    let conn = crate::drc::connectivity(board);
    let pads: Vec<usize> = (0..board.pads.len()).map(|i| conn.pad_group(i)).collect();
    let vias: Vec<usize> = (0..board.vias.len()).map(|k| conn.via_group(k)).collect();
    ratsnest_with_vias(board, &pads, &vias)
}

/// The connections the user sees as unrouted (the yellow lines): [`ratsnest`] plus one line for each pad a pour leaves out when it is cut into
/// pieces (its outline joins them, its copper does not). The screen, `ccad board status`, the pre-order check and the board picture all use
/// this one count; the routers work on [`ratsnest`] alone.
pub fn open_connections(board: &Board) -> Vec<Airwire> {
    let mut w = ratsnest(board);
    w.extend(crate::drc::pour_split_airwires(board));
    w
}

/// [`ratsnest`] with the copper groups of the pads (`grp[i]` for `board.pads[i]`) already known.
pub fn ratsnest_in(board: &Board, grp: &[usize]) -> Vec<Airwire> {
    ratsnest_with_vias(board, grp, &[])
}

/// [`ratsnest_in`] that also joins the vias placed for a net (`via_grp[k]` for `board.vias[k]`; none given: only pads).
fn ratsnest_with_vias(board: &Board, grp: &[usize], via_grp: &[usize]) -> Vec<Airwire> {
    let mut by_net: BTreeMap<&str, Vec<(Point, usize)>> = BTreeMap::new();
    for (p, g) in board.pads.iter().zip(grp) {
        if !p.net.is_empty() {
            by_net.entry(&p.net).or_default().push((p.pos, *g));
        }
    }
    for (v, g) in board.vias.iter().zip(via_grp) {
        if !v.net.is_empty() {
            by_net.entry(&v.net).or_default().push((v.pos, *g));
        }
    }

    let mut out = vec![];
    for (net, pads) in by_net {
        out.extend(join_groups(net, &pads));
    }
    out
}

/// The airwires of one net, from its points (position, copper group): Prim's algorithm over the copper groups, using
/// the closest pair of points as the length of an edge.
pub(crate) fn join_groups(net: &str, pads: &[(Point, usize)]) -> Vec<Airwire> {
    let mut out = vec![];
    let Some(first) = pads.first() else { return out };
    let mut connected: Vec<usize> = vec![first.1];
    loop {
        let mut best: Option<(f64, Point, Point, usize)> = None;
        for &(pa, _) in pads.iter().filter(|p| connected.contains(&p.1)) {
            for &(pb, gb) in pads.iter().filter(|p| !connected.contains(&p.1)) {
                let d = pa.dist(pb);
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, pa, pb, gb));
                }
            }
        }
        let Some((_, a, b, g)) = best else { break };
        out.push(Airwire { net: net.to_string(), a, b });
        connected.push(g);
    }
    out
}
