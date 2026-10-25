//! The copper of a pour as real geometry: the zone polygon, cut to the board outline, minus a clearance
//! around everything of another net (tracks, pads, vias), around mounting holes and along the board edge.
//! The result is a list of shapes (an outer contour and the holes in it), the same for the editor's picture
//! and the Gerber file, so what is drawn is what is made. Curves (round pads, track ends) are many short
//! straight pieces, fine enough that the error is a few micrometres.

use crate::drc::Connectivity;
use crate::geom::signed_area;
use crate::model::*;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use i_triangle::float::triangulatable::Triangulatable;
use std::f64::consts::PI;

/// Largest distance (mm) between a true curve and the straight pieces that stand for it.
pub const CURVE_TOLERANCE: f64 = 0.003;

type Path = Vec<[f64; 2]>;

/// One piece of a pour: its outer contour and the holes inside it.
#[derive(Clone, Debug)]
pub struct FillShape {
    pub outer: Vec<Point>,
    pub holes: Vec<Vec<Point>>,
}

impl FillShape {
    /// The shape as triangles (holes left open), for drawing.
    pub fn triangles(&self) -> Vec<[Point; 3]> {
        let contours: Vec<Path> = std::iter::once(&self.outer).chain(self.holes.iter()).map(|c| c.iter().map(|p| [p.x, p.y]).collect()).collect();
        let t = contours.as_slice().triangulate().to_triangulation::<u32>();
        t.indices
            .as_chunks::<3>().0.iter()
            .map(|i| {
                let p = |k: u32| Point::new(t.points[k as usize][0], t.points[k as usize][1]);
                [p(i[0]), p(i[1]), p(i[2])]
            })
            .collect()
    }
}

/// How many straight pieces a full circle of radius `r` needs to stay within `CURVE_TOLERANCE`.
pub fn circle_steps(r: f64) -> usize {
    if r <= CURVE_TOLERANCE {
        return 8;
    }
    let step = 2.0 * (1.0 - CURVE_TOLERANCE / r).clamp(-1.0, 1.0).acos();
    ((2.0 * PI / step).ceil() as usize).clamp(16, 720)
}

pub(crate) fn circle(c: Point, r: f64) -> Path {
    let n = circle_steps(r);
    (0..n)
        .map(|k| {
            let a = 2.0 * PI * k as f64 / n as f64;
            [c.x + r * a.cos(), c.y + r * a.sin()]
        })
        .collect()
}

/// A line of round-ended thickness 2r (counter-clockwise).
pub(crate) fn capsule(a: Point, b: Point, r: f64) -> Path {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    if dx.hypot(dy) < 1e-9 {
        return circle(a, r);
    }
    let th = dy.atan2(dx);
    let half = (circle_steps(r) / 2).max(4);
    let mut out = Vec::with_capacity(2 * half + 2);
    for k in 0..=half {
        let t = th - PI / 2.0 + PI * k as f64 / half as f64;
        out.push([b.x + r * t.cos(), b.y + r * t.sin()]);
    }
    for k in 0..=half {
        let t = th + PI / 2.0 + PI * k as f64 / half as f64;
        out.push([a.x + r * t.cos(), a.y + r * t.sin()]);
    }
    out
}

/// A pad grown by `c` on every side.
pub(crate) fn pad_path(pos: Point, shape: PadShape, c: f64) -> Path {
    match shape {
        PadShape::Circle { d } => circle(pos, d / 2.0 + c),
        PadShape::Rect { w, h } => {
            let (hw, hh) = (w / 2.0 + c, h / 2.0 + c);
            vec![[pos.x - hw, pos.y - hh], [pos.x + hw, pos.y - hh], [pos.x + hw, pos.y + hh], [pos.x - hw, pos.y + hh]]
        }
        PadShape::Oval { w, h } => {
            let r = w.min(h) / 2.0;
            let off = w.max(h) / 2.0 - r;
            let (a, b) = if w >= h { (Point::new(pos.x - off, pos.y), Point::new(pos.x + off, pos.y)) } else { (Point::new(pos.x, pos.y - off), Point::new(pos.x, pos.y + off)) };
            capsule(a, b, r + c)
        }
    }
}

