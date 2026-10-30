//! The tracks as ribbons: the tracks that join end to end (same layer, same width) are followed into one chain, and a chain is drawn as
//! one strip of triangles with a round cap at each end, not as a line and two discs for every little piece. A curve made of many short
//! tracks (the round corners of the finish) is then two triangles a piece, and the joins between pieces are mitred, so nothing is drawn
//! twice. The edge of the strip fades out over a pixel, like the lines of egui, so it is not stepped.

use eframe::egui::{Color32, Mesh, Pos2, Rect, Vec2};
use pcb_core::smooth::Finished;
use pcb_core::{Point, Side, Track};
use std::collections::HashMap;
use std::rc::Rc;

/// Tracks that join end to end, on one layer and with one width.
pub(super) struct Chain {
    pub side: Side,
    pub width: f64,
    pub pts: Vec<Point>,
    /// The corners of the box round the points (not grown by the width).
    pub lo: Point,
    pub hi: Point,
}

/// A polygon of the finish (a teardrop, a notch).
pub(super) struct PatchTris {
    pub side: Side,
    pub lo: Point,
    pub hi: Point,
    pub outline: Vec<Point>,
}

/// The copper of the tracks ready to be drawn: what the chains and patches were made from is kept to know when they are out of date.
pub(super) struct CopperCache {
    source: Source,
    pub chains: Vec<Chain>,
    pub patches: Vec<PatchTris>,
    /// The points of the chains and the triangles of the patches, with the corners left out that are less than a tolerance off the line
    /// (see [`LODS`]); worked out the first time a zoom needs them.
    levels: std::cell::RefCell<Vec<Option<Rc<Lod>>>>,
}

/// The chains' points and the patches' triangles at one level of detail (in the order of the cache's chains and patches).
pub(super) struct Lod {
    pub chains: Vec<Vec<Point>>,
    pub patches: Vec<Vec<[Point; 3]>>,
}

/// The tolerances (mm) of the levels of detail. At a zoom the level is the coarsest one that is under a quarter of a pixel.
const LODS: [f64; 12] = [0.0, 0.002, 0.004, 0.008, 0.016, 0.032, 0.064, 0.125, 0.25, 0.5, 1.0, 2.0];

enum Source {
    /// The plain tracks of the board, by a hash of them.
    Plain(u64),
    /// The finished copper (the cache holds it, so a new one can not be taken for it).
    Finished(Rc<Finished>),
}

impl CopperCache {
    /// Is this cache made from this copper?
    pub fn is_for(&self, tracks: &[Track], finished: Option<&Rc<Finished>>) -> bool {
        match (&self.source, finished) {
            (Source::Finished(a), Some(b)) => Rc::ptr_eq(a, b),
            (Source::Plain(k), None) => *k == tracks_hash(tracks),
            _ => false,
        }
    }

    pub fn new(tracks: &[Track], finished: Option<&Rc<Finished>>) -> Self {
        match finished {
            Some(f) => {
                let patches = f
                    .patches
                    .iter()
                    .filter(|p| p.outline.len() >= 3)
                    .map(|p| {
                        let (lo, hi) = bounds(&p.outline);
                        PatchTris { side: p.side, lo, hi, outline: p.outline.clone() }
                    })
                    .collect();
                CopperCache { source: Source::Finished(f.clone()), chains: chains(&f.board.tracks), patches, levels: Default::default() }
            }
            None => CopperCache { source: Source::Plain(tracks_hash(tracks)), chains: chains(tracks), patches: vec![], levels: Default::default() },
        }
    }

    /// The level of detail for a view of `zoom` pixels to the mm.
    pub fn lod(&self, zoom: f32) -> Rc<Lod> {
        let level = lod_level(zoom);
        let mut levels = self.levels.borrow_mut();
        levels.resize(LODS.len(), None);
        levels[level]
            .get_or_insert_with(|| {
                let tol = LODS[level];
                Rc::new(Lod {
                    chains: self.chains.iter().map(|c| if tol > 0.0 { simplify_open(&c.pts, tol) } else { c.pts.clone() }).collect(),
                    patches: self
                        .patches
                        .iter()
                        .map(|p| {
                            let outline = if tol > 0.0 { simplify_closed(&p.outline, tol) } else { p.outline.clone() };
                            pcb_core::zonefill::FillShape { outer: outline, holes: vec![] }.triangles()
                        })
                        .collect(),
                })
            })
            .clone()
    }
}

