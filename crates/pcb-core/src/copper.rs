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

/// A guard of tracks: along every track of `net` on the top layer, one track of width `width` on each side, `gap` mm from its edge,
/// joined at the corners, ending short of the pads (a pad end is a place where `net` leaves the track). Each run of guard tracks
/// gets one via of the guard net, so the run counts as copper of that net and the router can join it to the guard's source.
/// A track that the DRC would not accept (copper of another net in the way) is left out, so the guard has gaps there. Pours
/// do not do this job: the pour round a track is cut into pieces by its own track, and a piece that touches nothing of the guard
/// net is dropped as an island. Returns (tracks, vias).
pub fn add_guard_tracks(board: &mut Board, net: &str, guard: &str, gap: f64, width: f64) -> Result<(usize, usize), String> {
    if !board.pads.iter().any(|p| p.net == guard) {
        return Err(format!("{guard} につながるパッドがありません (ガードのネット名を確かめてください)"));
    }
    let conn = connectivity(board);
    let segs: Vec<(Point, Point, f64)> = board
        .tracks
        .iter()
        .enumerate()
        .filter(|(i, t)| t.side == Side::Top && conn.track_net(*i) == Some(net))
        .map(|(_, t)| (t.a, t.b, t.width))
        .filter(|(a, b, _)| a.dist(*b) > 1e-6)
        .collect();
    if segs.is_empty() {
        return Err(format!("{net} に表面の配線がありません。先に配線してください"));
    }
    let key = |p: Point| ((p.x * 100.0).round() as i64, (p.y * 100.0).round() as i64);
    let mut at: std::collections::HashMap<(i64, i64), Vec<usize>> = Default::default();
    for (i, (a, b, _)) in segs.iter().enumerate() {
        at.entry(key(*a)).or_default().push(i);
        at.entry(key(*b)).or_default().push(i);
    }
    // the offset line of segment i on side `sd` (+1 / -1): a point on it, the direction and the distance
    let offset = |i: usize, sd: f64| {
        let (a, b, w) = segs[i];
        let l = a.dist(b);
        let (ux, uy) = ((b.x - a.x) / l, (b.y - a.y) / l);
        let d = w / 2.0 + gap + width / 2.0;
        (Point::new(a.x - uy * sd * d, a.y + ux * sd * d), (ux, uy), d)
    };
    // ends of the guard track of (segment, side): [a end, b end], moved to the corner or pulled back from a pad
    let mut ends: Vec<[Point; 2]> = vec![];
    for i in 0..segs.len() {
        for sd in [1.0, -1.0] {
            let (p, (ux, uy), _) = offset(i, sd);
            let l = segs[i].0.dist(segs[i].1);
            ends.push([p, Point::new(p.x + ux * l, p.y + uy * l)]);
        }
    }
    let idx = |i: usize, sd: f64| i * 2 + if sd > 0.0 { 0 } else { 1 };
    let pull = 1.3;
    for (i, (a, b, _)) in segs.iter().enumerate() {
        for (end_k, v) in [(0usize, *a), (1usize, *b)] {
            let joined = at[&key(v)].clone();
            for sd in [1.0, -1.0] {
                let k = idx(i, sd);
                if joined.len() == 2 {
                    let j = if joined[0] == i { joined[1] } else { joined[0] };
                    // corner of two tracks: the offset lines meet
                    let (p1, d1, _) = offset(i, sd);
                    let (p2, d2, _) = offset(j, sd);
                    let den = d1.0 * d2.1 - d1.1 * d2.0;
                    if den.abs() > 1e-6 {
                        let t = ((p2.x - p1.x) * d2.1 - (p2.y - p1.y) * d2.0) / den;
                        let x = Point::new(p1.x + d1.0 * t, p1.y + d1.1 * t);
                        if x.dist(v) <= 4.0 * (segs[i].2 / 2.0 + gap + width / 2.0) {
                            ends[k][end_k] = x;
                        }
                    }
                    // straight on: the offset ends already meet
                } else {
                    // a pad, a branch or a free end: stop short of it, along the track
                    let other = if end_k == 0 { ends[k][1] } else { ends[k][0] };
                    let here = ends[k][end_k];
                    let l = here.dist(other);
                    if l > 1e-9 {
                        let f = (pull / l).min(1.0);
                        ends[k][end_k] = Point::new(here.x + (other.x - here.x) * f, here.y + (other.y - here.y) * f);
                    }
                }
            }
        }
    }
    let before = check(board).len();
    let mut work = board.clone();
    let mut added: Vec<Track> = vec![];
    for e in &ends {
        if e[0].dist(e[1]) < 0.2 {
            continue;
        }
        // a track that touches nothing has no net, and the DRC leaves it alone: for the test it starts on a tiny pad of the guard net,
        // so that its distance from the copper of the other nets is looked at
        let mut trial = work.clone();
        trial.tracks.push(Track { side: Side::Top, a: e[0], b: e[1], width });
        let mut probe = trial.clone();
        let mut dot = Pad::new(e[0], PadShape::Rect { w: 0.05, h: 0.05 }, None, Side::Top);
        dot.net = guard.to_string();
        probe.pads.push(dot);
        if check(&probe).len() <= before {
            added.push(trial.tracks.last().unwrap().clone());
            work = trial;
        }
    }
    if added.is_empty() {
        // nothing fits (copper of other nets is too near on both sides): the board stays as it is
        return Ok((0, 0));
    }
    // runs: guard tracks that share an end; one via of the guard net on each, at the end nearest a pad of the guard net
    let mut parent: Vec<usize> = (0..added.len()).collect();
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
        if p[x] != x {
            let r = find(p, p[x]);
            p[x] = r;
        }
        p[x]
    }
    for i in 0..added.len() {
        for j in 0..i {
            let close = [added[i].a, added[i].b].iter().any(|p| [added[j].a, added[j].b].iter().any(|q| p.dist(*q) < 0.05));
            if close {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                parent[ri] = rj;
            }
        }
    }
    let guard_pads: Vec<Point> = work.pads.iter().filter(|p| p.net == guard).map(|p| p.pos).collect();
    let mut vias = 0;
    let mut runs: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..added.len() {
        let r = find(&mut parent, i);
        runs.entry(r).or_default().push(i);
    }
    for (_, members) in runs {
        let mut spots: Vec<Point> = members.iter().flat_map(|&i| [added[i].a, added[i].b]).collect();
        spots.sort_by(|a, b| {
            let f = |p: &Point| guard_pads.iter().map(|g| g.dist(*p)).fold(f64::MAX, f64::min);
            f(a).total_cmp(&f(b))
        });
        for s in spots {
            let mut trial = work.clone();
            trial.vias.push(Via { pos: s, diameter: 0.6, drill: 0.3, net: guard.to_string() });
            if check(&trial).len() <= before {
                work = trial;
                vias += 1;
                break;
            }
        }
    }
    let n = added.len();
    *board = work;
    Ok((n, vias))
}
