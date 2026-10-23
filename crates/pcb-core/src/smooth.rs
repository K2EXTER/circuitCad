//! Finishing touches for routed tracks, as real boards have them: corners rounded off, and a width change
//! stepped smoothly instead of in one jump.
//!
//! Two ways to use it. [`smooth_tracks`] / [`smooth_selected`] bake corners and tapers into the tracks of the board
//! (a rounded corner is a few short tracks, a taper a few of changing width). [`finished`] leaves the board alone and
//! works out, every time, how the copper looks finished: the derived tracks, and for the fine shapes that tracks
//! cannot make (the concave end of the notch between two tracks) polygons ([`Patch`]). The settings are `Board.finish`.
//! In both, the result is used only if it breaks no more rules than before and never cuts a connection; when the whole
//! batch does, the edits are tried one by one and the harmless ones kept.

use std::collections::HashMap;

use crate::drc::check;
use crate::model::*;
use crate::ratsnest::ratsnest;
use crate::shove::pinned;

/// What to smooth and how much; stored in the board as its "finish" settings.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Finish {
    /// Show and make the board with the finishing applied (the tracks in the file stay as drawn).
    pub on: bool,
    /// Round the corners where two tracks of the same width meet.
    pub corners: bool,
    /// Corner radius (mm); smaller where the tracks are too short for it.
    pub radius: f64,
    /// Step a width change through a few in-between widths.
    pub tapers: bool,
    /// Fill and round the sharp notch between two tracks that leave one point at a narrow angle (only as a patch,
    /// so only when `on`; baking into the tracks leaves it out).
    pub forks: bool,
}

pub type SmoothOptions = Finish;

impl Default for Finish {
    fn default() -> Self {
        Finish { on: false, corners: true, radius: 0.5, tapers: true, forks: true }
    }
}

/// A piece of copper that is a polygon, not a track: the fine shapes of the finishing.
#[derive(Clone, Debug)]
pub struct Patch {
    pub side: Side,
    pub outline: Vec<Point>,
    /// A track of the copper this patch belongs to (its net, for instance, is that track's).
    pub track: usize,
}

/// The copper of a board with its finish applied.
#[derive(Clone, Debug)]
pub struct Finished {
    /// The board with the derived tracks (everything else as it was).
    pub board: Board,
    pub patches: Vec<Patch>,
}

/// How many places were smoothed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SmoothReport {
    pub corners: usize,
    pub tapers: usize,
    /// Sharp notches between two tracks leaving one point that were filled in and rounded.
    pub forks: usize,
}

/// A track end moved to a new point, and the tracks that fill the gap.
#[derive(Clone)]
struct Edit {
    trims: Vec<(usize, bool, Point)>,
    pieces: Vec<Track>,
    /// A polygon that is added next to the tracks (does not change the board).
    patch: Option<Patch>,
    /// What the live finish does instead of `trims` and `pieces`: a polygon with a smooth curved edge (not steps),
    /// and thin tracks that keep the copper connected.
    live: Option<Live>,
    kind: Kind,
    /// The tracks this edit is about (for "only the selected ones").
    touches: Vec<usize>,
}

#[derive(Clone)]
struct Live {
    trims: Vec<(usize, bool, Point)>,
    pieces: Vec<Track>,
    patch: Patch,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Corner,
    Taper,
    Fork,
    /// Fillets where a track leaves a pad's face (only in the live finish).
    Fillet,
}

/// Width differences up to this are left as they are, and no step of a taper is larger (so a taper is not tapered again).
const MIN_STEP: f64 = 0.1;
/// The width steps of a taper that is baked into the tracks (mm): fine enough that the round ends of the pieces do not show.
const BAKE_STEP: f64 = 0.02;
/// Corners that turn less than this are left as they are (the pieces of an arc turn 22.5° each, so an arc is not rounded again).
const MIN_TURN: f64 = 30.0;
const MIN_LEN: f64 = 0.05;
/// Two tracks leaving a point at an angle between these (degrees) get their notch filled; closer than the lower one they run on top of each other.
const MIN_FORK: f64 = 8.0;
const MAX_FORK: f64 = 60.0;

type End = (usize, bool);

