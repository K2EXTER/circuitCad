//! Push-and-shove editing of tracks: when a new or moved track comes too close to the track of another net,
//! that track is pushed aside (and the tracks joined to it stretch along), instead of leaving a clearance error.
//!
//! The shove is conservative: pads and vias never move, a track pinned at both ends is bent aside (see `dogleg`), a track
//! is moved by at most [`MAX_SHOVE`] mm, and the result is used only if it breaks fewer rules than leaving
//! things where they are (and never cuts a connection). Otherwise the board is left as the plain edit made it.

use crate::drc::{check, connectivity, seg_seg};
use crate::model::*;
use crate::ratsnest::ratsnest;

/// How far (mm) a track may be pushed from where it was.
pub const MAX_SHOVE: f64 = 4.0;

fn closest(p: Point, a: Point, b: Point) -> Point {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-18 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0) };
    Point::new(a.x + dx * t, a.y + dy * t)
}

/// The unit direction from the segment `m` towards the segment `o` at their closest approach.
fn away(m: &Track, o: &Track) -> (f64, f64) {
    let mut best: Option<(f64, Point, Point)> = None;
    for (p, on_o) in [(o.a, true), (o.b, true), (m.a, false), (m.b, false)] {
        let (q, pm, po) = if on_o {
            let q = closest(p, m.a, m.b);
            (p.dist(q), q, p)
        } else {
            let q = closest(p, o.a, o.b);
            (p.dist(q), p, q)
        };
        if best.is_none_or(|b| q < b.0) {
            best = Some((q, pm, po));
        }
    }
    let (d, pm, po) = best.unwrap();
    if d > 1e-9 {
        return ((po.x - pm.x) / d, (po.y - pm.y) / d);
    }
    // crossing or touching: push sideways, towards the side of `o`'s middle
    let (dx, dy) = (m.b.x - m.a.x, m.b.y - m.a.y);
    let l = dx.hypot(dy).max(1e-9);
    let (nx, ny) = (-dy / l, dx / l);
    let mid = Point::new((o.a.x + o.b.x) / 2.0, (o.a.y + o.b.y) / 2.0);
    let side = (mid.x - m.a.x) * nx + (mid.y - m.a.y) * ny;
    if side >= 0.0 { (nx, ny) } else { (-nx, -ny) }
}

/// Is a track end held in place by a pad or a via?
pub(crate) fn pinned(board: &Board, p: Point, side: Side) -> bool {
    board.pads.iter().any(|q| {
        let (w, h) = q.shape.size();
        (q.drill.is_some() || q.side == side) && (p.x - q.pos.x).abs() <= w / 2.0 + 1e-6 && (p.y - q.pos.y).abs() <= h / 2.0 + 1e-6
    }) || board.vias.iter().any(|v| v.pos.dist(p) <= v.diameter / 2.0 + 1e-6)
}

fn same(a: Point, b: Point) -> bool {
    a.dist(b) < 1e-6
}

/// Split a track whose both ends are held into five (two straight ends, two 45° ramps and a bulge) so that its
/// middle sits `need - d` further from the obstacle. `d` is the distance to the obstacle now, `need` the distance it
/// must have, `away` the direction from the obstacle towards the track. `None` when the track is too short for the
/// ramps, the obstacle is end-on, or the bulge would exceed [`MAX_SHOVE`].
fn dogleg(t: &Track, m: &Track, d: f64, need: f64, away: (f64, f64)) -> Option<Vec<Track>> {
    let len = t.a.dist(t.b);
    if len < 1e-6 {
        return None;
    }
    let (dx, dy) = ((t.b.x - t.a.x) / len, (t.b.y - t.a.y) / len);
    let (mut nx, mut ny) = (-dy, dx);
    let side = nx * away.0 + ny * away.1;
    if side.abs() < 0.5 {
        return None;
    }
    if side < 0.0 {
        (nx, ny) = (-nx, -ny);
    }
    let s = need - d + 0.01;
    if s > MAX_SHOVE {
        return None;
    }
    // where the obstacle lies along the track
    let along = |p: Point| (p.x - t.a.x) * dx + (p.y - t.a.y) * dy;
    let (lo, hi) = (along(m.a).min(along(m.b)), along(m.a).max(along(m.b)));
    // a 45° ramp that starts here stays `need` away from the obstacle's end
    let reach = std::f64::consts::SQRT_2 * need - d - 0.01;
    let (t1, t2) = (lo - reach, hi + reach);
    const MIN_PIECE: f64 = 0.05;
    if t1 < MIN_PIECE || t2 > len - MIN_PIECE {
        return None;
    }
    let at = |u: f64, off: f64| Point::new(t.a.x + dx * u + nx * off, t.a.y + dy * u + ny * off);
    let pts = [t.a, at(t1, 0.0), at(t1 + s, s), at(t2 - s, s), at(t2, 0.0), t.b];
    Some(pts.windows(2).map(|w| Track { side: t.side, a: w[0], b: w[1], width: t.width }).collect())
}