/// A polygon as a counter-clockwise path (the overlay engine wants one winding for everything it unions).
fn ccw(poly: &[Point]) -> Path {
    let mut p: Path = poly.iter().map(|p| [p.x, p.y]).collect();
    if signed_area(poly) < 0.0 {
        p.reverse();
    }
    p
}

fn to_points(path: &[[f64; 2]]) -> Vec<Point> {
    path.iter().map(|p| Point::new(p[0], p[1])).collect()
}

/// The copper of `zone` (without thermal spokes, which the Gerber writer adds).
pub fn zone_fill(board: &Board, zone: &Zone, conn: &Connectivity) -> Vec<FillShape> {
    let raws: Vec<Vec<FillShape>> = board.zones.iter().map(|z| if z.net == zone.net && z.side == zone.side { raw_fill(board, z, conn) } else { vec![] }).collect();
    fill_from(board, zone, conn, &raws)
}

/// [`zone_fill`] of `zone`, with the raw fills of all pours of its net and layer worked out already (`raws`, by zone index).
fn fill_from(board: &Board, zone: &Zone, conn: &Connectivity, raws: &[Vec<FillShape>]) -> Vec<FillShape> {
    let me = board.zones.iter().position(|z| std::ptr::eq(z, zone));
    let mut out = me.map_or_else(|| raw_fill(board, zone, conn), |i| raws[i].clone());
    if zone.remove_islands && !zone.net.is_empty() {
        // Pours of the same net on the same layer that touch or overlap are one piece of copper: a pour beside another one is not an
        // island because the pad is in the other one.
        let mut all: Vec<FillShape> = out.clone();
        for (k, _) in board.zones.iter().enumerate().filter(|(_, o)| !std::ptr::eq(*o, zone) && o.side == zone.side && o.net == zone.net) {
            all.extend(raws[k].iter().cloned());
        }
        let joined = union(&all);
        out.retain(|shape| match inner_point(shape) {
            Some(p) => joined.iter().find(|j| inside_shape(j, p)).is_none_or(|j| touches_net(board, zone, conn, j)),
            None => true,
        });
    }
    out
}

/// A point well inside a piece (the middle of its first triangle).
fn inner_point(shape: &FillShape) -> Option<Point> {
    let t = shape.triangles().into_iter().max_by(|a, b| signed_area(a).abs().total_cmp(&signed_area(b).abs()))?;
    Some(Point::new((t[0].x + t[1].x + t[2].x) / 3.0, (t[0].y + t[1].y + t[2].y) / 3.0))
}

fn inside_shape(shape: &FillShape, p: Point) -> bool {
    use crate::geom::inside_polygon;
    inside_polygon(&shape.outer, p) && !shape.holes.iter().any(|h| inside_polygon(h, p))
}

/// Join the pour `board.zones[idx]` with the pours of its net and layer that overlap or touch it, into one pour (the new
/// pour's settings are kept). A union with a hole (a ring) can't be one polygon, so those pours stay apart; the copper is the
/// same either way. Returns how many pours were absorbed and the new index of the joined pour.
pub fn merge_zone_outline(board: &mut Board, idx: usize) -> (usize, usize) {
    let Some(z) = board.zones.get(idx) else { return (0, idx) };
    let (net, side) = (z.net.clone(), z.side);
    if net.is_empty() {
        return (0, idx);
    }
    let as_shape = |poly: &[Point]| FillShape { outer: poly.to_vec(), holes: vec![] };
    let mut poly = z.polygon.clone();
    let mut absorbed: Vec<usize> = vec![];
    loop {
        let mut grew = false;
        for (k, o) in board.zones.iter().enumerate() {
            if k == idx || absorbed.contains(&k) || o.net != net || o.side != side {
                continue;
            }
            let joined = union(&[as_shape(&poly), as_shape(&o.polygon)]);
            if let [one] = joined.as_slice()
                && one.holes.is_empty() {
                    poly = one.outer.clone();
                    absorbed.push(k);
                    grew = true;
                }
        }
        if !grew {
            break;
        }
    }
    if absorbed.is_empty() {
        return (0, idx);
    }
    board.zones[idx].polygon = poly;
    absorbed.sort_unstable();
    for &k in absorbed.iter().rev() {
        board.zones.remove(k);
    }
    (absorbed.len(), idx - absorbed.iter().filter(|&&k| k < idx).count())
}

