//! Design-rule checks run before export.
//!
//! Copper clearance works on geometry only: copper items that touch on a layer
//! (directly, or through a via / through-hole pad) form one electrical group,
//! and only items in *different* groups must keep `min_clearance` apart.

use crate::geom::inside_polygon;
use crate::model::*;
use std::collections::HashMap;

mod acute;
mod area;
mod connectivity;
mod decoupling;
pub(crate) mod geo;
mod patch;
mod path;
mod pour;
mod silk;
mod slit;
#[cfg(test)]
mod tests;
mod thermal;

pub use acute::*;
pub use area::*;
pub use connectivity::*;
pub use decoupling::*;
pub use geo::*;
pub use patch::*;
pub use path::*;
pub use pour::*;
pub use silk::*;
pub use slit::*;
pub use thermal::*;

#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    pub message: String,
    pub at: Point,
}

fn v(message: String, at: Point) -> Violation {
    Violation { message, at }
}

// ---------------------------------------------------------------- geometry

fn side_name(s: Side) -> &'static str {
    match s {
        Side::Top => "表面",
        Side::Bottom => "裏面",
        Side::Inner(1) => "内層1",
        Side::Inner(2) => "内層2",
        Side::Inner(3) => "内層3",
        Side::Inner(_) => "内層",
    }
}

pub fn check(board: &Board) -> Vec<Violation> {
    let r = &board.rules;
    let mut out = vec![];

    if board.outline.len() < 3 {
        out.push(v("基板の外形が3点未満で、面になっていません。「基板外形」ツールか「矩形に設定」で外形を決めましょう".into(), Point::default()));
        return out;
    }

    for t in &board.tracks {
        if t.width + 1e-9 < r.min_track_width {
            out.push(v(format!("配線の幅 {:.3}mm が最小 {:.3}mm より細いです。配線を太くするか、発注先の最小幅に合わせたルールを選びましょう", t.width, r.min_track_width), t.a));
        }
        for p in [t.a, t.b] {
            if !inside_polygon(&board.outline, p) {
                out.push(v("配線の端が基板の外にあります。基板の内側に引き直すか、外形を広げましょう".into(), p));
            }
        }
    }

    let mut drills: Vec<(Point, f64, f64)> = vec![];
    for p in &board.pads {
        if let Some(d) = p.drill {
            let (w, h) = p.shape.size();
            drills.push((p.pos, d, (w.min(h) - d) / 2.0));
        }
    }
    for via in &board.vias {
        drills.push((via.pos, via.drill, (via.diameter - via.drill) / 2.0));
    }
    for (pos, d, ring) in &drills {
        if *d + 1e-9 < r.min_drill {
            out.push(v(format!("穴の径 {:.3}mm が最小 {:.3}mm より小さいです。穴を大きくするか、発注先に合ったルールを選びましょう", d, r.min_drill), *pos));
        }
        if *ring + 1e-9 < r.min_annular_ring {
            out.push(v(format!("穴のまわりの銅 (アニュラリング) {:.3}mm が最小 {:.3}mm より細いです。パッド/ビアの直径を大きくするか、穴を小さくしましょう", ring, r.min_annular_ring), *pos));
        }
        if !inside_polygon(&board.outline, *pos) {
            out.push(v("穴が基板の外にあります。基板の内側に動かすか、外形を広げましょう".into(), *pos));
        }
    }
    for h in &board.holes {
        let (a, b) = h.ends();
        if !(inside_polygon(&board.outline, h.pos) && inside_polygon(&board.outline, a) && inside_polygon(&board.outline, b)) {
            out.push(v("取り付け穴が基板の外にあります。基板の内側に動かすか、外形を広げましょう".into(), h.pos));
        }
    }

    for z in &board.zones {
        if !z.has_area() {
            out.push(v(format!("ゾーン({})の頂点が3点未満です (面になる形がありません)", z.net), Point::default()));
            continue;
        }
        // The copper is cut to the outline (zonefill), so sticking out is harmless (e.g. after the
        // outline was shrunk); only a pour that shares no point with the board is reported.
        let region = z.region();
        let touches = region.iter().any(|piece| piece.outer.iter().any(|p| inside_polygon(&board.outline, *p)) || board.outline.iter().any(|p| inside_polygon(&piece.outer, *p)));
        if !touches && board.outline.len() >= 3 {
            let at = region.first().and_then(|piece| piece.outer.first().copied()).unwrap_or_default();
            out.push(v(format!("ゾーン({})が基板外形の外にあり、銅が残りません。ゾーンを基板の上に描き直すか、外形を広げましょう", z.net), at));
        }
    }

    out.extend(clearance(board));
    out.extend(shorts(board));
    out
}