fn key(p: Point, side: Side) -> (i64, i64, Side) {
    ((p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64, side)
}

fn unit(from: Point, to: Point) -> (f64, f64, f64) {
    let l = from.dist(to);
    if l < 1e-9 { (0.0, 0.0, 0.0) } else { ((to.x - from.x) / l, (to.y - from.y) / l, l) }
}

/// Every place where exactly two tracks of one layer end at the same point.
fn pairs(board: &Board) -> Vec<(Point, End, End)> {
    let mut at: HashMap<(i64, i64, Side), Vec<End>> = HashMap::new();
    for (i, t) in board.tracks.iter().enumerate() {
        at.entry(key(t.a, t.side)).or_default().push((i, true));
        at.entry(key(t.b, t.side)).or_default().push((i, false));
    }
    let mut out: Vec<_> = at
        .into_values()
        .filter(|v| v.len() == 2 && v[0].0 != v[1].0)
        .map(|v| {
            let t = &board.tracks[v[0].0];
            (if v[0].1 { t.a } else { t.b }, v[0], v[1])
        })
        .collect();
    out.sort_by_key(|x| x.1);
    out
}

/// The same, off any pad or via.
fn joints(board: &Board) -> Vec<(Point, End, End)> {
    let mut out = pairs(board);
    out.retain(|(p, a, _)| !pinned(board, *p, board.tracks[a.0].side));
    out
}

/// Two tracks of one width leave `v` at a sharp angle: between them lies a narrow notch whose tip is a sharp
/// copper corner (an acid trap, and what the acute-angle DRC check looks for). Fill the notch from its tip up to a
/// round, concave end: the gap between the two ends in a circle of free space (radius = the track width) that
/// touches both edges, instead of in a point. The routes stay as they are; the fill is a polygon.
fn fork(board: &Board, v: Point, (i, ia): End, (j, ja): End) -> Option<Edit> {
    let (ti, tj) = (&board.tracks[i], &board.tracks[j]);
    let w = ti.width;
    if (w - tj.width).abs() > 0.02 {
        return None;
    }
    let (u1x, u1y, l1) = unit(v, far_end(ti, ia));
    let (u2x, u2y, l2) = unit(v, far_end(tj, ja));
    let phi = (u1x * u2x + u1y * u2y).clamp(-1.0, 1.0).acos();
    if phi < MIN_FORK.to_radians() || phi > MAX_FORK.to_radians() {
        return None;
    }
    // a third track runs through this point: it is a T, and `junctions` takes it
    if board.tracks.iter().enumerate().any(|(k, t)| k != i && k != j && t.side == ti.side && t.a.dist(v) > 1e-6 && t.b.dist(v) > 1e-6 && crate::drc::point_seg(v, t.a, t.b) < 1e-6) {
        return None;
    }
    let theta = phi / 2.0;
    let (bx, by) = (u1x + u2x, u1y + u2y);
    let bl = bx.hypot(by);
    let (bx, by) = (bx / bl, by / bl);
    // x runs along the middle line from `v`, y across it. The edges of the notch are y = +-(x tan(a) - w / (2 cos(a)))
    let (cos, sin) = (theta.cos(), theta.sin());
    let rho = w;
    let cx = (rho + w / 2.0) / sin; // the free circle's centre: it touches both edges
    let tip = w / (2.0 * sin); // where the two edges cross
    if cx - rho - tip < 0.1 || cx + rho > l1.min(l2) / 2.0 {
        return None;
    }
    let at = |x: f64, y: f64| Point::new(v.x + bx * x - by * y, v.y + by * x + bx * y);
    // the notch lies entirely under a pad: nothing to fill
    if pinned(board, at(cx - rho, 0.0), ti.side) && pinned(board, at(tip, 0.0), ti.side) {
        return None;
    }
    // the polygon: from the tip along one edge to where the circle touches it, round the near side of the circle
    // (a concave arc, seen from the copper), and back along the other edge. The ends are pushed a little into
    // the two tracks so that the copper overlaps and no hairline is left.
    const SINK: f64 = 0.03;
    let t_up = (cx - rho * sin, rho * cos);
    let t_low = (cx - rho * sin, -rho * cos);
    let mut pts = vec![at(tip - 0.05, 0.0), at(t_low.0, t_low.1 - SINK), at(t_low.0, t_low.1)];
    let (a_low, a_up) = (-(std::f64::consts::FRAC_PI_2 + theta), std::f64::consts::FRAC_PI_2 + theta);
    // from the lower touching point to the upper one through the side that faces the tip (angle pi)
    let from = std::f64::consts::TAU + a_low; // 270 deg - theta
    let steps = ((from - a_up) / 10f64.to_radians()).ceil().max(4.0) as usize;
    for k in 1..steps {
        let a = from - (from - a_up) * k as f64 / steps as f64;
        pts.push(at(cx + rho * a.cos(), rho * a.sin()));
    }
    pts.extend([at(t_up.0, t_up.1), at(t_up.0, t_up.1 + SINK)]);
    Some(Edit { trims: vec![], pieces: vec![], patch: Some(Patch { side: ti.side, outline: pts, track: i }), live: None, kind: Kind::Fork, touches: vec![i, j] })
}

/// One track leaving a junction: its direction, width, how far it runs and which track it is.
#[derive(Clone, Copy)]
struct Ray {
    u: (f64, f64),
    w: f64,
    len: f64,
    track: usize,
}

/// The tracks that meet at `v` (on one layer, not on a pad or via) when there are three or more ways out: tracks that
/// end there, and tracks that run through it (each gives two ways out).
fn junction_rays(board: &Board, v: Point, side: Side) -> Vec<Ray> {
    let mut rays = vec![];
    for (k, t) in board.tracks.iter().enumerate().filter(|(_, t)| t.side == side) {
        let (da, db) = (t.a.dist(v), t.b.dist(v));
        if da < 1e-6 {
            let (ux, uy, len) = unit(v, t.b);
            rays.push(Ray { u: (ux, uy), w: t.width, len, track: k });
        } else if db < 1e-6 {
            let (ux, uy, len) = unit(v, t.a);
            rays.push(Ray { u: (ux, uy), w: t.width, len, track: k });
        } else if crate::drc::point_seg(v, t.a, t.b) < 1e-6 {
            for (end, d) in [(t.a, da), (t.b, db)] {
                let (ux, uy, _) = unit(v, end);
                rays.push(Ray { u: (ux, uy), w: t.width, len: d, track: k });
            }
        }
    }
    rays
}

/// The copper to add in the corner between two tracks `a` and `b` that leave `v`, so that the corner is not sharp: a
/// concave arc of radius `rho` that touches the edges of both, the corner filled up to it. `None` when the tracks are
/// too short for it, lie too close to a straight line or too close together.
fn notch_fill(v: Point, a: &Ray, b: &Ray, rho: f64) -> Option<Vec<Point>> {
    let phi = (a.u.0 * b.u.0 + a.u.1 * b.u.1).clamp(-1.0, 1.0).acos();
    if phi < MIN_FORK.to_radians() || phi > 172f64.to_radians() {
        return None;
    }
    // the normal of each ray, pointing into the notch
    let normal = |r: &Ray, towards: &Ray| {
        let n = (-r.u.1, r.u.0);
        if n.0 * towards.u.0 + n.1 * towards.u.1 >= 0.0 { n } else { (-n.0, -n.1) }
    };
    let (na, nb) = (normal(a, b), normal(b, a));
    let det = na.0 * nb.1 - na.1 * nb.0;
    if det.abs() < 1e-6 {
        return None;
    }
    // where the edges of the two tracks cross: n_a . p = w_a / 2 and n_b . p = w_b / 2
    let (ra, rb) = (a.w / 2.0, b.w / 2.0);
    let p = ((ra * nb.1 - na.1 * rb) / det, (na.0 * rb - ra * nb.0) / det);
    let apex = Point::new(v.x + p.0, v.y + p.1);
    let reach = p.0.hypot(p.1);
    let tan_half = (phi / 2.0).tan();
    // the arc may not need more of the tracks than they have, and stays short
    let t_max = (a.len.min(b.len) / 2.0 - reach).min(1.5);
    let rho = rho.min(t_max * tan_half);
    let t = rho / tan_half;
    if t < 0.1 || rho < 0.05 {
        return None;
    }
    let (bx, by) = (a.u.0 + b.u.0, a.u.1 + b.u.1);
    let bl = bx.hypot(by);
    let (bx, by) = (bx / bl, by / bl);
    let dist_c = rho / (phi / 2.0).sin();
    let c = Point::new(apex.x + bx * dist_c, apex.y + by * dist_c);
    let (ta, tb) = (Point::new(apex.x + a.u.0 * t, apex.y + a.u.1 * t), Point::new(apex.x + b.u.0 * t, apex.y + b.u.1 * t));
    let (a1, a2) = ((ta.y - c.y).atan2(ta.x - c.x), (tb.y - c.y).atan2(tb.x - c.x));
    let mut sweep = a2 - a1;
    while sweep > std::f64::consts::PI {
        sweep -= std::f64::consts::TAU;
    }
    while sweep < -std::f64::consts::PI {
        sweep += std::f64::consts::TAU;
    }
    const SINK: f64 = 0.03;
    let mut pts = vec![
        Point::new(apex.x - bx * 0.05, apex.y - by * 0.05),
        Point::new(ta.x - na.0 * SINK, ta.y - na.1 * SINK),
        ta,
    ];
    let steps = (sweep.abs() / 10f64.to_radians()).ceil().max(3.0) as usize;
    for k in 1..steps {
        let ang = a1 + sweep * k as f64 / steps as f64;
        pts.push(Point::new(c.x + rho * ang.cos(), c.y + rho * ang.sin()));
    }
    pts.extend([tb, Point::new(tb.x - nb.0 * SINK, tb.y - nb.1 * SINK)]);
    Some(pts)
}

/// Where three or more tracks meet (a T, or a track that ends on the side of another): each corner between two
/// neighbouring tracks gets a concave round fillet instead of a sharp inside corner.
fn junctions(board: &Board, radius: f64) -> Vec<Edit> {
    let mut seen: std::collections::HashSet<(i64, i64, Side)> = Default::default();
    let mut out = vec![];
    for t in &board.tracks {
        for v in [t.a, t.b] {
            if !seen.insert(key(v, t.side)) || pinned(board, v, t.side) {
                continue;
            }
            let mut rays = junction_rays(board, v, t.side);
            if rays.len() < 3 {
                continue;
            }
            rays.sort_by(|x, y| x.u.1.atan2(x.u.0).total_cmp(&y.u.1.atan2(y.u.0)));
            let n = rays.len();
            for k in 0..n {
                let (a, b) = (&rays[k], &rays[(k + 1) % n]);
                if a.track == b.track {
                    continue; // the two halves of one track that runs through
                }
                let Some(outline) = notch_fill(v, a, b, radius) else { continue };
                out.push(Edit {
                    trims: vec![],
                    pieces: vec![],
                    patch: Some(Patch { side: t.side, outline, track: a.track }),
                    live: None,
                    kind: Kind::Fork,
                    touches: vec![a.track, b.track],
                });
            }
        }
    }
    out.extend(edge_junctions(board, radius));
    out
}

/// Branches that end inside another track's copper but not on its centre line (a thin track that ends at the edge of a
/// wide one): the branch is carried on to the centre line, and the notches on both sides of it are filled and rounded
/// as at any other T. The branch is not changed: the piece that is carried on lies inside the other track's copper.
fn edge_junctions(board: &Board, radius: f64) -> Vec<Edit> {
    let mut out = vec![];
    for (k, t) in board.tracks.iter().enumerate() {
        for (v, far) in [(t.a, t.b), (t.b, t.a)] {
            if pinned(board, v, t.side) || junction_rays(board, v, t.side).len() >= 3 {
                continue; // on a pad or via, or a T that is already found by its point
            }
            let (dx, dy, len) = unit(far, v);
            if len < 1e-9 {
                continue;
            }
            for (m, u) in board.tracks.iter().enumerate().filter(|(m, u)| *m != k && u.side == t.side) {
                let d = crate::drc::point_seg(v, u.a, u.b);
                // inside the other track's copper, off its centre line, and not at one of its ends
                if d < 1e-6 || d > u.width / 2.0 + 1e-6 || u.a.dist(v) < 1e-6 || u.b.dist(v) < 1e-6 {
                    continue;
                }
                let (mx, my, mlen) = unit(u.a, u.b);
                // where the branch's line crosses the other track's centre line
                let det = dx * my - dy * mx;
                if det.abs() < 1e-6 {
                    continue;
                }
                let s_cross = ((u.a.x - v.x) * my - (u.a.y - v.y) * mx) / det;
                let at = Point::new(v.x + dx * s_cross, v.y + dy * s_cross);
                if crate::drc::point_seg(at, u.a, u.b) > 1e-6 || mlen < 1e-9 {
                    continue;
                }
                let ray = |to: Point, w: f64, track: usize| {
                    let (ux, uy, l) = unit(at, to);
                    Ray { u: (ux, uy), w, len: l, track }
                };
                let mut rays = [ray(far, t.width, k), ray(u.a, u.width, m), ray(u.b, u.width, m)];
                if rays.iter().any(|r| r.len < 1e-6) {
                    continue;
                }
                rays.sort_by(|x, y| x.u.1.atan2(x.u.0).total_cmp(&y.u.1.atan2(y.u.0)));
                let n = rays.len();
                for a in 0..n {
                    let (ra, rb) = (&rays[a], &rays[(a + 1) % n]);
                    if ra.track == rb.track {
                        continue; // the two halves of the track that runs through
                    }
                    let Some(outline) = notch_fill(at, ra, rb, radius) else { continue };
                    out.push(Edit { trims: vec![], pieces: vec![], patch: Some(Patch { side: t.side, outline, track: ra.track }), live: None, kind: Kind::Fork, touches: vec![k, m] });
                }
                break;
            }
        }
    }
    out
}

fn far_end(t: &Track, is_a: bool) -> Point {
    if is_a { t.b } else { t.a }
}

/// Round the corner at `v` between the tracks `(i, ia)` and `(j, ja)` (`ia`: the joint is the `a` end).
fn corner(board: &Board, v: Point, (i, ia): End, (j, ja): End, radius: f64) -> Option<Edit> {
    let (ti, tj) = (&board.tracks[i], &board.tracks[j]);
    if (ti.width - tj.width).abs() > 0.02 {
        return None;
    }
    let (u1x, u1y, l1) = unit(v, far_end(ti, ia));
    let (u2x, u2y, l2) = unit(v, far_end(tj, ja));
    let phi = (u1x * u2x + u1y * u2y).clamp(-1.0, 1.0).acos(); // the angle between the two tracks: pi when straight on
    let turn = std::f64::consts::PI - phi;
    if turn < MIN_TURN.to_radians() || phi < 30f64.to_radians() {
        return None;
    }
    // each track gives at most half of its length to this corner, the other half stays for its other end
    let t_max = l1.min(l2) / 2.0;
    let tan_half = (phi / 2.0).tan();
    let r = radius.min(t_max * tan_half);
    let t = r / tan_half;
    if t < MIN_LEN {
        return None;
    }
    let (bx, by) = (u1x + u2x, u1y + u2y);
    let bl = bx.hypot(by);
    let to_centre = r / (phi / 2.0).sin();
    let c = Point::new(v.x + bx / bl * to_centre, v.y + by / bl * to_centre);
    let (p1, p2) = (Point::new(v.x + u1x * t, v.y + u1y * t), Point::new(v.x + u2x * t, v.y + u2y * t));
    let (a1, a2) = ((p1.y - c.y).atan2(p1.x - c.x), (p2.y - c.y).atan2(p2.x - c.x));
    let mut sweep = a2 - a1;
    while sweep > std::f64::consts::PI {
        sweep -= std::f64::consts::TAU;
    }
    while sweep < -std::f64::consts::PI {
        sweep += std::f64::consts::TAU;
    }
    let n = ((turn / 22.5f64.to_radians()).ceil() as usize).max(2);
    let mut pts = vec![p1];
    for k in 1..n {
        let a = a1 + sweep * k as f64 / n as f64;
        pts.push(Point::new(c.x + r * a.cos(), c.y + r * a.sin()));
    }
    pts.push(p2);
    let pieces = pts.windows(2).map(|w| Track { side: ti.side, a: w[0], b: w[1], width: ti.width }).collect();
    // live: the corner is a band of the exact arc (its edges are circles, sampled every 4 degrees), with a few coarse
    // tracks inside it that only keep the copper connected
    let r_out = r + ti.width / 2.0;
    let r_in = (r - ti.width / 2.0).max(0.0);
    let m = ((sweep.abs() / 4f64.to_radians()).ceil() as usize).max(4);
    let ang = |k: usize| a1 + sweep * k as f64 / m as f64;
    let on = |rad: f64, k: usize| Point::new(c.x + rad * ang(k).cos(), c.y + rad * ang(k).sin());
    let mut outline: Vec<Point> = (0..=m).map(|k| on(r_out, k)).collect();
    outline.extend((0..=m).rev().map(|k| on(r_in, k)));
    let n_live = ((sweep.abs() / 15f64.to_radians()).ceil() as usize).max(2);
    let live_pts: Vec<Point> = (0..=n_live).map(|k| {
        let a = a1 + sweep * k as f64 / n_live as f64;
        Point::new(c.x + r * a.cos(), c.y + r * a.sin())
    }).collect();
    let live = Live {
        trims: vec![(i, ia, p1), (j, ja, p2)],
        pieces: live_pts.windows(2).map(|w| Track { side: ti.side, a: w[0], b: w[1], width: ti.width }).collect(),
        patch: Patch { side: ti.side, outline, track: i },
    };
    Some(Edit { trims: vec![(i, ia, p1), (j, ja, p2)], pieces, patch: None, live: Some(live), kind: Kind::Corner, touches: vec![i, j] })
}

/// A smooth step from 0 to 1 (flat at both ends).
fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    (1.0 - (std::f64::consts::PI * t).cos()) / 2.0
}