/// The pieces merged where they touch or overlap.
fn union(shapes: &[FillShape]) -> Vec<FillShape> {
    // outer contours counter-clockwise, holes clockwise, so the holes are subtracted
    let mut contours: Vec<Path> = vec![];
    for s in shapes {
        contours.push(ccw(&s.outer));
        for h in &s.holes {
            let mut p = ccw(h);
            p.reverse();
            contours.push(p);
        }
    }
    let none: Vec<Path> = vec![];
    contours
        .overlay(&none, OverlayRule::Union, FillRule::NonZero)
        .into_iter()
        .filter_map(|shape| {
            let mut it = shape.into_iter();
            let outer = to_points(&it.next()?);
            (signed_area(&outer).abs() >= 1e-6).then(|| FillShape { outer, holes: it.map(|h| to_points(&h)).collect() })
        })
        .collect()
}

/// The pours as they are drawn: pours of the same net on one layer that overlap or touch are one piece of copper, so they are
/// merged (drawn separately, the overlap would be painted twice). One entry per layer and net (pours without a net stay apart),
/// with the pieces and the net name (empty for a pour without a net); the entries come in the order of the first pour of each.
type MergedFills = Vec<(Side, String, Vec<FillShape>)>;
static MERGED_CACHE: std::sync::Mutex<Vec<((u64, u64), std::sync::Arc<MergedFills>)>> = std::sync::Mutex::new(Vec::new());

pub fn merged_fills(board: &Board, conn: &Connectivity) -> Vec<(Side, String, Vec<FillShape>)> {
    // kept for the last few states of the board, like the fills of the single pours
    let key = (geometry_key(board), nets_key(board, conn));
    if let Ok(cache) = MERGED_CACHE.lock()
        && let Some((_, v)) = cache.iter().find(|(k, _)| *k == key) {
            return (**v).clone();
        }
    let out = merged_fills_uncached(board, conn);
    if let Ok(mut cache) = MERGED_CACHE.lock() {
        if cache.len() >= 4 {
            cache.remove(0);
        }
        cache.push((key, std::sync::Arc::new(out.clone())));
    }
    out
}

fn merged_fills_uncached(board: &Board, conn: &Connectivity) -> Vec<(Side, String, Vec<FillShape>)> {
    let mut out: Vec<(Side, Option<String>, Vec<FillShape>)> = vec![];
    // every pour's raw copper once (the island test of each pour looks at the others of its net)
    let raws: Vec<Vec<FillShape>> = board.zones.iter().map(|z| raw_fill(board, z, conn)).collect();
    for z in &board.zones {
        let fill = fill_from(board, z, conn, &raws);
        let key = (!z.net.is_empty()).then(|| z.net.clone());
        match key.as_ref().and_then(|_| out.iter_mut().find(|(s, k, _)| *s == z.side && *k == key)) {
            Some((_, _, shapes)) => shapes.extend(fill),
            None => out.push((z.side, key, fill)),
        }
    }
    out.into_iter().map(|(s, k, shapes)| (s, k.clone().unwrap_or_default(), if k.is_some() { union(&shapes) } else { shapes })).collect()
}

type FillKey = (u64, u64, usize);

/// The fills worked out last, by (the copper's key, the nets of the copper, the pour's place in the list). The editor asks for the
/// same pours from several places for one state of the board (the picture, the pours in pieces, the thermal vias, the DRC): each
/// is a boolean operation over every track, pad and via, tens of milliseconds a pour, so a state is worked out once.
static FILL_CACHE: std::sync::Mutex<Vec<(FillKey, std::sync::Arc<Vec<FillShape>>)>> = std::sync::Mutex::new(Vec::new());
const FILL_CACHE_LEN: usize = 48;

fn nets_key(board: &Board, conn: &Connectivity) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (0..board.tracks.len()).for_each(|i| conn.track_net(i).hash(&mut h));
    (0..board.pads.len()).for_each(|j| conn.pad_net(j).hash(&mut h));
    (0..board.vias.len()).for_each(|k| conn.via_net(k).hash(&mut h));
    h.finish()
}