/// Push the tracks that are too close to the `anchors` (indices of tracks that stay where they are) aside.
/// Returns the shoved board, or `None` when it cannot be done within the limits.
pub fn resolve(board: &Board, anchors: &[usize]) -> Option<Board> {
    let mut trial = board.clone();
    let mut origin: Vec<(Point, Point)> = trial.tracks.iter().map(|t| (t.a, t.b)).collect();
    let clearance = board.rules.min_clearance;
    let mut active: Vec<usize> = anchors.to_vec();
    for _ in 0..40 {
        let conn = connectivity(&trial);
        let mut next: Vec<usize> = vec![];
        let mut conflict = false;
        for &m in &active {
            for o in 0..trial.tracks.len() {
                if o == m || anchors.contains(&o) || trial.tracks[o].side != trial.tracks[m].side || conn.items[o] == conn.items[m] {
                    continue;
                }
                let (tm, to) = (trial.tracks[m].clone(), trial.tracks[o].clone());
                let need = clearance + (tm.width + to.width) / 2.0 + 0.005;
                let d = seg_seg(tm.a, tm.b, to.a, to.b);
                if d + 1e-9 >= need {
                    continue;
                }
                conflict = true;
                let (ux, uy) = away(&tm, &to);
                let s = (need - d + 0.01, ux, uy);
                let (fa, fb) = (pinned(&trial, to.a, to.side), pinned(&trial, to.b, to.side));
                if fa && fb {
                    // both ends are held: bend the middle aside instead
                    let pieces = dogleg(&to, &tm, d, need, (ux, uy))?;
                    origin[o] = (pieces[0].a, pieces[0].b);
                    trial.tracks[o] = pieces[0].clone();
                    for p in &pieces[1..] {
                        origin.push((p.a, p.b));
                        trial.tracks.push(p.clone());
                        next.push(trial.tracks.len() - 1);
                    }
                    if !next.contains(&o) {
                        next.push(o);
                    }
                    continue;
                }
                for (end, fixed) in [(to.a, fa), (to.b, fb)] {
                    if fixed {
                        continue;
                    }
                    let moved = Point::new(end.x + s.0 * s.1, end.y + s.0 * s.2);
                    // this end and every end of a joined track at the same spot go together
                    for k in 0..trial.tracks.len() {
                        if anchors.contains(&k) || trial.tracks[k].side != to.side {
                            continue;
                        }
                        let t = &mut trial.tracks[k];
                        let mut hit = false;
                        if same(t.a, end) {
                            t.a = moved;
                            hit = true;
                        }
                        if same(t.b, end) {
                            t.b = moved;
                            hit = true;
                        }
                        if hit && !next.contains(&k) {
                            next.push(k);
                        }
                    }
                }
                if !next.contains(&o) {
                    next.push(o);
                }
            }
        }
        for &k in &next {
            let (a0, b0) = origin.get(k).copied()?;
            let t = &trial.tracks[k];
            if t.a.dist(a0) > MAX_SHOVE || t.b.dist(b0) > MAX_SHOVE {
                return None;
            }
        }
        if !conflict {
            return Some(trial);
        }
        // the tracks that hold still are checked again too: a pushed track may still touch them elsewhere
        for &k in anchors {
            if !next.contains(&k) {
                next.push(k);
            }
        }
        active = next;
    }
    None
}