/// A band along the line from `v` in direction `(ux, uy)`, from distance `s0` to `s1`, whose width goes from `w0`
/// (up to `s_from`) to `w1` (from `s_to`) as a smooth curve.
fn band(v: Point, u: (f64, f64), s0: f64, s1: f64, along: (f64, f64), widths: (f64, f64)) -> Vec<Point> {
    band_off(v, u, s0, s1, along, widths, (0.0, 0.0))
}

/// [`band`] whose middle also moves sideways, from the offset `c0` (up to `s_from`) to `c1` (from `s_to`), measured to
/// the left of the line: a teardrop whose base is centred on the pad while its tip is on the track.
fn band_off(v: Point, (ux, uy): (f64, f64), s0: f64, s1: f64, (s_from, s_to): (f64, f64), (w0, w1): (f64, f64), (c0, c1): (f64, f64)) -> Vec<Point> {
    const STEPS: usize = 12;
    let (nx, ny) = (-uy, ux);
    let e = |s: f64| ease((s - s_from) / (s_to - s_from).max(1e-9));
    let half = |s: f64| (w0 + (w1 - w0) * e(s)) / 2.0;
    let mid = |s: f64| c0 + (c1 - c0) * e(s);
    let pt = |s: f64, y: f64| Point::new(v.x + ux * s + nx * y, v.y + uy * s + ny * y);
    let ss: Vec<f64> = (0..=STEPS).map(|k| s0 + (s1 - s0) * k as f64 / STEPS as f64).collect();
    let mut out: Vec<Point> = ss.iter().map(|&s| pt(s, mid(s) - half(s))).collect();
    out.extend(ss.iter().rev().map(|&s| pt(s, mid(s) + half(s))));
    out
}