/// Copper-to-copper and copper-to-board-edge clearance.
pub fn clearance(board: &Board) -> Vec<Violation> {
    let r = &board.rules;
    let items = copper_items(board);
    let conn = connectivity(board);
    let grp = conn.items.clone();
    let mut out = vec![];

    // only the pairs whose boxes come within the clearance of each other can break it; in the order of the item numbers
    // (the clearance of a net class can be more than the board's rule)
    let mut near: Vec<(usize, usize)> = vec![];
    near_pairs(&items, board.max_clearance(), |i, j| near.push((i.min(j), i.max(j))));
    near.sort_unstable();
    for side in board.copper_layers() {
        for &(i, j) in &near {
            if !(items[i].on(side) && items[j].on(side)) || grp[i] == grp[j] {
                continue;
            }
            let d = dist(&items[i].geo, &items[j].geo);
            let need = board.clearance_between(conn.group_net(grp[i]), conn.group_net(grp[j]));
            if d + 1e-9 < need {
                let (p, q) = (center(&items[i].geo), center(&items[j].geo));
                let why = if need > r.min_clearance + 1e-9 { " (ネットクラスで、広い間隔を決めたネットです)" } else { "" };
                out.push(v(
                    format!("{} 銅箔どうしの間隔 {:.3}mm が最小 {:.3}mm より狭いです。配線やパッドを離しましょう (別のネットが近づきすぎ){}", side_name(side), d, need, why),
                    Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0),
                ));
            }
        }
    }

    let n = board.outline.len();
    for it in &items {
        for e in 0..n {
            let edge = Geo::Cap { a: board.outline[e], b: board.outline[(e + 1) % n], r: 0.0 };
            let d = dist(&it.geo, &edge);
            if d + 1e-9 < r.edge_clearance {
                out.push(v(format!("銅箔と基板の端の間隔 {:.3}mm が最小 {:.3}mm より狭いです。端から離しましょう (切り出しで銅が欠けたり短絡したりします)", d, r.edge_clearance), center(&it.geo)));
                break;
            }
        }
    }
    out
}

/// Pads of different nets that end up in one copper group.
pub fn shorts(board: &Board) -> Vec<Violation> {
    let conn = connectivity(board);
    let mut seen: std::collections::BTreeMap<usize, (Vec<String>, Point)> = Default::default();
    for (i, p) in board.pads.iter().enumerate() {
        if p.net.is_empty() {
            continue;
        }
        let e = seen.entry(conn.pad_group(i)).or_insert_with(|| (vec![], p.pos));
        if !e.0.contains(&p.net) {
            e.0.push(p.net.clone());
        }
    }
    // a via placed for a net that sits on copper of another net
    for (k, v) in board.vias.iter().enumerate() {
        if v.net.is_empty() {
            continue;
        }
        let e = seen.entry(conn.via_group(k)).or_insert_with(|| (vec![], v.pos));
        if !e.0.contains(&v.net) {
            e.0.push(v.net.clone());
        }
    }
    seen.into_values()
        .filter(|(nets, _)| nets.len() > 1)
        .map(|(nets, at)| v(format!("ネット短絡: {}", nets.join(" と ")), at))
        .collect()
}

