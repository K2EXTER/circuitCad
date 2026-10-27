//! Tidy up routed tracks: two tracks that meet end to end and run in the same direction become one track.
//!
//! The router and `padentry` cut a route into short pieces (grid steps, the straight lead out of a pad). A joint is
//! merged only when nothing else is there: exactly two track ends on the same side and of the same width, no pad
//! or via at the point, and the two tracks form one straight line. The copper does not change (same shape, same
//! connections, same DRC result); there are just fewer tracks to draw, select and export.

use crate::model::*;
use std::collections::HashMap;

/// Merge collinear joints among the tracks with index `>= from` (a joint touched by an older track is left alone).
/// Returns the number of tracks removed.
pub fn merge_collinear(board: &mut Board, from: usize) -> usize {
    let mut removed = remove_covered(board, from);
    loop {
        let n = merge_pass(board, from);
        if n == 0 {
            return removed;
        }
        removed += n;
    }
}

/// Take out the tracks (index `>= from`) that lie entirely inside another track of the same layer that is as wide or wider: a stub that
/// goes out and comes back, or two tracks drawn over one another. The copper is the same, and so are the connections (an end that
/// lay on the covered track lies on the covering one). Of two identical tracks the later one goes. Returns how many were removed.
pub fn remove_covered(board: &mut Board, from: usize) -> usize {
    const TOL: f64 = 1e-6;
    let n = board.tracks.len();
    let mut dead = vec![false; n];
    for i in from..n {
        let a = &board.tracks[i];
        for j in 0..n {
            if i == j || dead[j] {
                continue;
            }
            let b = &board.tracks[j];
            if b.side != a.side || b.width + TOL < a.width {
                continue;
            }
            // both ends of `a` on `b` (a is inside b)
            if crate::drc::point_seg(a.a, b.a, b.b) > TOL || crate::drc::point_seg(a.b, b.a, b.b) > TOL {
                continue;
            }
            // two identical tracks: the earlier one stays
            let same = b.width - a.width < TOL && crate::drc::point_seg(b.a, a.a, a.b) <= TOL && crate::drc::point_seg(b.b, a.a, a.b) <= TOL;
            if same && j > i {
                continue;
            }
            dead[i] = true;
            break;
        }
    }
    let removed = dead.iter().filter(|d| **d).count();
    if removed > 0 {
        let mut k = 0;
        board.tracks.retain(|_| {
            k += 1;
            !dead[k - 1]
        });
    }
    removed
}

fn key(p: Point, side: Side) -> (i64, i64, Side) {
    ((p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64, side)
}

/// A pad or via covers `p` (on `side`): the joint is a connection point, so it stays.
fn covered(board: &Board, p: Point, side: Side) -> bool {
    const TOL: f64 = 0.01;
    let on_pad = board.pads.iter().any(|pad| {
        let (w, h) = pad.shape.size();
        (pad.drill.is_some() || pad.side == side) && (p.x - pad.pos.x).abs() <= w / 2.0 + TOL && (p.y - pad.pos.y).abs() <= h / 2.0 + TOL
    });
    on_pad || board.vias.iter().any(|v| p.dist(v.pos) <= v.diameter / 2.0 + TOL)
}

fn merge_pass(board: &mut Board, from: usize) -> usize {
    let mut ends: HashMap<(i64, i64, Side), Vec<(usize, bool)>> = HashMap::new();
    for (i, t) in board.tracks.iter().enumerate() {
        ends.entry(key(t.a, t.side)).or_default().push((i, true));
        ends.entry(key(t.b, t.side)).or_default().push((i, false));
    }
    let mut dead = vec![false; board.tracks.len()];
    let mut touched = vec![false; board.tracks.len()]; // changed in this pass: its ends in `ends` are stale
    let mut merged = 0;
    // by track number, so that the result does not depend on the hash order
    let mut lists: Vec<Vec<(usize, bool)>> = ends.into_values().collect();
    lists.sort_by_key(|l| l[0]);
    for list in &lists {
        let [(i, ia), (j, ja)] = list[..] else { continue };
        if i == j || i < from || j < from || touched[i] || touched[j] {
            continue;
        }
        let (ti, tj) = (&board.tracks[i], &board.tracks[j]);
        if ti.width != tj.width {
            continue;
        }
        let joint = if ia { ti.a } else { ti.b };
        let (far_i, far_j) = (if ia { ti.b } else { ti.a }, if ja { tj.b } else { tj.a });
        let (u, v) = ((far_i.x - joint.x, far_i.y - joint.y), (far_j.x - joint.x, far_j.y - joint.y));
        let (lu, lv) = (u.0.hypot(u.1), v.0.hypot(v.1));
        if lu < 1e-6 || lv < 1e-6 {
            continue;
        }
        // opposite directions from the joint, on one line (the sine of the angle between them is ~0)
        let cross = (u.0 * v.1 - u.1 * v.0) / (lu * lv);
        let dot = (u.0 * v.0 + u.1 * v.1) / (lu * lv);
        if cross.abs() > 1e-6 || dot > 0.0 || covered(board, joint, ti.side) {
            continue;
        }
        board.tracks[i] = Track { side: ti.side, a: far_i, b: far_j, width: ti.width };
        dead[j] = true;
        touched[i] = true;
        touched[j] = true;
        merged += 1;
    }
    if merged > 0 {
        let mut k = 0;
        board.tracks.retain(|_| {
            k += 1;
            !dead[k - 1]
        });
    }
    merged
}
