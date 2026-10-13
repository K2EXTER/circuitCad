use super::*;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Geo {
    /// Segment `a`-`b` swept by a disc of radius `r` (r > 0: track, round pad, oval pad, via).
    Cap { a: Point, b: Point, r: f64 },
    /// Axis-aligned rectangle.
    Rect { c: Point, hw: f64, hh: f64 },
}

pub(super) fn cross(o: Point, a: Point, b: Point) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

pub(crate) fn point_seg(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0) };
    p.dist(Point::new(a.x + t * dx, a.y + t * dy))
}

pub(super) fn segs_intersect(a: Point, b: Point, c: Point, d: Point) -> bool {
    let (d1, d2) = (cross(a, b, c), cross(a, b, d));
    let (d3, d4) = (cross(c, d, a), cross(c, d, b));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    // touching / collinear cases: an endpoint lying on the other segment
    point_seg(c, a, b) < 1e-12 || point_seg(d, a, b) < 1e-12 || point_seg(a, c, d) < 1e-12 || point_seg(b, c, d) < 1e-12
}

pub fn seg_seg(a: Point, b: Point, c: Point, d: Point) -> f64 {
    if segs_intersect(a, b, c, d) {
        return 0.0;
    }
    point_seg(a, c, d).min(point_seg(b, c, d)).min(point_seg(c, a, b)).min(point_seg(d, a, b))
}

pub(crate) fn point_rect(p: Point, c: Point, hw: f64, hh: f64) -> f64 {
    let dx = ((p.x - c.x).abs() - hw).max(0.0);
    let dy = ((p.y - c.y).abs() - hh).max(0.0);
    dx.hypot(dy)
}

pub(super) fn seg_rect(a: Point, b: Point, c: Point, hw: f64, hh: f64) -> f64 {
    if point_rect(a, c, hw, hh) == 0.0 || point_rect(b, c, hw, hh) == 0.0 {
        return 0.0;
    }
    let k = [
        Point::new(c.x - hw, c.y - hh),
        Point::new(c.x + hw, c.y - hh),
        Point::new(c.x + hw, c.y + hh),
        Point::new(c.x - hw, c.y + hh),
    ];
    for i in 0..4 {
        if segs_intersect(a, b, k[i], k[(i + 1) % 4]) {
            return 0.0;
        }
    }
    k.iter()
        .map(|&q| point_seg(q, a, b))
        .fold(point_rect(a, c, hw, hh).min(point_rect(b, c, hw, hh)), f64::min)
}

/// Edge-to-edge distance (0 when touching or overlapping).
pub(super) fn dist(x: &Geo, y: &Geo) -> f64 {
    match (*x, *y) {
        (Geo::Cap { a, b, r }, Geo::Cap { a: c, b: d, r: s }) => (seg_seg(a, b, c, d) - r - s).max(0.0),
        (Geo::Cap { a, b, r }, Geo::Rect { c, hw, hh }) | (Geo::Rect { c, hw, hh }, Geo::Cap { a, b, r }) => {
            (seg_rect(a, b, c, hw, hh) - r).max(0.0)
        }
        (Geo::Rect { c: c1, hw: w1, hh: h1 }, Geo::Rect { c: c2, hw: w2, hh: h2 }) => {
            let dx = ((c1.x - c2.x).abs() - w1 - w2).max(0.0);
            let dy = ((c1.y - c2.y).abs() - h1 - h2).max(0.0);
            dx.hypot(dy)
        }
    }
}

pub(crate) fn shape_geo(pos: Point, shape: PadShape) -> Geo {
    match shape {
        PadShape::Circle { d } => Geo::Cap { a: pos, b: pos, r: d / 2.0 },
        PadShape::Rect { w, h } => Geo::Rect { c: pos, hw: w / 2.0, hh: h / 2.0 },
        PadShape::Oval { w, h } => {
            let r = w.min(h) / 2.0;
            let off = (w.max(h) / 2.0) - r;
            let (da, db) = if w >= h {
                (Point::new(pos.x - off, pos.y), Point::new(pos.x + off, pos.y))
            } else {
                (Point::new(pos.x, pos.y - off), Point::new(pos.x, pos.y + off))
            };
            Geo::Cap { a: da, b: db, r }
        }
    }
}

pub(super) fn center(g: &Geo) -> Point {
    match *g {
        Geo::Cap { a, b, .. } => Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0),
        Geo::Rect { c, .. } => c,
    }
}

// ---------------------------------------------------------------- copper items