/// Number of tracks that differ between the two boards.
fn moved_count(a: &Board, b: &Board) -> usize {
    a.tracks.iter().zip(&b.tracks).filter(|(x, y)| !same(x.a, y.a) || !same(x.b, y.b)).count()
}

/// Add tracks, pushing the tracks of other nets aside where they are in the way. Returns how many existing
/// tracks were pushed (0 when nothing was in the way, or the shove was not possible and the tracks were added as they are).
pub fn add_tracks(board: &mut Board, new: Vec<Track>) -> usize {
    if new.is_empty() {
        return 0;
    }
    let first = board.tracks.len();
    let mut plain = board.clone();
    plain.tracks.extend(new);
    let anchors: Vec<usize> = (first..plain.tracks.len()).collect();
    let moved = accept(&plain, &anchors);
    match moved {
        Some((b, n)) => {
            *board = b;
            n
        }
        None => {
            *board = plain;
            0
        }
    }
}

/// The shoved board and the number of pushed tracks, if the shove is better than leaving `plain` as it is.
fn accept(plain: &Board, anchors: &[usize]) -> Option<(Board, usize)> {
    let shoved = resolve(plain, anchors)?;
    let n = moved_count(plain, &shoved);
    if n == 0 {
        return None;
    }
    // it must break fewer rules, and must not cut a connection
    if check(&shoved).len() >= check(plain).len() || ratsnest(&shoved).len() > ratsnest(plain).len() {
        return None;
    }
    Some((shoved, n))
}

/// Move one track by `(dx, dy)`. The tracks joined to its ends stretch along; an end that sits on a pad or a via
/// stays there and gets a short stub track to the moved end. Returns the indices of the tracks that must hold still
/// afterwards (the moved track and its stubs), or `None` when the track does not exist.
pub fn move_track(board: &mut Board, i: usize, dx: f64, dy: f64) -> Option<Vec<usize>> {
    let t = board.tracks.get(i)?.clone();
    let (na, nb) = (Point::new(t.a.x + dx, t.a.y + dy), Point::new(t.b.x + dx, t.b.y + dy));
    let mut stubs = vec![];
    for (old, new) in [(t.a, na), (t.b, nb)] {
        if pinned(board, old, t.side) {
            stubs.push(Track { side: t.side, a: old, b: new, width: t.width });
            continue;
        }
        for k in 0..board.tracks.len() {
            if k == i || board.tracks[k].side != t.side {
                continue;
            }
            if same(board.tracks[k].a, old) {
                board.tracks[k].a = new;
            }
            if same(board.tracks[k].b, old) {
                board.tracks[k].b = new;
            }
        }
    }
    board.tracks[i].a = na;
    board.tracks[i].b = nb;
    let mut held = vec![i];
    for s in stubs {
        if s.a.dist(s.b) > 1e-9 {
            board.tracks.push(s);
            held.push(board.tracks.len() - 1);
        }
    }
    Some(held)
}

/// Move one track and push what is in the way. Falls back to the plain move when the shove is not possible.
/// Returns the number of tracks pushed.
pub fn move_track_shoving(board: &mut Board, i: usize, dx: f64, dy: f64) -> usize {
    let mut plain = board.clone();
    let Some(held) = move_track(&mut plain, i, dx, dy) else { return 0 };
    match accept(&plain, &held) {
        Some((b, n)) => {
            *board = b;
            n
        }
        None => {
            *board = plain;
            0
        }
    }
}

/// Remove every track and via of the net that the track `i` belongs to (a net with several pads keeps its pads;
/// the airwires come back). Returns how many items were removed.
pub fn rip_up_net(board: &mut Board, net: &str) -> usize {
    let conn = connectivity(board);
    let tracks: Vec<bool> = (0..board.tracks.len()).map(|i| conn.track_net(i) == Some(net)).collect();
    let vias: Vec<bool> = (0..board.vias.len()).map(|i| conn.via_net(i) == Some(net)).collect();
    let n = tracks.iter().chain(&vias).filter(|x| **x).count();
    let mut k = 0;
    board.tracks.retain(|_| {
        k += 1;
        !tracks[k - 1]
    });
    let mut k = 0;
    board.vias.retain(|_| {
        k += 1;
        !vias[k - 1]
    });
    n
}