/// Step from the wider track's width to the narrower one's along the end of the wider track at `v`.
fn taper(board: &Board, v: Point, a: End, b: End) -> Option<Edit> {
    let (wide, narrow) = if board.tracks[a.0].width >= board.tracks[b.0].width { (a, b) } else { (b, a) };
    let (tw, tn) = (&board.tracks[wide.0], &board.tracks[narrow.0]);
    let dw = tw.width - tn.width;
    if dw <= MIN_STEP {
        return None;
    }
    let (ux, uy, len) = unit(v, far_end(tw, wide.1));
    let l = (dw * 4.0).max(0.3).min(len / 2.0);
    if l < 0.1 {
        return None;
    }
    let n = ((dw / BAKE_STEP).ceil() as usize).max(2);
    let at = |s: f64| Point::new(v.x + ux * s, v.y + uy * s);
    // from the far side (wide) to the joint (narrow)
    let pieces = (0..n)
        .map(|k| Track {
            side: tw.side,
            a: at(l - l * k as f64 / n as f64),
            b: at(l - l * (k + 1) as f64 / n as f64),
            width: tw.width - dw * (k as f64 + 0.5) / n as f64,
        })
        .collect();
    // live: the wide track stops `l` short of the joint, and a smooth band takes it down to the narrow track's width
    let outline = band(v, (ux, uy), -0.02, l + 0.02, (0.0, l), (tn.width, tw.width));
    let live = Live {
        trims: vec![(wide.0, wide.1, at(l))],
        pieces: vec![Track { side: tw.side, a: at(l), b: v, width: tn.width }],
        patch: Patch { side: tw.side, outline, track: wide.0 },
    };
    Some(Edit { trims: vec![(wide.0, wide.1, at(l))], pieces, patch: None, live: Some(live), kind: Kind::Taper, touches: vec![wide.0] })
}

/// The copper a track end sits on: how wide a track may be where the teardrop starts (the length of the flat edge it
/// leaves a rectangular pad through; the diameter at the centre of a round end), and how far from the track end that is.
fn holder(board: &Board, p: Point, side: Side, (ux, uy): (f64, f64)) -> Option<(f64, f64, bool, f64)> {
    for q in &board.pads {
        let (w, h) = q.shape.size();
        if (q.drill.is_some() || q.side == side) && (p.x - q.pos.x).abs() <= w / 2.0 + 1e-6 && (p.y - q.pos.y).abs() <= h / 2.0 + 1e-6 {
            if !matches!(q.shape, PadShape::Rect { .. }) {
                // a round end has no flat edge to match: the teardrop starts at the centre, as wide as the pad is
                return (p.dist(q.pos) < 0.05).then_some((w.min(h), 0.0, true, 0.0));
            }
            let exit = |half: f64, off: f64, u: f64| if u.abs() > 1e-9 { (half * u.signum() - off) / u } else { f64::INFINITY };
            let (tx, ty) = (exit(w / 2.0, p.x - q.pos.x, ux), exit(h / 2.0, p.y - q.pos.y, uy));
            // leaving through a side face of the pad: the edge is as long as the pad is tall, and the round end of the
            // track must still fit into the pad's depth
            let (edge, depth) = if tx <= ty { (h, w) } else { (w, h) };
            // the edge is centred on the pad, the track may leave it a little to the side
            let off = (q.pos.x - p.x) * -uy + (q.pos.y - p.y) * ux;
            return Some((edge.min(2.0 * depth), tx.min(ty).max(0.0), false, off));
        }
    }
    board.vias.iter().find(|v| v.pos.dist(p) <= v.diameter / 2.0 + 1e-6).and_then(|v| (p.dist(v.pos) < 0.05).then_some((v.diameter, 0.0, true, 0.0)))
}

