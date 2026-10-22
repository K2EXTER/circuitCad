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
    let mut out = raw_fill(board, zone, conn);
    if zone.remove_islands && !zone.net.is_empty() {
        // Pours of the same net on the same layer that touch or overlap are one piece of copper: a pour beside another one is not an
        // island because the pad is in the other one.
        let mut all: Vec<FillShape> = out.clone();
        for other in board.zones.iter().filter(|o| !std::ptr::eq(*o, zone) && o.side == zone.side && o.net == zone.net) {
            all.extend(raw_fill(board, other, conn));
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
pub fn merged_fills(board: &Board, conn: &Connectivity) -> Vec<(Side, String, Vec<FillShape>)> {
    let mut out: Vec<(Side, Option<String>, Vec<FillShape>)> = vec![];
    for z in &board.zones {
        let fill = zone_fill(board, z, conn);
        let key = (!z.net.is_empty()).then(|| z.net.clone());
        match key.as_ref().and_then(|_| out.iter_mut().find(|(s, k, _)| *s == z.side && *k == key)) {
            Some((_, _, shapes)) => shapes.extend(fill),
            None => out.push((z.side, key, fill)),
        }
    }
    out.into_iter().map(|(s, k, shapes)| (s, k.clone().unwrap_or_default(), if k.is_some() { union(&shapes) } else { shapes })).collect()
}

fn raw_fill(board: &Board, zone: &Zone, conn: &Connectivity) -> Vec<FillShape> {
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
