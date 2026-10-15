//! Finishing touches for routed tracks, as real boards have them: corners rounded off, and a width change
//! stepped smoothly instead of in one jump.
//!
//! Tracks stay straight segments (a rounded corner is a few short ones, a taper is a few of changing width), so
//! DRC, Gerber and the 3D view need nothing new. The result is used only if it breaks no more rules than before
//! and never cuts a connection; when the whole batch does, the edits are tried one by one and the harmless ones kept.

use std::collections::HashMap;

use crate::drc::check;
use crate::model::*;
use crate::ratsnest::ratsnest;
use crate::shove::pinned;

/// What to smooth and how much.
#[derive(Clone, Copy, Debug)]
pub struct SmoothOptions {
    /// Round the corners where two tracks of the same width meet.
    pub corners: bool,
    /// Corner radius (mm); smaller where the tracks are too short for it.
    pub radius: f64,
    /// Step a width change through a few in-between widths.
    pub tapers: bool,
    /// Fill and round the sharp notch between two tracks that leave one point at a narrow angle.
    pub forks: bool,
}

impl Default for SmoothOptions {
    fn default() -> Self {
        SmoothOptions { corners: true, radius: 0.5, tapers: true, forks: true }
    }
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
struct Edit {
    trims: Vec<(usize, bool, Point)>,
    pieces: Vec<Track>,
    kind: Kind,
    /// The tracks this edit is about (for "only the selected ones").
    touches: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Corner,
    Taper,
    Fork,
}

/// Width differences up to this are left as they are, and no step of a taper is larger (so a taper is not tapered again).
const MIN_STEP: f64 = 0.1;
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
/// round, concave end: the gap between the two ends in a circle of free space (radius `w`) that touches both edges,
/// instead of in a point. The routes stay as they are.
///
/// The fill is a row of thin tracks laid along the middle line between the two tracks, side by side across the
/// notch (it is |y| < x tan(a) - w / (2 cos(a)) for the half angle a); each runs from where the notch is that wide to the circle.
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
    let theta = phi / 2.0;
    let (bx, by) = (u1x + u2x, u1y + u2y);
    let bl = bx.hypot(by);
    let (bx, by) = (bx / bl, by / bl);
    // x runs along the middle line from `v`, y across it. The edges of the notch are y = +-e(x)
    let (tan, cos, sin) = (theta.tan(), theta.cos(), theta.sin());
    let edge_x = |y: f64| (y.abs() + w / (2.0 * cos)) / tan; // where the notch is `2 y` wide
    // the free circle: radius `rho`, touching both edges, centre at `cx` on the middle line
    let rho = w;
    let cx = (rho + w / 2.0) / sin;
    let tip = w / (2.0 * sin);
    if cx - rho - tip < 0.1 || cx + rho > l1.min(l2) / 2.0 {
        return None;
    }
    let at = |x: f64, y: f64| Point::new(v.x + bx * x - by * y, v.y + by * x + bx * y);
    // the thin tracks: an odd number, the middle one on the middle line
    let reach = rho * cos; // the circle touches the edges at y = +-reach
    let min_w = board.rules.min_track_width.max(0.05);
    // each track is as thin as the rules allow and they overlap, so the round ends trace the circle closely
    let thick = min_w;
    let m = ((2.0 * reach / (thick / 3.0)).ceil() as usize).max(3) | 1;
    let step = 2.0 * reach / m as f64;
    let end_of = |y: f64| cx - (rho * rho - y * y).max(0.0).sqrt() - thick / 2.0; // short of the circle by the round end
    // done before: a track already ends where the middle one ends
    let mark = at(end_of(0.0), 0.0);
    if board.tracks.iter().any(|t| t.side == ti.side && (t.a.dist(mark) < 1e-6 || t.b.dist(mark) < 1e-6)) {
        return None;
    }
    // the notch lies entirely under a pad: nothing to fill
    if pinned(board, at(cx - rho, 0.0), ti.side) && pinned(board, at(tip, 0.0), ti.side) {
        return None;
    }
    let mut pieces = vec![];
    for k in 0..m {
        let y = -reach + (k as f64 + 0.5) * step;
        let (x0, x1) = (edge_x(y) - 0.05, end_of(y));
        if x1 < x0 {
            continue;
        }
        pieces.push(Track { side: ti.side, a: at(x0, y), b: at(x1, y), width: thick });
    }
    if pieces.is_empty() {
        return None;
    }
    Some(Edit { trims: vec![], pieces, kind: Kind::Fork, touches: vec![i, j] })
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
    Some(Edit { trims: vec![(i, ia, p1), (j, ja, p2)], pieces, kind: Kind::Corner, touches: vec![i, j] })
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
    let n = ((dw / (MIN_STEP * 0.8)).ceil() as usize).max(2);
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
    Some(Edit { trims: vec![(wide.0, wide.1, at(l))], pieces, kind: Kind::Taper, touches: vec![wide.0] })
}

