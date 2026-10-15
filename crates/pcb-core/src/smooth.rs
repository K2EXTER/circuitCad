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
    Some(Edit { trims: vec![], pieces: vec![], patch: Some(Patch { side: ti.side, outline: pts, track: i }), kind: Kind::Fork, touches: vec![i, j] })
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
    Some(Edit { trims: vec![(i, ia, p1), (j, ja, p2)], pieces, patch: None, kind: Kind::Corner, touches: vec![i, j] })
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
    Some(Edit { trims: vec![(wide.0, wide.1, at(l))], pieces, patch: None, kind: Kind::Taper, touches: vec![wide.0] })
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
    // another track leaves the same place: two teardrops would pile up into a lump
    let crowded = board.tracks.iter().enumerate().any(|(k, u)| k != i && u.side == t.side && (u.a.dist(v) < top || u.b.dist(v) < top));
    if crowded {
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
    Some(Edit { trims: vec![(i, is_a, at(exit + ramp))], pieces, patch: None, kind: Kind::Taper, touches: vec![i] })
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
fn plan(board: &Board, opts: &SmoothOptions, only: Option<&[usize]>) -> Vec<Edit> {
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
    // the nets that are left as they are
    if !board.finish_exclude.is_empty() {
        let conn = crate::drc::connectivity(board);
        edits.retain(|e| e.touches.iter().all(|&i| conn.track_net(i).is_none_or(|n| !board.finish_exclude.iter().any(|x| x == n))));
    }
    edits
}

/// Apply the track edits to a copy of `board`: all at once if that breaks nothing, else one by one, keeping the
/// harmless ones. Returns the new board and the edits that were kept.
fn apply_checked(board: &Board, edits: &[Edit]) -> (Board, Vec<usize>) {
    let (base_drc, base_open) = (check(board).len(), ratsnest(board).len());
    let ok = |b: &Board| check(b).len() <= base_drc && ratsnest(b).len() <= base_open;
    let mut all = board.clone();
    for e in edits {
        apply(&mut all, e);
    }
    if ok(&all) {
        return (all, (0..edits.len()).collect());
    }
    // the batch broke something: keep only the edits that are harmless on their own
    let mut cur = board.clone();
    let mut kept = vec![];
    for (k, e) in edits.iter().enumerate() {
        let mut trial = cur.clone();
        apply(&mut trial, e);
        if ok(&trial) {
            cur = trial;
            kept.push(k);
        }
    }
    (cur, kept)
}

fn smooth(board: &mut Board, opts: &SmoothOptions, only: Option<&[usize]>) -> SmoothReport {
    let opts = SmoothOptions { forks: false, ..*opts };
    let edits = plan(board, &opts, only);
    if edits.is_empty() {
        return SmoothReport::default();
    }
    let (new, kept) = apply_checked(board, &edits);
    *board = new;
    report(&kept.iter().map(|&k| &edits[k]).collect::<Vec<_>>())
}

/// The copper of `board` as it is made when its finish settings are on: the tracks with corners rounded and widths
/// stepped, and the polygons for the notches between tracks. `None` when the finish is off (use the board as it is).
/// The board is not changed, so this is worked out again whenever it is needed.
pub fn finished(board: &Board) -> Option<Finished> {
    if !board.finish.on {
        return None;
    }
    let edits = plan(board, &board.finish, None);
    let track_edits: Vec<Edit> = edits.iter().filter(|e| e.patch.is_none()).cloned().collect();
    let (new, _) = apply_checked(board, &track_edits);
    // a patch is added where the other copper keeps its distance from it
    let conn = crate::drc::connectivity(&new);
    let patches = edits
        .iter()
        .filter_map(|e| e.patch.as_ref().map(|p| (p, e.touches[0])))
        .filter(|(p, t)| crate::drc::patch_clear(&new, &conn, p.side, &p.outline, *t))
        .map(|(p, _)| p.clone())
        .collect();
    Some(Finished { board: new, patches })
}
