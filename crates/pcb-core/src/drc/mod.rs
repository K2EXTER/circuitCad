//! Design-rule checks run before export.
//!
//! Copper clearance works on geometry only: copper items that touch on a layer
//! (directly, or through a via / through-hole pad) form one electrical group,
//! and only items in *different* groups must keep `min_clearance` apart.

use crate::geom::inside_polygon;
use crate::model::*;
use std::collections::HashMap;

mod acute;
mod connectivity;
mod decoupling;
mod geo;
mod pour;
mod silk;
#[cfg(test)]
mod tests;
mod thermal;

pub use acute::*;
pub use connectivity::*;
pub use decoupling::*;
pub use geo::*;
pub use pour::*;
pub use silk::*;
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
        if z.polygon.len() < 3 {
            out.push(v(format!("ゾーン({})の頂点が3点未満です", z.net), Point::default()));
            continue;
        }
        for p in &z.polygon {
            if !inside_polygon(&board.outline, *p) {
                out.push(v(format!("ゾーン({})が基板外形の外にあります", z.net), *p));
                break;
            }
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
    let grp = connectivity(board).items;
    let mut out = vec![];

    for side in board.copper_layers() {
        let on: Vec<usize> = (0..items.len()).filter(|&i| items[i].on(side)).collect();
        for (k, &i) in on.iter().enumerate() {
            for &j in &on[k + 1..] {
                if grp[i] == grp[j] {
                    continue;
                }
                let d = dist(&items[i].geo, &items[j].geo);
                if d + 1e-9 < r.min_clearance {
                    let (p, q) = (center(&items[i].geo), center(&items[j].geo));
                    out.push(v(
                        format!("{} 銅箔どうしの間隔 {:.3}mm が最小 {:.3}mm より狭いです。配線やパッドを離しましょう (別のネットが近づきすぎ)", side_name(side), d, r.min_clearance),
                        Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0),
                    ));
                }
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
    let grp = pad_groups(board);
    let mut seen: std::collections::BTreeMap<usize, (Vec<String>, Point)> = Default::default();
    for (p, g) in board.pads.iter().zip(grp) {
        if p.net.is_empty() {
            continue;
        }
        let e = seen.entry(g).or_insert_with(|| (vec![], p.pos));
        if !e.0.contains(&p.net) {
            e.0.push(p.net.clone());
        }
    }
    seen.into_values()
        .filter(|(nets, _)| nets.len() > 1)
        .map(|(nets, at)| v(format!("ネット短絡: {}", nets.join(" と ")), at))
        .collect()
}