/// Where a track ends on a pad or via that is wider than the track: from the edge of the pad, a short stretch that
/// narrows from the width of that edge to the track's (a teardrop), so the track does not meet the pad in one jump.
fn pad_taper(board: &Board, i: usize, is_a: bool) -> Option<Edit> {
    let t = &board.tracks[i];
    let (v, far) = if is_a { (t.a, t.b) } else { (t.b, t.a) };
    let (ux, uy, len) = unit(v, far);
    let (top, exit, round, off) = holder(board, v, t.side, (ux, uy))?;
    let dw = top - t.width;
    if dw <= MIN_STEP {
        return None;
    }
    // another track leaves the same place: two teardrops would pile up into a lump
    let crowded = board.tracks.iter().enumerate().any(|(k, u)| k != i && u.side == t.side && (u.a.dist(v) < top || u.b.dist(v) < top));
    if crowded {
        return None;
    }
    // a track that runs into another pad at its far end shares its length with the teardrop there (half each: that also
    // keeps a second run from tapering the same end again). Otherwise the whole length but a little is free, and a
    // short track (such as the lead the router makes) gets a shorter, steeper teardrop rather than none.
    let far_on_pad = holder(board, far, t.side, (-ux, -uy)).is_some();
    let limit = if far_on_pad { len / 2.0 } else { len - 0.05 };
    let full = dw * 1.5;
    let ramp = if far_on_pad { full } else { full.min(limit - exit) };
    if exit + ramp > limit + 1e-9 || ramp < full.min((dw * 0.3).max(0.15)) {
        return None;
    }
    // the pieces that go into the tracks are centred on the track, so their base is only as wide as the pad is on both
    // sides of the track (the live polygon is centred on the pad)
    let base = if round { top } else { (top - 2.0 * off.abs()).max(t.width + MIN_STEP) };
    let dwp = base - t.width;
    let n = ((dwp / BAKE_STEP).ceil() as usize).max(2);
    let at = |s: f64| Point::new(v.x + ux * s, v.y + uy * s);
    // the track runs on to the pad's edge; there the first piece is exactly as wide as the edge, and each next is narrower
    let mut pieces = vec![];
    if exit > 1e-6 {
        pieces.push(Track { side: t.side, a: v, b: at(exit), width: t.width });
    }
    pieces.extend((0..n).map(|k| Track {
        side: t.side,
        a: at(exit + ramp * k as f64 / n as f64),
        b: at(exit + ramp * (k + 1) as f64 / n as f64),
        width: base - dwp * k as f64 / n as f64,
    }));
    // live: the polygon. On a round end the sides are the tangents from the edges of the track to the pad's circle;
    // at a flat edge it is a smooth band from the length of the edge to the track's width.
    let (outline, stop) = if round {
        let r = top / 2.0;
        let lp = r + dw;
        let (qx, qy) = (lp + 0.02, t.width / 2.0);
        let th = qy.atan2(qx) + (r / qx.hypot(qy)).acos();
        let (tx, ty) = (r * th.cos(), r * th.sin());
        let (nx, ny) = (-uy, ux);
        let pt = |x: f64, y: f64| Point::new(v.x + ux * x + nx * y, v.y + uy * x + ny * y);
        (vec![pt(0.0, 0.0), pt(tx, -ty), pt(qx, -qy), pt(qx, qy), pt(tx, ty)], lp)
    } else {
        (band_off(v, (ux, uy), exit - 0.05, exit + ramp + 0.02, (exit, exit + ramp), (top, t.width), (off, 0.0)), exit + ramp)
    };
    let live = (stop <= limit + 1e-9).then(|| Live {
        trims: vec![(i, is_a, at(stop))],
        pieces: vec![Track { side: t.side, a: v, b: at(stop), width: t.width }],
        patch: Patch { side: t.side, outline, track: i },
    });
    Some(Edit { trims: vec![(i, is_a, at(exit + ramp))], pieces, patch: None, live, kind: Kind::Taper, touches: vec![i] })
}

/// A track that comes straight into one face of a pad that is deeper than the track is wide (across the track) but shallower than half
/// the track's width (along it): the round end would reach past the far face of the pad. The track is shortened until the tip of its round
/// end just touches the far face: the part that stuck out is cut, the rest of the end lies on the pad. (Where the pad is narrower across the
/// track than the track is wide there is no cut that helps: `pad_neck` narrows the track instead.)
fn pad_clip(board: &Board, i: usize, is_a: bool) -> Option<Edit> {
    let t = &board.tracks[i];
    let (v, far) = if is_a { (t.a, t.b) } else { (t.b, t.a) };
    let (ux, uy, len) = unit(v, far);
    let axis_x = ux.abs() > 0.9999;
    if !axis_x && uy.abs() <= 0.9999 {
        return None;
    }
    let pad = board.pads.iter().find(|q| {
        let (w, h) = q.shape.size();
        matches!(q.shape, PadShape::Rect { .. }) && (q.drill.is_some() || q.side == t.side) && (v.x - q.pos.x).abs() <= w / 2.0 + 1e-6 && (v.y - q.pos.y).abs() <= h / 2.0 + 1e-6
    })?;
    let (w, h) = pad.shape.size();
    // along the track (depth) and across it
    let (depth, across, off_across) = if axis_x { (w, h, (v.y - pad.pos.y).abs()) } else { (h, w, (v.x - pad.pos.x).abs()) };
    let r = t.width / 2.0;
    if off_across + r > across / 2.0 + 1e-9 {
        return None;
    }
    // the end of the copper, not a joint with another piece of track (the base of a teardrop that was baked into the tracks): finishing twice
    // is the same as once
    if board.tracks.iter().enumerate().any(|(k, u)| k != i && u.side == t.side && (u.a.dist(v) < 0.02 || u.b.dist(v) < 0.02)) {
        return None;
    }
    // how far the pad's far face (the one the round end points at) is from the track's end
    let c = (v.x - pad.pos.x) * -ux + (v.y - pad.pos.y) * -uy;
    let far_face = depth / 2.0 - c;
    let shift = r - far_face;
    if shift <= 0.01 || shift > len - 0.1 {
        return None;
    }
    Some(Edit { trims: vec![(i, is_a, Point::new(v.x + ux * shift, v.y + uy * shift))], pieces: vec![], patch: None, live: None, kind: Kind::Taper, touches: vec![i] })
}

