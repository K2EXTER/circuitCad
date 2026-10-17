//! Unrouted connections ("airwires"): for every net, the minimum set of straight lines
//! that would join its still-disconnected copper groups.

use crate::drc::pad_groups;
use crate::model::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Airwire {
    pub net: String,
    pub a: Point,
    pub b: Point,
}

pub fn ratsnest(board: &Board) -> Vec<Airwire> {
    ratsnest_in(board, &pad_groups(board))
}

/// [`ratsnest`] with the copper groups of the pads (`grp[i]` for `board.pads[i]`) already known.
pub fn ratsnest_in(board: &Board, grp: &[usize]) -> Vec<Airwire> {
    let mut by_net: BTreeMap<&str, Vec<(Point, usize)>> = BTreeMap::new();
    for (p, g) in board.pads.iter().zip(grp) {
        if !p.net.is_empty() {
            by_net.entry(&p.net).or_default().push((p.pos, *g));
        }
    }

    let mut out = vec![];
    for (net, pads) in by_net {
        // Prim's algorithm over copper groups, using the closest pad pair as edge length
        let mut connected: Vec<usize> = vec![pads[0].1];
        loop {
            let mut best: Option<(f64, Point, Point, usize)> = None;
            for &(pa, ga) in pads.iter().filter(|p| connected.contains(&p.1)) {
                for &(pb, gb) in pads.iter().filter(|p| !connected.contains(&p.1)) {
                    let d = pa.dist(pb);
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, pa, pb, gb));
                    }
                }
                let _ = ga;
            }
            let Some((_, a, b, g)) = best else { break };
            out.push(Airwire { net: net.to_string(), a, b });
            connected.push(g);
        }
    }
    out
}