fn raw_fill(board: &Board, zone: &Zone, conn: &Connectivity) -> Vec<FillShape> {
    let Some(me) = board.zones.iter().position(|z| std::ptr::eq(z, zone)) else { return raw_fill_uncached(board, zone, conn) };
    let key: FillKey = (geometry_key(board), nets_key(board, conn), me);
    if let Ok(cache) = FILL_CACHE.lock()
        && let Some((_, v)) = cache.iter().find(|(k, _)| *k == key) {
            return (**v).clone();
        }
    let fill = raw_fill_uncached(board, zone, conn);
    if let Ok(mut cache) = FILL_CACHE.lock() {
        if cache.len() >= FILL_CACHE_LEN {
            cache.remove(0);
        }
        cache.push((key, std::sync::Arc::new(fill.clone())));
    }
    fill
}

fn raw_fill_uncached(board: &Board, zone: &Zone, conn: &Connectivity) -> Vec<FillShape> {
    if zone.polygon.len() < 3 {
        return vec![];
    }
    let c = zone.clearance;
    let side = zone.side;
    let same = |net: Option<&str>| !zone.net.is_empty() && net == Some(zone.net.as_str());
    let mut cuts: Vec<Path> = vec![];
    for (i, t) in board.tracks.iter().enumerate().filter(|(_, t)| t.side == side) {
        if !same(conn.track_net(i)) {
            cuts.push(capsule(t.a, t.b, t.width / 2.0 + c));
        }
    }
    for (j, p) in board.pads.iter().enumerate().filter(|(_, p)| p.drill.is_some() || p.side == side) {
        if !same(conn.pad_net(j)) {
            cuts.push(pad_path(p.pos, p.shape, c));
        }
    }
    for (k, v) in board.vias.iter().enumerate() {
        if !same(conn.via_net(k)) {
            cuts.push(circle(v.pos, v.diameter / 2.0 + c));
        }
    }
    for h in &board.holes {
        // a slot is cleared with circles along its centre line
        let (a, b) = h.ends();
        let steps = ((a.dist(b) / (h.drill / 2.0).max(0.05)).ceil() as usize).max(1);
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            cuts.push(circle(Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t), h.drill / 2.0 + c));
        }
    }
    // a later pour of another net on the same layer wins where they overlap; this one keeps its clearance from it
    if let Some(me) = board.zones.iter().position(|z| std::ptr::eq(z, zone)) {
        for other in board.zones[me + 1..].iter().filter(|o| o.side == side && o.polygon.len() >= 3 && !same(Some(o.net.as_str()))) {
            cuts.push(ccw(&other.polygon));
            let m = other.polygon.len();
            for i in 0..m {
                cuts.push(capsule(other.polygon[i], other.polygon[(i + 1) % m], c));
            }
        }
    }
    let n = board.outline.len();
    let edge = c.max(board.rules.edge_clearance);
    if n >= 3 {
        for i in 0..n {
            cuts.push(capsule(board.outline[i], board.outline[(i + 1) % n], edge));
        }
    }
    // only what reaches the pour can cut it (a small pour among many tracks and pads is cut by a handful of them)
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in &zone.polygon {
        lo = [lo[0].min(p.x), lo[1].min(p.y)];
        hi = [hi[0].max(p.x), hi[1].max(p.y)];
    }
    cuts.retain(|path| {
        let (mut plo, mut phi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for q in path {
            plo = [plo[0].min(q[0]), plo[1].min(q[1])];
            phi = [phi[0].max(q[0]), phi[1].max(q[1])];
        }
        plo[0] <= hi[0] && phi[0] >= lo[0] && plo[1] <= hi[1] && phi[1] >= lo[1]
    });
    let subject = ccw(&zone.polygon);
    let inside = if n >= 3 { subject.overlay(&ccw(&board.outline), OverlayRule::Intersect, FillRule::NonZero) } else { vec![vec![subject]] };
    let mut out = vec![];
    for shape in inside {
        let cut = if cuts.is_empty() { vec![shape] } else { shape.overlay(&cuts, OverlayRule::Difference, FillRule::NonZero) };
        for s in cut {
            let mut it = s.into_iter();
            let Some(outer) = it.next() else { continue };
            let outer = to_points(&outer);
            if signed_area(&outer).abs() < 1e-6 {
                continue;
            }
            out.push(FillShape { outer, holes: it.map(|h| to_points(&h)).collect() });
        }
    }
    out
}