/// How many findings the copper just added to `board` (the tracks from `tracks_before` and the vias from `vias_before`
/// on, `conn` being the [`connectivity`] of `board` as it is) is responsible for: what [`check`] reports about those
/// items, found without going over every pair of the board again. A board that was clean before has `check(board)`
/// findings exactly when this is not 0; the router uses it to take a route or not.
pub fn violations_of_added(board: &Board, tracks_before: usize, vias_before: usize, conn: &Connectivity) -> usize {
    let tracks: Vec<usize> = (tracks_before..board.tracks.len()).collect();
    let vias: Vec<usize> = (vias_before..board.vias.len()).collect();
    violations_of_items(board, &tracks, &vias, conn)
}

/// How many findings the tracks `changed_tracks` and the vias `changed_vias` (numbers in `board.tracks` and
/// `board.vias`) are responsible for, `conn` being the [`connectivity`] of `board`: what [`check`] reports about those
/// items (the width, the ends, the holes and rings, the clearance to other copper and to the edge, a short), found
/// without going over every pair of the board. On a board that was clean without these changes, `check(board)` has
/// findings exactly when this is not 0.
pub fn violations_of_items(board: &Board, changed_tracks: &[usize], changed_vias: &[usize], conn: &Connectivity) -> usize {
    let r = &board.rules;
    let mut n = 0;
    for &k in changed_tracks {
        let t = &board.tracks[k];
        if t.width + 1e-9 < r.min_track_width {
            n += 1;
        }
        n += [t.a, t.b].iter().filter(|p| !inside_polygon(&board.outline, **p)).count();
    }
    for &k in changed_vias {
        let via = &board.vias[k];
        if via.drill + 1e-9 < r.min_drill {
            n += 1;
        }
        if (via.diameter - via.drill) / 2.0 + 1e-9 < r.min_annular_ring {
            n += 1;
        }
        if !inside_polygon(&board.outline, via.pos) {
            n += 1;
        }
    }
    let items = copper_items(board);
    let first_via = conn.n_tracks + conn.n_pads;
    let mut is_new = vec![false; items.len()];
    for &k in changed_tracks {
        is_new[k] = true;
    }
    for &k in changed_vias {
        is_new[first_via + k] = true;
    }
    let new: Vec<usize> = (0..items.len()).filter(|&i| is_new[i]).collect();
    // copper of different groups too close
    for side in board.copper_layers() {
        for &i in new.iter().filter(|&&i| items[i].on(side)) {
            for j in (0..items.len()).filter(|&j| j != i && items[j].on(side) && conn.items[i] != conn.items[j]) {
                if is_new[j] && j < i {
                    continue; // a pair of new items is counted once
                }
                if dist(&items[i].geo, &items[j].geo) + 1e-9 < board.clearance_between(conn.group_net(conn.items[i]), conn.group_net(conn.items[j])) {
                    n += 1;
                }
            }
        }
    }
    // copper too close to the board's edge
    let m = board.outline.len();
    for &i in &new {
        let near = (0..m).any(|e| dist(&items[i].geo, &Geo::Cap { a: board.outline[e], b: board.outline[(e + 1) % m], r: 0.0 }) + 1e-9 < r.edge_clearance);
        n += usize::from(near);
    }
    // pads of different nets in the groups of the new copper
    let groups: std::collections::HashSet<usize> = new.iter().map(|&i| conn.items[i]).collect();
    let mut nets: HashMap<usize, Vec<&str>> = HashMap::new();
    for (k, p) in board.pads.iter().enumerate().filter(|(_, p)| !p.net.is_empty()) {
        let g = conn.pad_group(k);
        if groups.contains(&g) {
            let e = nets.entry(g).or_default();
            if !e.contains(&p.net.as_str()) {
                e.push(&p.net);
            }
        }
    }
    n + nets.values().filter(|v| v.len() > 1).count()
}