/// How long (mm) the neck of a track on a pad narrower than the track is, before the track widens.
const NECK_LEN: f64 = 0.35;

/// A track that ends on a pad narrower than the track: its round end would stick out of the pad on both sides. From the pad, a neck as
/// wide as the pad's short side (the same as the pad, so nothing sticks out), then a short stretch that widens to the track's width. The
/// neck also keeps the solder on the small pad (a wide trace straight on it can pull the solder off). Not when the neck would be thinner than
/// the board's minimum track width, when another track leaves the same place, or when the track is too short for it.
fn pad_neck(board: &Board, i: usize, is_a: bool) -> Option<Edit> {
    let t = &board.tracks[i];
    let (v, far) = if is_a { (t.a, t.b) } else { (t.b, t.a) };
    let (ux, uy, len) = unit(v, far);
    // the pad the track's end is on
    let pad = board.pads.iter().find(|q| {
        let (w, h) = q.shape.size();
        (q.drill.is_some() || q.side == t.side) && (v.x - q.pos.x).abs() <= w / 2.0 + 1e-6 && (v.y - q.pos.y).abs() <= h / 2.0 + 1e-6
    })?;
    // the widest round end that still lies inside the pad at the place where the track ends (the pad's short side when the track ends at
    // its centre, less when it ends off centre: a track that comes in from the side may end at the pad's edge)
    let (w, h) = pad.shape.size();
    let (dx, dy) = ((v.x - pad.pos.x).abs(), (v.y - pad.pos.y).abs());
    let short = match pad.shape {
        PadShape::Circle { d } => d - 2.0 * v.dist(pad.pos),
        _ => 2.0 * (w / 2.0 - dx).min(h / 2.0 - dy),
    };
    // an end that is at the edge of the pad (the base of a teardrop that was baked into the tracks) has no room for a neck: it is left, so
    // that finishing twice is the same as once
    if short < board.rules.min_track_width - 1e-9 || t.width - short <= MIN_STEP {
        return None;
    }
    let crowded = board.tracks.iter().enumerate().any(|(k, u)| k != i && u.side == t.side && (u.a.dist(v) < t.width || u.b.dist(v) < t.width));
    if crowded {
        return None;
    }
    let dw = t.width - short;
    // a short track (the lead of a route that enters a pad from the side) gets a shorter neck and a steeper widening rather than none
    let room = len - 0.05;
    let neck = NECK_LEN.min(room * 0.45);
    let ramp = (dw * 1.5).min(room - neck);
    if neck < 0.1 || ramp < 0.1 {
        return None;
    }
    let n = ((dw / BAKE_STEP).ceil() as usize).max(2);
    let at = |s: f64| Point::new(v.x + ux * s, v.y + uy * s);
    let mut pieces = vec![Track { side: t.side, a: v, b: at(neck), width: short }];
    pieces.extend((0..n).map(|k| Track {
        side: t.side,
        a: at(neck + ramp * k as f64 / n as f64),
        b: at(neck + ramp * (k + 1) as f64 / n as f64),
        width: short + dw * (k + 1) as f64 / n as f64,
    }));
    Some(Edit { trims: vec![(i, is_a, at(neck + ramp))], pieces, patch: None, live: None, kind: Kind::Taper, touches: vec![i] })
}

/// The face of a rectangular pad that a track leaves straight out of (at a right angle): which pad, how far (mm) along the
/// track from its end `p` the face is, and where the face's two ends lie to the sides of the track's line (`lo` < 0 <
/// `hi` when the track leaves between them; measured to the left of the way out, `(-uy, ux)`).
struct Face {
    pad: usize,
    exit: f64,
    lo: f64,
    hi: f64,
}

fn flat_face(board: &Board, p: Point, side: Side, (ux, uy): (f64, f64)) -> Option<Face> {
    for (n, q) in board.pads.iter().enumerate() {
        let (w, h) = q.shape.size();
        if (q.drill.is_some() || q.side == side) && (p.x - q.pos.x).abs() <= w / 2.0 + 1e-6 && (p.y - q.pos.y).abs() <= h / 2.0 + 1e-6 {
            if !matches!(q.shape, PadShape::Rect { .. }) {
                return None;
            }
            let exit = |half: f64, off: f64, u: f64| if u.abs() > 1e-9 { (half * u.signum() - off) / u } else { f64::INFINITY };
            let (tx, ty) = (exit(w / 2.0, p.x - q.pos.x, ux), exit(h / 2.0, p.y - q.pos.y, uy));
            let x_face = tx <= ty;
            // only straight out of a face: along x through an end face, along y through a long face
            if (x_face && uy.abs() > 1e-6) || (!x_face && ux.abs() > 1e-6) {
                return None;
            }
            let (a, b) = if x_face {
                let x = q.pos.x + ux.signum() * w / 2.0;
                (Point::new(x, q.pos.y - h / 2.0), Point::new(x, q.pos.y + h / 2.0))
            } else {
                let y = q.pos.y + uy.signum() * h / 2.0;
                (Point::new(q.pos.x - w / 2.0, y), Point::new(q.pos.x + w / 2.0, y))
            };
            let (nx, ny) = (-uy, ux);
            let (ta, tb) = ((a.x - p.x) * nx + (a.y - p.y) * ny, (b.x - p.x) * nx + (b.y - p.y) * ny);
            return Some(Face { pad: n, exit: tx.min(ty).max(0.0), lo: ta.min(tb), hi: ta.max(tb) });
        }
    }
    None
}

/// The biggest radius (mm) of the fillets where a track meets the edge of its pad.
pub(crate) const MAX_FILLET: f64 = 0.4;
/// Smaller than this a fillet is not made.
const MIN_FILLET: f64 = 0.1;

