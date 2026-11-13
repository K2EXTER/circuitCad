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

pub(crate) fn segs_intersect(a: Point, b: Point, c: Point, d: Point) -> bool {
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

pub(crate) fn seg_rect(a: Point, b: Point, c: Point, hw: f64, hh: f64) -> f64 {
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

fn nearest_on_seg(p: Point, a: Point, b: Point) -> Point {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0) };
    Point::new(a.x + t * dx, a.y + t * dy)
}

fn nearest_in_rect(p: Point, c: Point, hw: f64, hh: f64) -> Point {
    Point::new(p.x.clamp(c.x - hw, c.x + hw), p.y.clamp(c.y - hh, c.y + hh))
}

/// The closest pair of points of two cores (a segment, or a rectangle), the first on `x`.
fn nearest_cores(x: &Geo, y: &Geo) -> (Point, Point) {
    // the pair of least distance among candidates: each end or corner of one against the other
    fn best(c: impl Iterator<Item = (Point, Point)>) -> (Point, Point) {
        c.min_by(|(a, b), (c, d)| a.dist(*b).total_cmp(&c.dist(*d))).unwrap_or_default()
    }
    let corners = |c: Point, hw: f64, hh: f64| [Point::new(c.x - hw, c.y - hh), Point::new(c.x + hw, c.y - hh), Point::new(c.x + hw, c.y + hh), Point::new(c.x - hw, c.y + hh)];
    match (*x, *y) {
        (Geo::Cap { a, b, .. }, Geo::Cap { a: c, b: d, .. }) => {
            if segs_intersect(a, b, c, d) {
                // crossing: where they cross is near enough (the ends' nearest points on the other)
                let p = best([a, b].into_iter().map(|e| (e, nearest_on_seg(e, c, d))));
                return (p.1, p.1);
            }
            best([a, b].into_iter().map(|e| (e, nearest_on_seg(e, c, d))).chain([c, d].into_iter().map(|e| (nearest_on_seg(e, a, b), e))))
        }
        (Geo::Cap { a, b, .. }, Geo::Rect { c, hw, hh }) => {
            best([a, b].into_iter().map(|e| (e, nearest_in_rect(e, c, hw, hh))).chain(corners(c, hw, hh).into_iter().map(|k| (nearest_on_seg(k, a, b), k))))
        }
        (Geo::Rect { .. }, Geo::Cap { .. }) => {
            let (p, q) = nearest_cores(y, x);
            (q, p)
        }
        (Geo::Rect { c: c1, hw: w1, hh: h1 }, Geo::Rect { c: c2, hw: w2, hh: h2 }) => {
            // along each axis: the middle of the overlap, or the two facing sides
            let axis = |c1: f64, w1: f64, c2: f64, w2: f64| {
                let (lo, hi) = ((c1 - w1).max(c2 - w2), (c1 + w1).min(c2 + w2));
                if lo <= hi {
                    ((lo + hi) / 2.0, (lo + hi) / 2.0)
                } else if c1 < c2 {
                    (c1 + w1, c2 - w2)
                } else {
                    (c1 - w1, c2 + w2)
                }
            };
            let (x1, x2) = axis(c1.x, w1, c2.x, w2);
            let (y1, y2) = axis(c1.y, h1, c2.y, h2);
            (Point::new(x1, y1), Point::new(x2, y2))
        }
    }
}

/// Where two copper shapes come closest: the middle of the gap between them (where a finding is marked, so the mark sits on the
/// narrow spot and not between the shapes' centres, which for a long track can be far from it).
pub(super) fn gap_point(x: &Geo, y: &Geo) -> Point {
    let (p, q) = nearest_cores(x, y);
    let r = |g: &Geo| if let Geo::Cap { r, .. } = *g { r } else { 0.0 };
    let d = p.dist(q);
    if d < 1e-12 {
        return p;
    }
    // from the cores out to the copper's edges (the discs round the segments)
    let (ux, uy) = ((q.x - p.x) / d, (q.y - p.y) / d);
    let (r1, r2) = (r(x).min(d), r(y).min(d));
    let e1 = Point::new(p.x + ux * r1, p.y + uy * r1);
    let e2 = Point::new(q.x - ux * r2, q.y - uy * r2);
    Point::new((e1.x + e2.x) / 2.0, (e1.y + e2.y) / 2.0)
}

pub(super) fn center(g: &Geo) -> Point {
    match *g {
        Geo::Cap { a, b, .. } => Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0),
        Geo::Rect { c, .. } => c,
    }
}

// ---------------------------------------------------------------- copper items

#[cfg(test)]
mod gap_tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn a_finding_is_marked_in_the_gap_not_between_the_centres() {
        // a long track along y = 0 from x = 0 to 40, and a pad near its far end: the gap is at x ≈ 40, not at the centres' middle
        let track = Geo::Cap { a: p(0.0, 0.0), b: p(40.0, 0.0), r: 0.125 };
        let pad = Geo::Rect { c: p(40.0, 0.9), hw: 0.5, hh: 0.6 };
        let at = gap_point(&track, &pad);
        assert!((at.x - 40.0).abs() < 1e-9 && (at.y - 0.2125).abs() < 1e-9, "{at:?}");
        // the other way round gives the same point
        let back = gap_point(&pad, &track);
        assert!(back.dist(at) < 1e-9, "{back:?}");
        // two rectangles side by side: the middle of the facing sides, at the height they share
        let a = Geo::Rect { c: p(0.0, 0.0), hw: 1.0, hh: 1.0 };
        let b = Geo::Rect { c: p(2.2, 0.5), hw: 1.0, hh: 1.0 };
        let at = gap_point(&a, &b);
        assert!((at.x - 1.1).abs() < 1e-9 && (at.y - 0.25).abs() < 1e-9, "{at:?}");
        // two round pads: on the line between them, in the middle of the gap
        let c = Geo::Cap { a: p(0.0, 0.0), b: p(0.0, 0.0), r: 1.0 };
        let d = Geo::Cap { a: p(3.0, 0.0), b: p(3.0, 0.0), r: 0.5 };
        let at = gap_point(&c, &d);
        assert!((at.x - 1.75).abs() < 1e-9 && at.y.abs() < 1e-9, "{at:?}");
    }
}
