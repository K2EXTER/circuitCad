//! Where tracks meet rectangular pads: make every track leave its pad at a right angle to the pad's edge for a
//! short stretch, and when two tracks leave one pad from the same point, move them to the two ends of the pad so that
//! they run side by side (no sharp notch between them).
//!
//! The track is changed only near the pad: the end on the pad gets a straight lead out of the pad's edge, then a
//! straight piece (along a horizontal, vertical or 45° line) that joins the old track. A pad is done only if the DRC
//! finds no more problems than before and nothing gets disconnected; otherwise it is left as it was.

use crate::drc::check;
use crate::model::*;
use crate::ratsnest::ratsnest;

#[derive(Clone, Copy, Debug)]
pub struct EntryOptions {
    /// Length (mm) of the straight lead outside the pad's edge.
    pub lead: f64,
    /// Move two tracks that leave one pad from the same point to the two ends of the pad.
    pub spread: bool,
}

impl Default for EntryOptions {
    fn default() -> Self {
        Self { lead: 0.5, spread: true }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EntryReport {
    /// Track ends that now leave their pad at a right angle.
    pub aligned: usize,
    /// Pads where two tracks were moved to the two ends.
    pub spread: usize,
}

/// A track end on a pad.
#[derive(Clone, Copy)]
struct End {
    track: usize,
    is_a: bool,
}

fn end_pos(board: &Board, e: &End) -> Point {
    let t = &board.tracks[e.track];
    if e.is_a { t.a } else { t.b }
}

fn far_pos(board: &Board, e: &End) -> Point {
    let t = &board.tracks[e.track];
    if e.is_a { t.b } else { t.a }
}

/// What to do at one track end.
struct Edit {
    /// The track on the pad (for telling edits apart).
    track: usize,
    /// A track that is cut short: (its index, which end, where it now begins). `None` when the way is rerouted up to
    /// the end of a track that stays as it is.
    trim: Option<(usize, bool, Point)>,
    /// The new tracks from the pad to the joint.
    new: Vec<Track>,
    /// Short tracks between the pad and the joint that are no longer needed (indices).
    remove: Vec<usize>,
}

/// The inset (mm) of the snap points at the ends of a pad besides half a track width (as in the editor).
const END_INSET: f64 = 0.05;

/// The shortest (mm) piece joining the lead to the old route: shorter than this the bend shows as a step.
const MIN_LEG: f64 = 0.25;

fn unit(from: Point, to: Point) -> (f64, f64, f64) {
    let l = from.dist(to);
    if l < 1e-9 { (0.0, 0.0, 0.0) } else { ((to.x - from.x) / l, (to.y - from.y) / l, l) }
}

fn pad_holds(q: &Pad, p: Point, side: Side) -> bool {
    let (w, h) = q.shape.size();
    !matches!(q.shape, PadShape::Circle { .. })
        && (q.drill.is_some() || q.side == side)
        && (p.x - q.pos.x).abs() <= w / 2.0 + 1e-6
        && (p.y - q.pos.y).abs() <= h / 2.0 + 1e-6
}

/// The unit direction out of the pad's edge for a track leaving `anchor` towards `far`.
fn face_normal(pad: &Pad, anchor: Point, far: Point) -> (f64, f64) {
    let (w, h) = pad.shape.size();
    let (ox, oy) = (anchor.x - pad.pos.x, anchor.y - pad.pos.y);
    let (dx, dy) = (far.x - anchor.x, far.y - anchor.y);
    let sign = |v: f64| if v >= 0.0 { 1.0 } else { -1.0 };
    if ox.abs() > 1e-6 && oy.abs() <= 1e-6 {
        (0.0, sign(dy)) // an end of the pad: out through the long side
    } else if (oy.abs() > 1e-6 && ox.abs() <= 1e-6) || dx.abs() * h >= dy.abs() * w {
        (sign(dx), 0.0)
    } else {
        (0.0, sign(dy))
    }
}

/// How far (mm) from `anchor` the pad's edge is, going in direction `n` (axis-aligned).
fn exit_distance(pad: &Pad, anchor: Point, n: (f64, f64)) -> f64 {
    let (w, h) = pad.shape.size();
    if n.0 != 0.0 { w / 2.0 - n.0 * (anchor.x - pad.pos.x) } else { h / 2.0 - n.1 * (anchor.y - pad.pos.y) }.max(0.0)
}

/// How far (mm) from `p` (inside the pad) the pad's edge is, going in the direction `u`.
fn exit_along(pad: &Pad, p: Point, u: (f64, f64)) -> f64 {
    let (w, h) = pad.shape.size();
    let axis = |half: f64, off: f64, d: f64| if d.abs() > 1e-9 { (half * d.signum() - off) / d } else { f64::INFINITY };
    axis(w / 2.0, p.x - pad.pos.x, u.0).min(axis(h / 2.0, p.y - pad.pos.y, u.1)).max(0.0)
}

/// The tracks that run on from the pad along `e`: the one on the pad, then the ones joined to its far end while they
/// are only two at a joint (same layer and width, off any pad or via), as far as `reach` mm. Each is (index, which end
/// is the near one, near point, far point).
fn chain(board: &Board, e: End, reach: f64) -> Vec<(usize, bool, Point, Point)> {
    let t = &board.tracks[e.track];
    let (v, w) = if e.is_a { (t.a, t.b) } else { (t.b, t.a) };
    let mut out = vec![(e.track, e.is_a, v, w)];
    let mut acc = v.dist(w);
    while (acc < reach || out.len() < 2) && out.len() < 6 {
        let (cur, _, _, far) = *out.last().unwrap();
        let side = board.tracks[cur].side;
        if crate::shove::pinned(board, far, side) {
            break;
        }
        // the other tracks that end at `far`
        let mut next: Vec<(usize, bool)> = vec![];
        for (k, u) in board.tracks.iter().enumerate().filter(|(k, u)| *k != cur && u.side == side) {
            if u.a.dist(far) < 1e-6 {
                next.push((k, true));
            }
            if u.b.dist(far) < 1e-6 {
                next.push((k, false));
            }
        }
        let [(k, near_is_a)] = next[..] else { break };
        if out.iter().any(|c| c.0 == k) || (board.tracks[k].width - board.tracks[cur].width).abs() > 0.02 {
            break;
        }
        let nt = &board.tracks[k];
        let other = if near_is_a { nt.b } else { nt.a };
        acc += far.dist(other);
        out.push((k, near_is_a, far, other));
    }
    out
}

/// The edit that makes the track end `e` leave `pad` from `anchor` at a right angle (`None`: nothing to do or not possible).
fn plan_end(board: &Board, pad: &Pad, e: End, anchor: Point, lead: f64) -> Option<Edit> {
    let t = &board.tracks[e.track];
    let (v, w) = if e.is_a { (t.a, t.b) } else { (t.b, t.a) };
    let (ux, uy, len) = unit(v, w);
    if len < 1e-6 {
        return None;
    }
    let n = face_normal(pad, anchor, w);
    let exit = exit_distance(pad, anchor, n);
    // already leaving at a right angle, from the right place
    if anchor.dist(v) < 1e-6 && (ux * n.0 + uy * n.1) > 0.9999 {
        return None;
    }
    let q = Point::new(anchor.x + n.0 * (exit + lead), anchor.y + n.1 * (exit + lead));
    // an end that was moved onto the centre line may not leave a track after the joint that is only a little step
    let tail = if anchor.dist(v) > 1e-6 { MIN_LEG } else { 0.2 };
    // the joint: where a horizontal, vertical or 45° line from `q` meets the old route (the track on the pad, or the
    // ones joined to it), beyond the pad
    let s_min = exit_along(pad, v, (ux, uy)) + 0.1;
    let links = chain(board, e, s_min + 2.0 * lead + 1.0);
    // the end sticks out over the pad's edge: move the whole straight run onto the pad's centre line, the next track
    // keeps its line and only begins a little further along it (no jog)
    if anchor.dist(v) > 1e-6 && let Some(edit) = shift_run(board, pad, e, anchor, n, exit, lead, &links) {
        return Some(edit);
    }
    if anchor.dist(v) < 1e-6 && let Some(edit) = slide_foot(board, pad, e, anchor, n, exit, lead, &links) {
        return Some(edit);
    }
    let mut best: Option<(f64, usize, Point, f64)> = None; // (score, index in `links`, joint, the sharpest turn)
    for (idx, &(_, _, p0, p1)) in links.iter().enumerate() {
        let (sx, sy, sl) = unit(p0, p1);
        for k in 0..8 {
            let a = k as f64 * std::f64::consts::FRAC_PI_4;
            let d = (a.cos(), a.sin());
            // q + t d = p0 + s u
            let det = d.0 * (-sy) - d.1 * (-sx);
            if det.abs() < 1e-6 {
                continue;
            }
            let (rx, ry) = (p0.x - q.x, p0.y - q.y);
            let tt = (rx * (-sy) - ry * (-sx)) / det;
            let ss = (d.0 * ry - d.1 * rx) / det;
            let s_lo = if idx == 0 { s_min } else { 0.05 };
            if tt < 0.05 || ss < s_lo || ss > sl - tail {
                continue;
            }
            // a short bend would only be a visible little step
            if tt < MIN_LEG && d.0 * n.0 + d.1 * n.1 < 0.9999 {
                continue;
            }
            let j = Point::new(p0.x + sx * ss, p0.y + sy * ss);
            if pad_holds(pad, j, t.side) {
                continue;
            }
            let turn_q = (d.0 * n.0 + d.1 * n.1).clamp(-1.0, 1.0).acos();
            let turn_j = (d.0 * sx + d.1 * sy).clamp(-1.0, 1.0).acos();
            if turn_q > 1.7 {
                continue; // not back towards the pad
            }
            let score = tt + 0.3 * (turn_q + turn_j);
            let sharpest = turn_q.max(turn_j);
            // a gentler fold wins; among about equally gentle ones, the shorter way
            if best.is_none_or(|b| sharpest + 0.1 < b.3 || (sharpest < b.3 + 0.1 && score < b.0)) {
                best = Some((score, idx, j, sharpest));
            }
        }
    }
    let (side, width) = (t.side, t.width);
    // the other way: lead out of the pad, then the plain 45° route to the end of one of the short tracks, so that no
    // turn is sharper than it has to be (the join above turns 90° when the old route runs at 45°)
    let mut reroute: Option<(f64, f64, usize, Vec<Point>)> = None; // (sharpest turn, length, link, points after q)
    for (k, link) in links.iter().enumerate() {
        let w_k = link.3;
        // the way on from there: the next short track, or (at the last one) nothing when it ends on a pad or via
        let next = if k + 1 < links.len() {
            Some(unit(links[k + 1].2, links[k + 1].3))
        } else if crate::shove::pinned(board, w_k, t.side) {
            None
        } else {
            continue;
        };
        if q.dist(w_k) < 0.3 {
            continue;
        }
        for flip in [false, true] {
            let pts = bend45(q, w_k, flip);
            let mut prev = q;
            let mut dir = n;
            let mut sharpest = 0.0f64;
            let mut length = 0.0;
            let mut ok = true;
            for &p in &pts {
                let (dx, dy, l) = unit(prev, p);
                if l < MIN_LEG {
                    ok = false;
                    break;
                }
                sharpest = sharpest.max((dx * dir.0 + dy * dir.1).clamp(-1.0, 1.0).acos());
                length += l;
                dir = (dx, dy);
                prev = p;
            }
            if !ok {
                continue;
            }
            if let Some(next) = next {
                sharpest = sharpest.max((dir.0 * next.0 + dir.1 * next.1).clamp(-1.0, 1.0).acos());
            }
            if sharpest > 1.7 {
                continue;
            }
            if reroute.as_ref().is_none_or(|r| (sharpest, length) < (r.0, r.1)) {
                reroute = Some((sharpest, length, k, pts));
            }
        }
    }
    let use_reroute = match (&best, &reroute) {
        (Some(b), Some(r)) => r.0 + 0.1 < b.3,
        (None, Some(_)) => true,
        _ => false,
    };
    if use_reroute {
        let (_, _, k, pts) = reroute?;
        let mut new = vec![];
        let first_dir = unit(q, pts[0]);
        if (first_dir.0 * n.0 + first_dir.1 * n.1) > 0.9999 {
            new.push(Track { side, a: anchor, b: pts[0], width });
        } else {
            new.push(Track { side, a: anchor, b: q, width });
            new.push(Track { side, a: q, b: pts[0], width });
        }
        for w in pts.windows(2) {
            new.push(Track { side, a: w[0], b: w[1], width });
        }
        let remove = links[..=k].iter().map(|l| l.0).collect();
        return Some(Edit { track: e.track, trim: None, new, remove });
    }
    let (_, idx, joint, _) = best?;
    let mid_dir = unit(q, joint);
    let mut new = vec![];
    if (mid_dir.0 * n.0 + mid_dir.1 * n.1) > 0.9999 {
        new.push(Track { side, a: anchor, b: joint, width }); // the lead runs on to the joint
    } else {
        new.push(Track { side, a: anchor, b: q, width });
        new.push(Track { side, a: q, b: joint, width });
    }
    let (track, is_a, _, _) = links[idx];
    let remove = links[..idx].iter().map(|l| l.0).collect();
    Some(Edit { track: e.track, trim: Some((track, is_a, joint)), new, remove })
}

/// A pad that sits a hair off the route's grid line (the router comes to the pad's centre from the grid) would get a
/// tiny 45° jog to join the line. When a track further on already runs straight away from the pad's edge, only a
/// little to the side, slide the foot of the lead along the pad's edge onto that line instead: one straight track.
#[allow(clippy::too_many_arguments)]
fn slide_foot(board: &Board, pad: &Pad, e: End, anchor: Point, n: (f64, f64), exit: f64, lead: f64, links: &[(usize, bool, Point, Point)]) -> Option<Edit> {
    let t = &board.tracks[e.track];
    let (pw, ph) = pad.shape.size();
    let perp = (-n.1, n.0);
    // how far the foot may move sideways and still keep the whole track width on the pad
    let room = (if n.0 != 0.0 { ph } else { pw }) / 2.0 - t.width / 2.0 - END_INSET;
    for (idx, &(track, is_a, p0, p1)) in links.iter().enumerate().skip(1) {
        let (sx, sy, sl) = unit(p0, p1);
        if sx * n.0 + sy * n.1 < 0.9999 {
            continue; // not running straight away from the pad
        }
        let lat = (p0.x - anchor.x) * perp.0 + (p0.y - anchor.y) * perp.1;
        let off = (anchor.x - pad.pos.x) * perp.0 + (anchor.y - pad.pos.y) * perp.1 + lat;
        if lat.abs() < 1e-6 || lat.abs() > MAX_SLIDE || off.abs() > room {
            continue;
        }
        let foot = Point::new(anchor.x + perp.0 * lat, anchor.y + perp.1 * lat);
        // the lead is at least `lead` long outside the pad's edge; the link starts further out or is cut there
        let ahead = (p0.x - foot.x) * n.0 + (p0.y - foot.y) * n.1;
        let s = (exit + lead - ahead).max(0.0);
        if ahead < 0.05 + exit || s > sl - 0.2 {
            continue;
        }
        let joint = Point::new(p0.x + sx * s, p0.y + sy * s);
        let remove = links[..idx].iter().map(|l| l.0).collect();
        return Some(Edit { track: e.track, trim: Some((track, is_a, joint)), new: vec![Track { side: t.side, a: foot, b: joint, width: t.width }], remove });
    }
    None
}

/// The straight track on the pad (running straight away from its edge, `links[0]`) is laid again on the pad's centre line
/// from `anchor`; the track that follows keeps its line and begins where that line meets the centre line. `None` when the
/// run does not leave straight, the next track is parallel to it, or the lead would be shorter than `lead`.
#[allow(clippy::too_many_arguments)]
fn shift_run(board: &Board, pad: &Pad, e: End, anchor: Point, n: (f64, f64), exit: f64, lead: f64, links: &[(usize, bool, Point, Point)]) -> Option<Edit> {
    let t = &board.tracks[e.track];
    let (_, _, p0, p1) = *links.first()?;
    let (sx, sy, _) = unit(p0, p1);
    if sx * n.0 + sy * n.1 < 0.9999 {
        return None;
    }
    let &(next, is_a, q0, q1) = links.get(1)?;
    let (dx, dy, dl) = unit(q0, q1);
    // anchor + s n = q0 + u d
    let det = n.0 * (-dy) - n.1 * (-dx);
    if det.abs() < 1e-6 {
        return None;
    }
    let (rx, ry) = (q0.x - anchor.x, q0.y - anchor.y);
    let s = (rx * (-dy) - ry * (-dx)) / det;
    let u = (n.0 * ry - n.1 * rx) / det;
    // the track that follows keeps at least `MIN_LEG`, so that it does not become a little step
    if s < exit + lead || u < 0.05 || u > dl - MIN_LEG {
        return None;
    }
    let joint = Point::new(q0.x + dx * u, q0.y + dy * u);
    if pad_holds(pad, joint, t.side) {
        return None;
    }
    Some(Edit { track: e.track, trim: Some((next, is_a, joint)), new: vec![Track { side: t.side, a: anchor, b: joint, width: t.width }], remove: vec![links[0].0] })
}

/// The most (mm) the foot of a lead is slid along the pad's edge to meet a track that runs on beside it.
const MAX_SLIDE: f64 = 0.3;

/// The points to visit from `a` to `p` (the last is `p`) with horizontal, vertical and 45° legs; `flip` picks which of
/// the two bends is used.
pub(crate) fn bend45(a: Point, p: Point, flip: bool) -> Vec<Point> {
    let (dx, dy) = (p.x - a.x, p.y - a.y);
    let eps = 1e-9;
    if dx.abs() < eps || dy.abs() < eps || (dx.abs() - dy.abs()).abs() < eps {
        return vec![p];
    }
    let d = dx.abs().min(dy.abs());
    let (sx, sy) = (dx.signum(), dy.signum());
    let diag_first = Point::new(a.x + sx * d, a.y + sy * d);
    let straight_first = if dx.abs() > dy.abs() { Point::new(a.x + sx * (dx.abs() - d), a.y) } else { Point::new(a.x, a.y + sy * (dy.abs() - d)) };
    vec![if flip { diag_first } else { straight_first }, p]
}

/// Apply the edits to `board`; returns the indices of the tracks that were removed (the later indices move down).
fn apply(board: &mut Board, edits: &[&Edit]) -> Vec<usize> {
    for e in edits {
        if let Some((i, is_a, joint)) = e.trim {
            if is_a {
                board.tracks[i].a = joint;
            } else {
                board.tracks[i].b = joint;
            }
        }
        board.tracks.extend(e.new.iter().cloned());
    }
    let mut gone: Vec<usize> = edits.iter().flat_map(|e| e.remove.iter().copied()).collect();
    gone.sort_unstable();
    gone.dedup();
    for &i in gone.iter().rev() {
        board.tracks.remove(i);
    }
    gone
}

/// Move a list of track indices down past the removed ones.
fn shift(only: &mut Option<Vec<usize>>, gone: &[usize]) {
    if let Some(list) = only {
        list.retain(|i| !gone.contains(i));
        for i in list.iter_mut() {
            *i -= gone.iter().filter(|g| **g < *i).count();
        }
    }
}

/// The two anchors at the ends of the long axis of `pad` for tracks of width `w`, if there is room for two tracks.
fn end_anchors(board: &Board, pad: &Pad, w: f64) -> Option<(Point, Point, (f64, f64))> {
    let (pw, ph) = pad.shape.size();
    if (pw - ph).abs() < 1e-6 {
        return None; // a square pad has no long axis
    }
    let along_x = pw > ph;
    let len = pw.max(ph);
    let off = len / 2.0 - w / 2.0 - END_INSET;
    if off <= 0.0 || 2.0 * off < w + board.rules.min_clearance {
        return None;
    }
    let axis = if along_x { (1.0, 0.0) } else { (0.0, 1.0) };
    Some((Point::new(pad.pos.x - axis.0 * off, pad.pos.y - axis.1 * off), Point::new(pad.pos.x + axis.0 * off, pad.pos.y + axis.1 * off), axis))
}

/// A track that ends on its pad off to one side (the router comes to the grid line, which may miss the pad's centre by a
/// little) can stick out over the pad's edge: the copper then crowds the next pad or track. For such an end, the point on the
/// pad's centre line where the lead should start; `None` when the whole track width is on the pad already.
fn recentred(board: &Board, pad: &Pad, e: &End) -> Option<Point> {
    let t = &board.tracks[e.track];
    let (v, w) = (end_pos(board, e), far_pos(board, e));
    let n = face_normal(pad, v, w);
    let perp = (-n.1, n.0);
    let (pw, ph) = pad.shape.size();
    let half = (if n.0 != 0.0 { ph } else { pw }) / 2.0;
    let off = (v.x - pad.pos.x) * perp.0 + (v.y - pad.pos.y) * perp.1;
    if off.abs() + t.width / 2.0 <= half + 1e-6 {
        return None;
    }
    Some(Point::new(v.x - perp.0 * off, v.y - perp.1 * off))
}

/// Make the tracks of `board` leave their rectangular pads at a right angle (see the module). Only the track ends
/// listed in `only` (indices of tracks), when given, are handled. Returns what was done.
pub fn align_entries(board: &mut Board, opts: &EntryOptions, only: Option<&[usize]>) -> EntryReport {
    let mut report = EntryReport::default();
    let mut only: Option<Vec<usize>> = only.map(<[usize]>::to_vec);
    for pi in 0..board.pads.len() {
        let pad = board.pads[pi].clone();
        if matches!(pad.shape, PadShape::Circle { .. }) {
            continue;
        }
        // the track ends on this pad
        let mut ends: Vec<End> = vec![];
        for (i, t) in board.tracks.iter().enumerate().filter(|(i, _)| only.as_ref().is_none_or(|o| o.contains(i))) {
            for (is_a, p) in [(true, t.a), (false, t.b)] {
                if pad_holds(&pad, p, t.side) {
                    ends.push(End { track: i, is_a });
                }
            }
        }
        if ends.is_empty() {
            continue;
        }
        // two tracks from one point, and room for both at the ends of the pad: spread them
        let mut spread: Option<[(End, Point); 2]> = None;
        if opts.spread && ends.len() == 2 && end_pos(board, &ends[0]).dist(end_pos(board, &ends[1])) < 0.05 && board.tracks[ends[0].track].side == board.tracks[ends[1].track].side {
            let w = board.tracks[ends[0].track].width.max(board.tracks[ends[1].track].width);
            if let Some((lo, hi, axis)) = end_anchors(board, &pad, w) {
                // the one whose far end lies further along the axis gets the far anchor: no crossing
                let lat = |e: &End| {
                    let f = far_pos(board, e);
                    (f.x - pad.pos.x) * axis.0 + (f.y - pad.pos.y) * axis.1
                };
                let (first, second) = if lat(&ends[0]) <= lat(&ends[1]) { (ends[0], ends[1]) } else { (ends[1], ends[0]) };
                spread = Some([(first, lo), (second, hi)]);
            }
        }
        if let Some(sp) = spread {
            let edits: Vec<Edit> = sp.iter().filter_map(|(e, a)| plan_end(board, &pad, *e, *a, opts.lead)).collect();
            // both edits must be on different tracks (a chain may not run into the other one's)
            let apart = edits.len() == 2 && edits[0].track != edits[1].track && !edits[0].remove.iter().any(|r| edits[1].remove.contains(r) || *r == edits[1].track) && !edits[1].remove.iter().any(|r| *r == edits[0].track);
            if apart {
                let before = (check(board).len(), ratsnest(board).len());
                let mut trial = board.clone();
                let gone = apply(&mut trial, &edits.iter().collect::<Vec<_>>());
                if check(&trial).len() <= before.0 && ratsnest(&trial).len() <= before.1 {
                    *board = trial;
                    shift(&mut only, &gone);
                    if let Some(list) = only.as_mut() {
                        let added: usize = edits.iter().map(|e| e.new.len()).sum();
                        list.extend(board.tracks.len() - added..board.tracks.len());
                    }
                    report.spread += 1;
                    report.aligned += 2;
                    continue;
                }
            }
        }
        for k in 0..ends.len() {
            let e = ends[k];
            let anchor = recentred(board, &pad, &e).unwrap_or_else(|| end_pos(board, &e));
            let Some(edit) = plan_end(board, &pad, e, anchor, opts.lead) else { continue };
            let before = (check(board).len(), ratsnest(board).len());
            let mut trial = board.clone();
            let gone = apply(&mut trial, &[&edit]);
            if check(&trial).len() <= before.0 && ratsnest(&trial).len() <= before.1 {
                *board = trial;
                shift(&mut only, &gone);
                // the tracks that were laid belong to the work too (their other end may be on another pad)
                if let Some(list) = only.as_mut() {
                    list.extend(board.tracks.len() - edit.new.len()..board.tracks.len());
                }
                // the later ends of this pad are on tracks whose numbers may have moved
                for later in ends.iter_mut().skip(k + 1) {
                    later.track -= gone.iter().filter(|g| **g < later.track).count();
                }
                report.aligned += 1;
            }
        }
    }
    report
}