/// Where a track leaves a rectangular pad straight out of a face: a fillet on each side, filling the sharp corner
/// between the track's edge and the pad's edge with a concave arc (a teardrop with a flat base at the pad's edge and sides
/// that run smoothly into the track). The radius is as big as there is room for: up to the pad's corner, up to half the gap
/// to a track that leaves the same face beside it (the two fillets then make one round arch between the tracks), and up
/// to `MAX_FILLET`. Only for the live finish: the track itself is not changed. `None` where there is nothing to do.
fn pad_fillet(board: &Board, i: usize, is_a: bool) -> Option<Edit> {
    let t = &board.tracks[i];
    let (v, far) = if is_a { (t.a, t.b) } else { (t.b, t.a) };
    let (ux, uy, len) = unit(v, far);
    let face = flat_face(board, v, t.side, (ux, uy))?;
    let h = t.width / 2.0;
    let (nx, ny) = (-uy, ux);
    // room beside the track on each side, up to the corner of the pad
    let (mut left, mut right) = (face.hi - h, -face.lo - h);
    // a track that leaves the same face in the same direction shares the gap with this one
    for u in board.tracks.iter().enumerate().filter(|(k, u)| *k != i && u.side == t.side).map(|(_, u)| u) {
        for (w0, w1) in [(u.a, u.b), (u.b, u.a)] {
            let (vx, vy, _) = unit(w0, w1);
            if vx * ux + vy * uy < 0.9999 {
                continue;
            }
            let Some(f2) = flat_face(board, w0, u.side, (vx, vy)) else { continue };
            // the same face of the same pad: its plane is the same
            let plane = |p: Point, f: &Face| (p.x + vx * f.exit) * ux + (p.y + vy * f.exit) * uy;
            if f2.pad != face.pad || (plane(w0, &f2) - plane(v, &face)).abs() > 1e-6 {
                continue;
            }
            let lateral = (w0.x - v.x) * nx + (w0.y - v.y) * ny;
            let gap = lateral.abs() - h - u.width / 2.0;
            if lateral > 0.0 {
                left = left.min(gap / 2.0);
            } else {
                right = right.min(gap / 2.0);
            }
        }
    }
    // along the track: a track that runs into another pad shares its length with the fillets there
    let far_on_pad = holder(board, far, t.side, (-ux, -uy)).is_some();
    let room = if far_on_pad { (len - face.exit) / 2.0 } else { len - face.exit - 0.05 };
    let radius = |free: f64| {
        let r = free.min(MAX_FILLET).min(room);
        if r >= MIN_FILLET { r } else { 0.0 }
    };
    let (rl, rr) = (radius(left), radius(right));
    if rl == 0.0 && rr == 0.0 {
        return None;
    }
    // in the frame of the track: s along it from the end `v`, t to its left; the pad's edge is at s0
    let s0 = face.exit;
    let eps = 0.02;
    let mut pts: Vec<(f64, f64)> = vec![(s0 - eps, -h - rr)];
    const STEPS: usize = 10;
    // the right side (t < 0): from the pad's edge, a concave arc to the track's edge
    if rr > 0.0 {
        pts.push((s0, -h - rr));
        for k in 1..=STEPS {
            // the circle's centre is at (s0 + rr, -h - rr); the angle goes from 180° to 90°
            let a = std::f64::consts::PI * (1.0 - 0.5 * k as f64 / STEPS as f64);
            pts.push((s0 + rr + rr * a.cos(), -h - rr + rr * a.sin()));
        }
    } else {
        pts = vec![(s0 - eps, -h), (s0, -h)];
    }
    // the left side (t > 0), from the track's edge out to the pad's edge
    if rl > 0.0 {
        for k in 0..=STEPS {
            // the centre is at (s0 + rl, h + rl); the angle goes from 270° to 180°
            let a = std::f64::consts::PI * (1.5 - 0.5 * k as f64 / STEPS as f64);
            pts.push((s0 + rl + rl * a.cos(), h + rl + rl * a.sin()));
        }
        pts.push((s0 - eps, h + rl));
    } else {
        pts.push((s0, h));
        pts.push((s0 - eps, h));
    }
    let outline = pts.into_iter().map(|(s, l)| Point::new(v.x + ux * s + nx * l, v.y + uy * s + ny * l)).collect();
    let patch = Patch { side: t.side, outline, track: i };
    Some(Edit { trims: vec![], pieces: vec![], patch: None, live: Some(Live { trims: vec![], pieces: vec![], patch }), kind: Kind::Fillet, touches: vec![i] })
}

fn apply(board: &mut Board, e: &Edit) {
    for &(i, is_a, p) in &e.trims {
        if is_a {
            board.tracks[i].a = p;
        } else {
            board.tracks[i].b = p;
        }
    }
    board.tracks.extend(e.pieces.iter().cloned());
}

fn report(es: &[&Edit]) -> SmoothReport {
    let n = |k: Kind| es.iter().filter(|e| e.kind == k).count();
    SmoothReport { corners: n(Kind::Corner), tapers: n(Kind::Taper), forks: n(Kind::Fork) }
}

/// Bake the finishing into the tracks of `board`. Returns what was done; the board is untouched when nothing could be done
/// safely. (Forks are patches, which a board does not store: they are not part of this.)
pub fn smooth_tracks(board: &mut Board, opts: &SmoothOptions) -> SmoothReport {
    smooth(board, opts, None)
}

/// Like [`smooth_tracks`], but only where a track in `tracks` (indices) is involved: its corners (the track next to
/// it gives way too), and where it meets a pad or via.
pub fn smooth_selected(board: &mut Board, opts: &SmoothOptions, tracks: &[usize]) -> SmoothReport {
    smooth(board, opts, Some(tracks))
}

/// What the finishing would do on `board`: every edit, the ones for ends that a notch claims left out of the others.
/// The edits for the finish. `live`: for the finish that is only drawn and made into files (not baked into the tracks):
/// where a track leaves a rectangular pad's face the fillets are used instead of the stepped teardrop.
fn plan(board: &Board, opts: &SmoothOptions, only: Option<&[usize]>, live: bool) -> Vec<Edit> {
    let mut edits = vec![];
    // ends that belong to a notch are left to it: no corner or teardrop there
    let mut used: Vec<End> = vec![];
    if opts.forks {
        for (v, a, b) in pairs(board) {
            if let Some(e) = fork(board, v, a, b) {
                edits.push(e);
                used.extend([a, b]);
            }
        }
        edits.extend(junctions(board, opts.radius));
    }
    for (v, a, b) in joints(board) {
        if used.contains(&a) || used.contains(&b) {
            continue;
        }
        let e = if opts.corners { corner(board, v, a, b, opts.radius) } else { None };
        if let Some(e) = e.or_else(|| if opts.tapers { taper(board, v, a, b) } else { None }) {
            edits.push(e);
        }
    }
    if opts.tapers {
        for i in 0..board.tracks.len() {
            for is_a in [true, false] {
                if !used.contains(&(i, is_a)) {
                    // a track wider than its pad is narrowed first (the neck): a fillet at the pad's face would leave the round end sticking out
                    // on the far side
                    let fillet = || live.then(|| pad_fillet(board, i, is_a)).flatten();
                    if let Some(mut clip) = pad_clip(board, i, is_a) {
                        // the cut at the far face keeps the fillets (the teardrop curves) at the near face
                        if let Some(Live { patch, .. }) = fillet().and_then(|f| f.live) {
                            clip.live = Some(Live { trims: clip.trims.clone(), pieces: vec![], patch });
                        }
                        edits.push(clip);
                    } else {
                        match pad_neck(board, i, is_a).or_else(fillet) {
                            Some(e) => edits.push(e),
                            None => edits.extend(pad_taper(board, i, is_a)),
                        }
                    }
                }
            }
        }
    }
    if let Some(only) = only {
        edits.retain(|e| e.touches.iter().any(|i| only.contains(i)));
    }
    // the nets that are left as they are
    if !board.finish_exclude.is_empty() {
        let conn = crate::drc::connectivity(board);
        edits.retain(|e| e.touches.iter().all(|&i| conn.track_net(i).is_none_or(|n| !board.finish_exclude.iter().any(|x| x == n))));
    }
    edits
}