/// The level of detail (an index of [`LODS`]) for a view of `zoom` pixels to the mm: the coarsest one that is under a quarter of a pixel.
pub(super) fn lod_level(zoom: f32) -> usize {
    lod_level_px(zoom, 0.25)
}

/// The coarsest level that is under `px` pixels at the zoom (a thin line can do with a coarser one than a filled shape).
pub(super) fn lod_level_px(zoom: f32, px: f64) -> usize {
    let tol = px / zoom.max(1e-6) as f64;
    LODS.iter().rposition(|&t| t <= tol).unwrap_or(0)
}

/// The tolerance (mm) of a level of detail.
pub(super) fn lod_tolerance(level: usize) -> f64 {
    LODS[level.min(LODS.len() - 1)]
}

/// How far `p` is from the segment `a`-`b`.
fn off_segment(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    if l2 < 1e-18 {
        return p.dist(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0);
    p.dist(Point::new(a.x + dx * t, a.y + dy * t))
}

/// Douglas-Peucker: the line through the points with the ones left out that are less than `tol` from it; the ends stay.
pub(super) fn simplify_open(pts: &[Point], tol: f64) -> Vec<Point> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0, pts.len() - 1)];
    while let Some((i, j)) = stack.pop() {
        let far = (i + 1..j).map(|k| (k, off_segment(pts[k], pts[i], pts[j]))).max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((k, d)) = far
            && d > tol
        {
            keep[k] = true;
            stack.push((i, k));
            stack.push((k, j));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// [`simplify_open`] for a polygon (cut at the point farthest from its first corner); a polygon that would be left with fewer than 3 corners
/// stays as it is.
pub(super) fn simplify_closed(pts: &[Point], tol: f64) -> Vec<Point> {
    if pts.len() < 4 {
        return pts.to_vec();
    }
    let far = (1..pts.len()).max_by(|&a, &b| pts[0].dist(pts[a]).total_cmp(&pts[0].dist(pts[b]))).unwrap_or(1);
    let mut a = simplify_open(&pts[..=far], tol);
    let mut closing: Vec<Point> = pts[far..].to_vec();
    closing.push(pts[0]);
    let b = simplify_open(&closing, tol);
    a.pop();
    a.extend(b);
    a.pop();
    if a.len() < 3 { pts.to_vec() } else { a }
}

/// The silkscreen (lines and the strokes of the texts) followed into chains like the tracks, kept until the silk changes. The lines and
/// texts of parts are apart from the others: they are shown with the parts.
pub(super) struct SilkCache {
    key: u64,
    pub plain: Vec<Chain>,
    pub of_parts: Vec<Chain>,
}

impl SilkCache {
    pub fn is_for(&self, board: &pcb_core::Board) -> bool {
        self.key == silk_hash(board)
    }

    pub fn new(board: &pcb_core::Board) -> Self {
        let (mut plain, mut of_parts): (Vec<Track>, Vec<Track>) = (vec![], vec![]);
        for l in &board.silk {
            let t = Track { side: l.side, a: l.a, b: l.b, width: l.width };
            if l.refdes.is_empty() { plain.push(t) } else { of_parts.push(t) }
        }
        for t in &board.texts {
            // as thick as the drawing of a text has always been
            let width = (t.height * 0.15).max(0.12);
            let dst = if t.refdes.is_empty() { &mut plain } else { &mut of_parts };
            for [a, b] in t.strokes() {
                dst.push(Track { side: t.side, a: Point::new(t.pos.x + a.x, t.pos.y + a.y), b: Point::new(t.pos.x + b.x, t.pos.y + b.y), width });
            }
        }
        SilkCache { key: silk_hash(board), plain: chains(&plain), of_parts: chains(&of_parts) }
    }
}

fn silk_hash(board: &pcb_core::Board) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for l in &board.silk {
        (l.a.x.to_bits(), l.a.y.to_bits(), l.b.x.to_bits(), l.b.y.to_bits(), l.width.to_bits(), l.side, l.refdes.is_empty()).hash(&mut h);
    }
    for t in &board.texts {
        (t.pos.x.to_bits(), t.pos.y.to_bits(), t.height.to_bits(), &t.text, t.rot, t.side, t.refdes.is_empty()).hash(&mut h);
    }
    h.finish()
}

fn tracks_hash(tracks: &[Track]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tracks.len().hash(&mut h);
    for t in tracks {
        (t.a.x.to_bits(), t.a.y.to_bits(), t.b.x.to_bits(), t.b.y.to_bits(), t.width.to_bits(), t.side).hash(&mut h);
    }
    h.finish()
}

fn bounds(pts: &[Point]) -> (Point, Point) {
    let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
    for p in pts {
        lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    (lo, hi)
}

type NodeKey = (Side, u64, i64, i64);

fn node_key(t: &Track, p: Point) -> NodeKey {
    // a micrometre: ends that close are one joint
    (t.side, t.width.to_bits(), (p.x * 1000.0).round() as i64, (p.y * 1000.0).round() as i64)
}

/// A turn of more than 120 degrees would need a mitre of more than twice the half width: there the chain is cut and both pieces get a cap.
const SHARPEST_TURN_DOT: f64 = -0.5;

/// The tracks followed into chains: through every joint where exactly two track ends meet.
pub(super) fn chains(tracks: &[Track]) -> Vec<Chain> {
    let mut at: HashMap<NodeKey, Vec<(usize, bool)>> = HashMap::new();
    for (i, t) in tracks.iter().enumerate() {
        at.entry(node_key(t, t.a)).or_default().push((i, false));
        at.entry(node_key(t, t.b)).or_default().push((i, true));
    }
    let mut seen = vec![false; tracks.len()];
    let mut out = vec![];
    // walks on from the end `to_b` of track `i` (the end the walk has not been at)
    let walk = |i: usize, from_b: bool, seen: &mut Vec<bool>| -> Vec<Point> {
        let t = &tracks[i];
        let mut pts = vec![if from_b { t.b } else { t.a }];
        let (mut cur, mut at_b) = (i, from_b);
        loop {
            seen[cur] = true;
            let t = &tracks[cur];
            let far = if at_b { t.a } else { t.b };
            pts.push(far);
            let here = &at[&node_key(t, far)];
            if here.len() != 2 {
                break;
            }
            // the other track end at this joint
            let Some(&(next, next_b)) = here.iter().find(|&&(n, nb)| !(n == cur && nb == !at_b)) else { break };
            if seen[next] || next == cur {
                break;
            }
            cur = next;
            // the walk starts at the end of the next track that is here
            at_b = next_b;
        }
        pts
    };
    let push = |side: Side, width: f64, pts: Vec<Point>, out: &mut Vec<Chain>| {
        for piece in cut_sharp(&dedup(pts)) {
            let (lo, hi) = bounds(&piece);
            out.push(Chain { side, width, pts: piece, lo, hi });
        }
    };
    // chains start at the ends that are not a plain joint of two
    for (i, t) in tracks.iter().enumerate() {
        if seen[i] {
            continue;
        }
        for from_b in [false, true] {
            let p = if from_b { t.b } else { t.a };
            if !seen[i] && at[&node_key(t, p)].len() != 2 {
                let pts = walk(i, from_b, &mut seen);
                push(t.side, t.width, pts, &mut out);
            }
        }
    }
    // what is left lies on rings
    for (i, t) in tracks.iter().enumerate() {
        if !seen[i] {
            let pts = walk(i, false, &mut seen);
            push(t.side, t.width, pts, &mut out);
        }
    }
    out
}

/// The points with the repeated ones and those on the straight line between the neighbours left out.
fn dedup(pts: Vec<Point>) -> Vec<Point> {
    let mut v: Vec<Point> = vec![];
    for p in pts {
        if v.last().is_none_or(|q| q.dist(p) > 1e-6) {
            v.push(p);
        }
    }
    let mut out: Vec<Point> = vec![];
    for (k, &p) in v.iter().enumerate() {
        if let (Some(&a), Some(&n)) = (out.last(), v.get(k + 1)) {
            let (d0, d1) = ((p.x - a.x, p.y - a.y), (n.x - p.x, n.y - p.y));
            let len = (n.x - a.x).hypot(n.y - a.y);
            let off = ((n.x - a.x) * (p.y - a.y) - (n.y - a.y) * (p.x - a.x)).abs() / len.max(1e-12);
            if off < 1e-4 && d0.0 * d1.0 + d0.1 * d1.1 > 0.0 {
                continue;
            }
        }
        out.push(p);
    }
    out
}

/// A chain cut where it turns back on itself (see [`SHARPEST_TURN_DOT`]); the cut point belongs to both pieces.
fn cut_sharp(pts: &[Point]) -> Vec<Vec<Point>> {
    let mut out = vec![];
    let mut cur: Vec<Point> = vec![];
    for (k, &p) in pts.iter().enumerate() {
        cur.push(p);
        if k > 0 && k + 1 < pts.len() {
            let (a, n) = (pts[k - 1], pts[k + 1]);
            let (d0, d1) = (unit(a, p), unit(p, n));
            if d0.0 * d1.0 + d0.1 * d1.1 < SHARPEST_TURN_DOT {
                out.push(std::mem::take(&mut cur));
                cur.push(p);
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn unit(a: Point, b: Point) -> (f64, f64) {
    let l = a.dist(b).max(1e-12);
    ((b.x - a.x) / l, (b.y - a.y) / l)
}

/// One chain as a strip, `width` pixels wide (at least a pixel), with round caps; `pts` are on the screen.
pub(super) fn add_chain(mesh: &mut Mesh, pts: &[Pos2], width: f32, col: Color32) {
    let mut p: Vec<Pos2> = vec![];
    for &q in pts {
        if p.last().is_none_or(|l| (*l - q).length() > 1e-3) {
            p.push(q);
        }
    }
    if p.is_empty() {
        return;
    }
    let half = width.max(1.0) / 2.0;
    let (core, outer) = ((half - 0.5).max(0.0), half + 0.5);
    let clear = Color32::TRANSPARENT;
    if p.len() == 1 {
        cap(mesh, p[0], Vec2::new(1.0, 0.0), core, outer, col);
        cap(mesh, p[0], Vec2::new(-1.0, 0.0), core, outer, col);
        return;
    }
    let dirs: Vec<Vec2> = p.windows(2).map(|w| (w[1] - w[0]).normalized()).collect();
    let normal = |d: Vec2| Vec2::new(-d.y, d.x);
    let mut prev_row: Option<[u32; 4]> = None;
    for (k, &q) in p.iter().enumerate() {
        // the way out to the side at this point: the mitre of the two segments that meet here
        let m = match (k.checked_sub(1).map(|i| dirs[i]), dirs.get(k).copied()) {
            (Some(a), Some(b)) => {
                let (n0, n1) = (normal(a), normal(b));
                (n0 + n1) / (1.0 + n0.dot(n1)).max(0.5)
            }
            (Some(a), None) => normal(a),
            (None, Some(b)) => normal(b),
            (None, None) => unreachable!(),
        };
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(q + m * outer, clear);
        mesh.colored_vertex(q + m * core, col);
        mesh.colored_vertex(q - m * core, col);
        mesh.colored_vertex(q - m * outer, clear);
        let row = [base, base + 1, base + 2, base + 3];
        if let Some(r) = prev_row {
            for j in 0..3 {
                mesh.add_triangle(r[j], r[j + 1], row[j]);
                mesh.add_triangle(r[j + 1], row[j + 1], row[j]);
            }
        }
        prev_row = Some(row);
    }
    // a line that is only a pixel or two wide has an end that no one can tell from a round one
    if width < ROUND_ENDS_FROM {
        return;
    }
    cap(mesh, p[0], -dirs[0], core, outer, col);
    cap(mesh, p[p.len() - 1], dirs[dirs.len() - 1], core, outer, col);
}

/// The width (pixels) from which the end of a strip is drawn round.
const ROUND_ENDS_FROM: f32 = 2.5;

/// The closed line through `pts` (on the screen) as a pixel wide quad for each piece, two triangles a piece: no fade at the edge and no
/// join at the corners (a corner of a line one pixel wide shows no gap). The outlines of the pours, of which there are very many.
pub(super) fn add_hairline_ring(mesh: &mut Mesh, pts: &[Pos2], col: Color32) {
    let n = pts.len();
    if n < 2 {
        return;
    }
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let d = b - a;
        let len = d.length();
        if len < 1e-3 {
            continue;
        }
        let h = Vec2::new(-d.y, d.x) / len * 0.5;
        let base = mesh.vertices.len() as u32;
        for q in [a + h, a - h, b - h, b + h] {
            mesh.colored_vertex(q, col);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
}

/// A half disc round `c`, bulging towards `dir`, from the left edge of a strip to its right edge.
fn cap(mesh: &mut Mesh, c: Pos2, dir: Vec2, core: f32, outer: f32, col: Color32) {
    // an even number of steps, so that the middle of the cap is a corner (the cap then reaches out by the full radius)
    let n = ((outer.sqrt() * 0.8).ceil() * 2.0).clamp(2.0, 24.0) as usize;
    let clear = Color32::TRANSPARENT;
    let theta = dir.y.atan2(dir.x);
    let centre = mesh.vertices.len() as u32;
    mesh.colored_vertex(c, col);
    let mut prev: Option<(u32, u32)> = None;
    for j in 0..=n {
        let a = theta + std::f32::consts::FRAC_PI_2 - std::f32::consts::PI * j as f32 / n as f32;
        let u = Vec2::new(a.cos(), a.sin());
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(c + u * core, col);
        mesh.colored_vertex(c + u * outer, clear);
        if let Some((pi, _)) = prev {
            mesh.add_triangle(centre, pi, i);
            mesh.add_triangle(pi, pi + 1, i);
            mesh.add_triangle(pi + 1, i + 1, i);
        }
        prev = Some((i, i + 1));
    }
}

/// Is the box of the points, grown by `margin` pixels once on the screen, in the view?
pub(super) fn on_screen(view: Rect, a: Pos2, b: Pos2, margin: f32) -> bool {
    Rect::from_two_pos(a, b).expand(margin).intersects(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(side: Side, w: f64, a: (f64, f64), b: (f64, f64)) -> Track {
        Track { side, a: Point::new(a.0, a.1), b: Point::new(b.0, b.1), width: w }
    }

    #[test]
    fn tracks_that_join_end_to_end_are_one_chain() {
        // listed out of order, one of them backwards
        let t = vec![tr(Side::Top, 0.2, (1.0, 0.0), (2.0, 1.0)), tr(Side::Top, 0.2, (0.0, 0.0), (1.0, 0.0)), tr(Side::Top, 0.2, (3.0, 1.0), (2.0, 1.0))];
        let c = chains(&t);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].pts.len(), 4);
        let ends = [c[0].pts[0], c[0].pts[3]];
        assert!(ends.contains(&Point::new(0.0, 0.0)) && ends.contains(&Point::new(3.0, 1.0)));
    }

    #[test]
    fn a_branch_or_a_change_of_width_or_layer_ends_a_chain() {
        let t = vec![tr(Side::Top, 0.2, (0.0, 0.0), (1.0, 0.0)), tr(Side::Top, 0.2, (1.0, 0.0), (2.0, 0.0)), tr(Side::Top, 0.2, (1.0, 0.0), (1.0, 1.0))];
        assert_eq!(chains(&t).len(), 3, "three tracks meet in one joint");
        let t = vec![tr(Side::Top, 0.2, (0.0, 0.0), (1.0, 0.0)), tr(Side::Top, 0.3, (1.0, 0.0), (2.0, 1.0))];
        assert_eq!(chains(&t).len(), 2, "another width");
        let t = vec![tr(Side::Top, 0.2, (0.0, 0.0), (1.0, 0.0)), tr(Side::Bottom, 0.2, (1.0, 0.0), (2.0, 1.0))];
        assert_eq!(chains(&t).len(), 2, "another layer");
    }

    #[test]
    fn a_straight_run_keeps_only_its_ends_and_a_ring_is_followed_all_round() {
        let t: Vec<Track> = (0..5).map(|i| tr(Side::Top, 0.2, (i as f64, 0.0), (i as f64 + 1.0, 0.0))).collect();
        let c = chains(&t);
        assert_eq!((c.len(), c[0].pts.len()), (1, 2));
        let ring = vec![
            tr(Side::Top, 0.2, (0.0, 0.0), (1.0, 0.0)),
            tr(Side::Top, 0.2, (1.0, 0.0), (1.0, 1.0)),
            tr(Side::Top, 0.2, (1.0, 1.0), (0.0, 1.0)),
            tr(Side::Top, 0.2, (0.0, 1.0), (0.0, 0.0)),
        ];
        let c = chains(&ring);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].pts.len(), 5, "round and back to the start");
    }

    #[test]
    fn a_track_that_doubles_back_is_cut_where_it_turns() {
        let t = vec![tr(Side::Top, 0.2, (0.0, 0.0), (2.0, 0.0)), tr(Side::Top, 0.2, (2.0, 0.0), (0.0, 0.1))];
        assert_eq!(chains(&t).len(), 2);
    }

    #[test]
    fn a_strip_is_as_wide_as_asked_and_has_round_caps() {
        let mut m = Mesh::default();
        add_chain(&mut m, &[Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0)], 4.0, Color32::RED);
        let (mut lo, mut hi) = (Pos2::new(f32::MAX, f32::MAX), Pos2::new(f32::MIN, f32::MIN));
        for v in &m.vertices {
            lo = Pos2::new(lo.x.min(v.pos.x), lo.y.min(v.pos.y));
            hi = Pos2::new(hi.x.max(v.pos.x), hi.y.max(v.pos.y));
        }
        // half the width, and the pixel it fades over
        assert!((lo.y + 2.5).abs() < 1e-3 && (hi.y - 2.5).abs() < 1e-3, "{lo:?} {hi:?}");
        assert!((lo.x + 2.5).abs() < 1e-3 && (hi.x - 12.5).abs() < 1e-3, "the caps reach out by the half width: {lo:?} {hi:?}");
        assert!(m.indices.iter().all(|&i| (i as usize) < m.vertices.len()));
    }

    #[test]
    fn a_curve_of_many_pieces_costs_little() {
        // a quarter circle of 16 pieces, 0.2 mm wide, at 20 px a mm
        let pts: Vec<Pos2> = (0..=16).map(|k| {
            let a = std::f32::consts::FRAC_PI_2 * k as f32 / 16.0;
            Pos2::new(a.cos() * 200.0, a.sin() * 200.0)
        }).collect();
        let mut m = Mesh::default();
        add_chain(&mut m, &pts, 4.0, Color32::RED);
        // 6 triangles a piece for the strip and its fade, and the two caps
        assert!(m.indices.len() / 3 < 16 * 6 + 2 * 30, "{}", m.indices.len() / 3);
    }

    #[test]
    fn simplifying_keeps_the_ends_and_what_sticks_out_and_drops_the_rest() {
        let p = |x: f64, y: f64| Point::new(x, y);
        // a small bump of 0.001 goes, a bump of 0.5 stays
        assert_eq!(simplify_open(&[p(0.0, 0.0), p(1.0, 0.001), p(2.0, 0.0)], 0.01), vec![p(0.0, 0.0), p(2.0, 0.0)]);
        assert_eq!(simplify_open(&[p(0.0, 0.0), p(1.0, 0.5), p(2.0, 0.0)], 0.01).len(), 3);
        // a circle of 64 corners and radius 1 at 0.01: far fewer corners, all on the circle's side of it
        let circle: Vec<Point> = (0..64).map(|k| p((k as f64 * std::f64::consts::TAU / 64.0).cos(), (k as f64 * std::f64::consts::TAU / 64.0).sin())).collect();
        let s = simplify_closed(&circle, 0.01);
        assert!(s.len() < 40 && s.len() >= 8, "{}", s.len());
        for q in &circle {
            let d = (0..s.len()).map(|i| off_segment(*q, s[i], s[(i + 1) % s.len()])).fold(f64::MAX, f64::min);
            assert!(d <= 0.01 + 1e-9, "a corner is {d} from the simplified outline");
        }
        // a triangle is not made smaller than a triangle
        let tri = [p(0.0, 0.0), p(1.0, 0.0), p(0.0, 1.0)];
        assert_eq!(simplify_closed(&tri, 5.0), tri.to_vec());
    }

    #[test]
    fn the_silk_is_followed_into_chains_and_the_parts_are_kept_apart() {
        let mut b = pcb_core::Board::new("t", 20.0, 20.0);
        let line = |a: (f64, f64), c: (f64, f64), refdes: &str| pcb_core::SilkLine { side: Side::Top, a: Point::new(a.0, a.1), b: Point::new(c.0, c.1), width: 0.15, refdes: refdes.into() };
        // an outline of a part in three pieces, and a free line
        b.silk = vec![line((0.0, 0.0), (1.0, 0.0), "R1"), line((1.0, 0.0), (1.0, 1.0), "R1"), line((1.0, 1.0), (0.0, 1.0), "R1"), line((5.0, 5.0), (6.0, 5.0), "")];
        let c = SilkCache::new(&b);
        assert_eq!((c.plain.len(), c.of_parts.len()), (1, 1));
        assert_eq!(c.of_parts[0].pts.len(), 4, "three lines are one chain of four points");
        assert!(c.is_for(&b));
        // a text is a few chains, not a chain for each stroke
        b.texts.push(pcb_core::SilkText { side: Side::Top, pos: Point::new(2.0, 2.0), height: 1.0, text: "R12".into(), refdes: "R12".into(), rot: 0 });
        assert!(!c.is_for(&b), "a new text is a change");
        let c = SilkCache::new(&b);
        let strokes: usize = b.texts[0].strokes().len();
        assert!(c.of_parts.len() < strokes, "{} chains for {strokes} strokes", c.of_parts.len());
    }

    #[test]
    fn a_thin_line_has_flat_ends_and_a_wide_one_round_ends() {
        let n = |w: f32| {
            let mut m = Mesh::default();
            add_chain(&mut m, &[Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0)], w, Color32::RED);
            m.indices.len() / 3
        };
        assert_eq!(n(1.5), 6, "a strip and its fade, nothing else");
        assert!(n(6.0) > 6 + 2 * 6, "caps are added");
    }

    #[test]
    fn a_hairline_ring_is_two_triangles_a_piece() {
        let mut m = Mesh::default();
        add_hairline_ring(&mut m, &[Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0), Pos2::new(10.0, 10.0), Pos2::new(0.0, 10.0)], Color32::RED);
        assert_eq!((m.vertices.len(), m.indices.len() / 3), (16, 8));
        let w = m.vertices.iter().map(|v| v.pos.y).fold(f32::MIN, f32::max) - m.vertices.iter().map(|v| v.pos.y).fold(f32::MAX, f32::min);
        assert!((w - 11.0).abs() < 1e-4, "a pixel wide line round a 10 px square reaches half a pixel out on both sides");
    }

    #[test]
    fn a_dot_and_a_hair_line_are_drawn() {
        let mut m = Mesh::default();
        add_chain(&mut m, &[Pos2::new(5.0, 5.0)], 3.0, Color32::RED);
        assert!(!m.indices.is_empty());
        let mut m = Mesh::default();
        add_chain(&mut m, &[Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0)], 0.1, Color32::RED);
        assert!(m.vertices.iter().any(|v| v.color == Color32::RED), "a track narrower than a pixel is still a pixel wide");
    }
}
