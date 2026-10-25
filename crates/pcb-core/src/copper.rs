//! Copper added by net, after the routing: a guard of copper along a sensitive net, and a patch of copper for a net that
//! needs the area (the drain of a power transistor).
//!
//! A guard is a pour of the guard net (a follower output that has the sensitive net's own voltage) laid round every
//! track and pad of the sensitive net: it fills the room beside the track, keeps the pour's clearance to it, and drops the
//! pieces that touch nothing of its own net. Surface leakage between the sensitive net and the rails then has no
//! voltage across it. Nothing is changed when the DRC would find more than before.

use crate::drc::{check, connectivity};
use crate::model::*;
use crate::zonefill::merge_zone_outline;

/// Four corners of the rectangle round the segment a-b, `margin` mm beyond its sides and its ends.
fn around_segment(a: Point, b: Point, half_w: f64, margin: f64) -> Vec<Point> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l = (dx * dx + dy * dy).sqrt();
    let (ux, uy) = if l < 1e-9 { (1.0, 0.0) } else { (dx / l, dy / l) };
    let (nx, ny) = (-uy, ux);
    let h = half_w + margin;
    let (a2, b2) = (Point::new(a.x - ux * margin, a.y - uy * margin), Point::new(b.x + ux * margin, b.y + uy * margin));
    vec![
        Point::new(a2.x + nx * h, a2.y + ny * h),
        Point::new(b2.x + nx * h, b2.y + ny * h),
        Point::new(b2.x - nx * h, b2.y - ny * h),
        Point::new(a2.x - nx * h, a2.y - ny * h),
    ]
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
    vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)]
}

fn merge_all(board: &mut Board, from: usize) {
    let mut i = board.zones.len();
    while i > from {
        i -= 1;
        if i < board.zones.len() {
            merge_zone_outline(board, i);
        }
    }
}

/// Put a pour of `guard` along every track and pad of `net` (`margin` mm wide beside them, `clearance` mm from `net`) on the
/// layers of its tracks. The guard net's own tracks and pads are covered too, so the pour joins them. Returns the number of
/// pours added (after joining).
pub fn add_guard(board: &mut Board, net: &str, guard: &str, margin: f64, clearance: f64) -> Result<usize, String> {
    if !board.pads.iter().any(|p| p.net == guard) {
        return Err(format!("{guard} につながるパッドがありません (ガードのネット名を確かめてください)"));
    }
    let conn = connectivity(board);
    let mut shapes: Vec<(Side, Vec<Point>)> = vec![];
    for (k, t) in board.tracks.iter().enumerate() {
        match conn.track_net(k) {
            Some(n) if n == net => shapes.push((t.side, around_segment(t.a, t.b, t.width / 2.0, margin))),
            Some(n) if n == guard => shapes.push((t.side, around_segment(t.a, t.b, t.width / 2.0, 0.3))),
            _ => {}
        }
    }
    let sides: Vec<Side> = shapes.iter().map(|s| s.0).collect();
    for p in board.pads.iter().filter(|p| p.net == net || p.net == guard) {
        let (w, h) = p.shape.size();
        let m = if p.net == net { margin } else { 0.3 };
        if p.drill.is_some() {
            continue; // a through-hole pad reaches every layer: the guard goes round the tracks only
        }
        if sides.contains(&p.side) {
            shapes.push((p.side, rect(p.pos.x - w / 2.0 - m, p.pos.y - h / 2.0 - m, p.pos.x + w / 2.0 + m, p.pos.y + h / 2.0 + m)));
        }
    }
    if shapes.is_empty() {
        return Err(format!("{net} に配線がありません。先に配線してください"));
    }
    let before = check(board).len();
    let mut work = board.clone();
    let first = work.zones.len();
    for (side, poly) in shapes {
        let mut z = Zone::new(guard, side, poly);
        z.clearance = clearance;
        z.remove_islands = true;
        work.zones.push(z);
    }
    merge_all(&mut work, first);
    // a guard pour that kept no copper (a layer that only had a stray track) is not left behind: it would only cost time
    let conn = connectivity(&work);
    let empty: Vec<bool> = (0..work.zones.len()).map(|i| i >= first && crate::zonefill::zone_fill(&work, &work.zones[i], &conn).is_empty()).collect();
    let mut k = 0;
    work.zones.retain(|_| {
        k += 1;
        !empty[k - 1]
    });
    if check(&work).len() > before {
        return Err("ガードの銅を置くと、ルール違反が増えるので、置きませんでした".into());
    }
    let added = work.zones.len() - first;
    *board = work;
    Ok(added)
}

/// A patch of copper of `net` over the rectangle (x0, y0, x1, y1) on each of `sides`, and up to `vias` vias of the net that join
/// the layers (the first places, from the middle outwards, where the DRC finds nothing more). Returns (pours, vias).
pub fn add_copper(board: &mut Board, net: &str, area: (f64, f64, f64, f64), sides: &[Side], vias: usize) -> Result<(usize, usize), String> {
    if !board.pads.iter().any(|p| p.net == net) {
        return Err(format!("{net} につながるパッドがありません"));
    }
    let (x0, y0, x1, y1) = area;
    if x1 <= x0 || y1 <= y0 {
        return Err("rect=x0,y0,x1,y1 (x1 > x0 かつ y1 > y0)".into());
    }
    let before = check(board).len();
    let mut work = board.clone();
    for &s in sides {
        let mut z = Zone::new(net, s, rect(x0, y0, x1, y1));
        z.remove_islands = true;
        work.zones.push(z);
    }
    let mut put = 0;
    if sides.len() > 1 && vias > 0 {
        // the vias go first beside the pads of the net that are in the patch (the heat comes from there), else round the middle
        let mut near: Vec<Point> = work.pads.iter().filter(|p| p.net == net && p.pos.x >= x0 && p.pos.x <= x1 && p.pos.y >= y0 && p.pos.y <= y1).map(|p| p.pos).collect();
        if near.is_empty() {
            near.push(Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0));
        }
        let key = |q: &Point| near.iter().map(|n| (n.x - q.x).powi(2) + (n.y - q.y).powi(2)).fold(f64::MAX, f64::min);
        let mut spots: Vec<Point> = vec![];
        let mut y = y0 + 1.0;
        while y <= y1 - 1.0 {
            let mut x = x0 + 1.0;
            while x <= x1 - 1.0 {
                spots.push(Point::new(x, y));
                x += 2.0;
            }
            y += 2.0;
        }
        spots.sort_by(|a, b| key(a).total_cmp(&key(b)));
        for s in spots {
            if put >= vias {
                break;
            }
            let mut trial = work.clone();
            trial.vias.push(Via { pos: s, diameter: 0.6, drill: 0.3, net: net.to_string() });
            if check(&trial).len() <= before {
                work = trial;
                put += 1;
            }
        }
    }
    if check(&work).len() > before {
        return Err("銅を置くと、ルール違反が増えるので、置きませんでした".into());
    }
    let pours = sides.len();
    *board = work;
    Ok((pours, put))
}