/// Apply the track edits to a copy of `board`: all at once if that breaks nothing, else one by one, keeping the
/// harmless ones. Returns the new board and the edits that were kept.
/// How many times the edits were tried one by one (the batch broke something), counted for the tests.
#[cfg(test)]
pub(crate) static ONE_BY_ONE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn apply_checked(board: &Board, edits: &[Edit], fast: bool) -> (Board, Vec<usize>) {
    let (base_drc, base_open) = (check(board).len(), ratsnest(board).len());
    let ok = |b: &Board| check(b).len() <= base_drc && ratsnest(b).len() <= base_open;
    let mut all = board.clone();
    for e in edits {
        apply(&mut all, e);
    }
    if ok(&all) {
        return (all, (0..edits.len()).collect());
    }
    // the batch broke something: keep only the edits that are harmless on their own. On a board without findings the
    // copper an edit changes is all that has to be looked at (and one pass for the groups of copper); otherwise the
    // whole board is checked (`fast` off is the way it was, for the tests).
    let quick = fast && base_drc == 0;
    #[cfg(test)]
    ONE_BY_ONE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut cur = board.clone();
    let mut kept = vec![];
    for (k, e) in edits.iter().enumerate() {
        let before = cur.tracks.len();
        let mut trial = cur.clone();
        apply(&mut trial, e);
        let good = if quick {
            let conn = crate::drc::connectivity(&trial);
            let mut changed: Vec<usize> = e.trims.iter().map(|t| t.0).collect();
            changed.extend(before..trial.tracks.len());
            let groups: Vec<usize> = (0..trial.pads.len()).map(|i| conn.pad_group(i)).collect();
            crate::drc::violations_of_items(&trial, &changed, &[], &conn) == 0 && crate::ratsnest::ratsnest_in(&trial, &groups).len() <= base_open
        } else {
            ok(&trial)
        };
        if good {
            cur = trial;
            kept.push(k);
        }
    }
    (cur, kept)
}

fn smooth(board: &mut Board, opts: &SmoothOptions, only: Option<&[usize]>) -> SmoothReport {
    smooth_with(board, opts, only, true)
}

/// [`smooth_tracks`] with the whole board checked for every edit that is tried on its own (what it was before).
#[cfg(test)]
pub(crate) fn smooth_tracks_slow(board: &mut Board, opts: &SmoothOptions) -> SmoothReport {
    smooth_with(board, opts, None, false)
}

fn smooth_with(board: &mut Board, opts: &SmoothOptions, only: Option<&[usize]>, fast: bool) -> SmoothReport {
    let opts = SmoothOptions { forks: false, ..*opts };
    let edits = plan(board, &opts, only, false);
    if edits.is_empty() {
        return SmoothReport::default();
    }
    let (new, kept) = apply_checked(board, &edits, fast);
    *board = new;
    report(&kept.iter().map(|&k| &edits[k]).collect::<Vec<_>>())
}

/// The copper of `board` as it is made when its finish settings are on: the tracks with corners rounded and widths
/// stepped, and the polygons for the notches between tracks. `None` when the finish is off (use the board as it is).
/// The board is not changed, so this is worked out again whenever it is needed.
pub fn finished(board: &Board) -> Option<Finished> {
    finished_with(board, true)
}

/// [`finished`] with the whole board checked for every edit that is tried on its own (what it was before).
#[cfg(test)]
pub(crate) fn finished_slow(board: &Board) -> Option<Finished> {
    finished_with(board, false)
}

fn finished_with(board: &Board, fast: bool) -> Option<Finished> {
    if !board.finish.on {
        return None;
    }
    let conn = crate::drc::connectivity(board);
    // the live version of every edit: the polygon, and the tracks that go with it. A polygon is used only where the
    // other copper keeps its distance from it.
    let mut kept_edits: Vec<Edit> = vec![];
    let mut patches_of: Vec<Option<Patch>> = vec![];
    let copper = crate::drc::PatchCheck::new(board);
    for e in plan(board, &board.finish, None, true) {
        let (edit, patch) = match (&e.live, &e.patch) {
            (Some(l), _) => (Edit { trims: l.trims.clone(), pieces: l.pieces.clone(), patch: None, live: None, ..e.clone() }, Some(l.patch.clone())),
            (None, Some(p)) => (Edit { patch: None, ..e.clone() }, Some(p.clone())),
            (None, None) => (e, None),
        };
        if let Some(p) = &patch
            && !copper.clear(board, &conn, p.side, &p.outline, p.track)
        {
            continue;
        }
        kept_edits.push(edit);
        patches_of.push(patch);
    }
    let (new, kept) = apply_checked(board, &kept_edits, fast);
    let mut patches: Vec<Patch> = kept.iter().filter_map(|&k| patches_of[k].clone()).collect();
    // the slots between tracks of one net that run side by side (the fork patch only fills the sharp tip)
    if board.finish.forks {
        // the pads that a trunk's necks leave: the necks are thin on purpose and the pads stay apart, so nothing is filled in between them
        let necked: Vec<&Pad> = (0..board.tracks.len())
            .filter(|&i| crate::current::is_neck(board, &conn, i))
            .filter_map(|i| {
                let t = &board.tracks[i];
                [t.a, t.b].into_iter().find_map(|e| board.pads.iter().find(|q| (q.drill.is_some() || q.side == t.side) && e.dist(q.pos) < 0.05))
            })
            .collect();
        let near_necked_pad = |outline: &[Point]| {
            let n = outline.len().max(1) as f64;
            let c = Point::new(outline.iter().map(|p| p.x).sum::<f64>() / n, outline.iter().map(|p| p.y).sum::<f64>() / n);
            necked.iter().any(|q| {
                let (w, h) = q.shape.size();
                let (dx, dy) = (((c.x - q.pos.x).abs() - w / 2.0).max(0.0), ((c.y - q.pos.y).abs() - h / 2.0).max(0.0));
                dx.hypot(dy) <= 0.4
            })
        };
        for s in crate::slot::slot_patches(board, &conn, &patches, board.finish.radius) {
            let excluded = conn.track_net(s.track).is_some_and(|n| board.finish_exclude.iter().any(|x| x == n)) || near_necked_pad(&s.outline);
            if !excluded && copper.clear(board, &conn, s.side, &s.outline, s.track) {
                patches.push(Patch { side: s.side, outline: s.outline, track: s.track });
            }
        }
    }
    Some(Finished { board: new, patches })
}