/// True when a pad, via or track end of the zone's net lies inside the pour piece `shape`.
fn touches_net(board: &Board, zone: &Zone, conn: &Connectivity, shape: &FillShape) -> bool {
    use crate::geom::inside_polygon;
    let net = Some(zone.net.as_str());
    let inside = |p: Point| inside_polygon(&shape.outer, p) && !shape.holes.iter().any(|h| inside_polygon(h, p));
    board.pads.iter().enumerate().any(|(j, p)| conn.pad_net(j) == net && (p.drill.is_some() || p.side == zone.side) && inside(p.pos))
        || board.vias.iter().enumerate().any(|(k, v)| conn.via_net(k) == net && inside(v.pos))
        || board.tracks.iter().enumerate().any(|(i, t)| t.side == zone.side && conn.track_net(i) == net && (inside(t.a) || inside(t.b)))
}

/// A number that changes whenever something a pour depends on changes (zones, copper, holes, outline,
/// nets), so a drawn pour can be kept until then.
pub fn geometry_key(board: &Board) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let f = |h: &mut std::collections::hash_map::DefaultHasher, v: f64| v.to_bits().hash(h);
    let pt = |h: &mut std::collections::hash_map::DefaultHasher, p: Point| {
        p.x.to_bits().hash(h);
        p.y.to_bits().hash(h);
    };
    for z in &board.zones {
        z.net.hash(&mut h);
        z.side.hash(&mut h);
        z.thermal.hash(&mut h);
        z.remove_islands.hash(&mut h);
        f(&mut h, z.clearance);
        for p in &z.polygon {
            pt(&mut h, *p);
        }
    }
    for t in &board.tracks {
        t.side.hash(&mut h);
        pt(&mut h, t.a);
        pt(&mut h, t.b);
        f(&mut h, t.width);
    }
    for p in &board.pads {
        pt(&mut h, p.pos);
        let (w, hh) = p.shape.size();
        f(&mut h, w);
        f(&mut h, hh);
        p.drill.map(f64::to_bits).hash(&mut h);
        p.side.hash(&mut h);
        p.net.hash(&mut h);
    }
    for v in &board.vias {
        pt(&mut h, v.pos);
        f(&mut h, v.diameter);
    }
    for hole in &board.holes {
        pt(&mut h, hole.pos);
        f(&mut h, hole.drill);
        f(&mut h, hole.slot);
        f(&mut h, hole.slot_v as u8 as f64);
    }
    for p in &board.outline {
        pt(&mut h, *p);
    }
    f(&mut h, board.rules.edge_clearance);
    h.finish()
}

#[cfg(test)]
mod merge_outline_tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
        vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)]
    }

    #[test]
    fn overlapping_pours_of_one_net_become_one_outline() {
        let mut b = Board::new("t", 50.0, 50.0);
        b.zones.push(Zone::new("GND", Side::Top, rect(0.0, 0.0, 10.0, 10.0)));
        b.zones.push(Zone::new("5V", Side::Top, rect(5.0, 5.0, 15.0, 15.0)));
        b.zones.push(Zone::new("GND", Side::Top, rect(20.0, 0.0, 30.0, 10.0)));
        b.zones.push(Zone::new("GND", Side::Top, rect(8.0, 2.0, 12.0, 6.0)));
        let (n, at) = merge_zone_outline(&mut b, 3);
        assert_eq!(n, 1);
        assert_eq!(b.zones.len(), 3);
        assert_eq!(b.zones[at].net, "GND");
        assert!(signed_area(&b.zones[at].polygon).abs() > 100.0);
    }

    #[test]
    fn a_ring_is_left_apart() {
        let mut b = Board::new("t", 50.0, 50.0);
        b.zones.push(Zone::new("GND", Side::Top, vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 3.0), Point::new(0.0, 3.0)]));
        b.zones.push(Zone::new("GND", Side::Top, vec![Point::new(0.0, 3.0), Point::new(3.0, 3.0), Point::new(3.0, 10.0), Point::new(0.0, 10.0)]));
        b.zones.push(Zone::new("GND", Side::Top, vec![Point::new(0.0, 10.0), Point::new(10.0, 10.0), Point::new(10.0, 7.0), Point::new(0.0, 7.0)]));
        b.zones.push(Zone::new("GND", Side::Top, vec![Point::new(7.0, 3.0), Point::new(10.0, 3.0), Point::new(10.0, 7.0), Point::new(7.0, 7.0)]));
        let (n, _) = merge_zone_outline(&mut b, 3);
        assert!(n < 3, "a ring can't be one polygon");
    }
}