/// The copper a track end sits on: how wide a track may be where the teardrop starts (the length of the flat edge it
/// leaves a rectangular pad through; the diameter at the centre of a round end), and how far from the track end that is.
fn holder(board: &Board, p: Point, side: Side, (ux, uy): (f64, f64)) -> Option<(f64, f64)> {
    for q in &board.pads {
        let (w, h) = q.shape.size();
        if (q.drill.is_some() || q.side == side) && (p.x - q.pos.x).abs() <= w / 2.0 + 1e-6 && (p.y - q.pos.y).abs() <= h / 2.0 + 1e-6 {
            if !matches!(q.shape, PadShape::Rect { .. }) {
                // a round end has no flat edge to match: the teardrop starts at the centre, as wide as the pad is
                return (p.dist(q.pos) < 0.05).then_some((w.min(h), 0.0));
            }
            let exit = |half: f64, off: f64, u: f64| if u.abs() > 1e-9 { (half * u.signum() - off) / u } else { f64::INFINITY };
            let (tx, ty) = (exit(w / 2.0, p.x - q.pos.x, ux), exit(h / 2.0, p.y - q.pos.y, uy));
            // leaving through a side face of the pad: the edge is as long as the pad is tall, and the round end of the
            // track must still fit into the pad's depth
            let (edge, depth) = if tx <= ty { (h, w) } else { (w, h) };
            return Some((edge.min(2.0 * depth), tx.min(ty).max(0.0)));
        }
    }
    board.vias.iter().find(|v| v.pos.dist(p) <= v.diameter / 2.0 + 1e-6).and_then(|v| (p.dist(v.pos) < 0.05).then_some((v.diameter, 0.0)))
}

/// Where a track ends on a pad or via that is wider than the track: from the edge of the pad, a short stretch that
/// narrows from the width of that edge to the track's (a teardrop), so the track does not meet the pad in one jump.
fn pad_taper(board: &Board, i: usize, is_a: bool) -> Option<Edit> {
    let t = &board.tracks[i];
    let (v, far) = if is_a { (t.a, t.b) } else { (t.b, t.a) };
    let (ux, uy, len) = unit(v, far);
    let (top, exit) = holder(board, v, t.side, (ux, uy))?;
    let dw = top - t.width;
    if dw <= MIN_STEP {
        return None;
    }
    let ramp = dw * 1.5;
    // also what keeps a second run from tapering the same end again: the piece left on the pad is shorter than that
    if exit + ramp > len / 2.0 {
        return None;
    }
    let n = ((dw / (MIN_STEP * 0.8)).ceil() as usize).max(2);
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
        width: top - dw * k as f64 / n as f64,
    }));
    Some(Edit { trims: vec![(i, is_a, at(exit + ramp))], pieces, kind: Kind::Taper, touches: vec![i] })
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

/// Smooth the tracks of `board`. Returns what was done; the board is untouched when nothing could be done safely.
pub fn smooth_tracks(board: &mut Board, opts: &SmoothOptions) -> SmoothReport {
    smooth(board, opts, None)
}

/// Like [`smooth_tracks`], but only where a track in `tracks` (indices) is involved: its corners (the track next to
/// it gives way too), and where it meets a pad or via.
pub fn smooth_selected(board: &mut Board, opts: &SmoothOptions, tracks: &[usize]) -> SmoothReport {
    smooth(board, opts, Some(tracks))
}

fn smooth(board: &mut Board, opts: &SmoothOptions, only: Option<&[usize]>) -> SmoothReport {
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
                    edits.extend(pad_taper(board, i, is_a));
                }
            }
        }
    }
    if let Some(only) = only {
        edits.retain(|e| e.touches.iter().any(|i| only.contains(i)));
    }
    if edits.is_empty() {
        return SmoothReport::default();
    }
    let (base_drc, base_open) = (check(board).len(), ratsnest(board).len());
    let ok = |b: &Board| check(b).len() <= base_drc && ratsnest(b).len() <= base_open;
    let mut all = board.clone();
    for e in &edits {
        apply(&mut all, e);
    }
    if ok(&all) {
        *board = all;
        return report(&edits.iter().collect::<Vec<_>>());
    }
    // the batch broke something: keep only the edits that are harmless on their own
    let mut cur = board.clone();
    let mut kept = vec![];
    for e in &edits {
        let mut trial = cur.clone();
        apply(&mut trial, e);
        if ok(&trial) {
            cur = trial;
            kept.push(e);
        }
    }
    *board = cur;
    report(&kept)
}
