//! Board outlines with curves. The board outline is a polygon; curves are stored as short
//! straight pieces (a fab reads the outline as a closed path of segments anyway), so rounded
//! corners and circles need no special support in the Gerber writer, the DRC or the editor.

use crate::model::Point;
use std::f64::consts::PI;

/// Longest turn of one straight piece of a curve (degrees): small enough to look round, large
/// enough to keep the file small.
pub const ARC_STEP_DEG: f64 = 6.0;

/// Largest distance (mm) between a true arc and the straight pieces that stand for it. A big radius gets
/// shorter pieces than a small one, so a large rounded corner or a circle does not look like a polygon.
pub const ARC_TOLERANCE: f64 = 0.002;

/// The turn (radians) of one straight piece of an arc of radius `r`: within `ARC_TOLERANCE`, never above `ARC_STEP_DEG`.
fn arc_step(r: f64) -> f64 {
    let by_tolerance = if r > ARC_TOLERANCE { 2.0 * (1.0 - ARC_TOLERANCE / r).clamp(-1.0, 1.0).acos() } else { PI / 2.0 };
    by_tolerance.min(ARC_STEP_DEG.to_radians()).max(0.001)
}

fn sub(a: Point, b: Point) -> (f64, f64) {
    (a.x - b.x, a.y - b.y)
}

fn unit(v: (f64, f64)) -> Option<(f64, f64)> {
    let l = v.0.hypot(v.1);
    (l > 1e-12).then(|| (v.0 / l, v.1 / l))
}

/// Replaces every corner of `poly` by an arc of radius `radius` (mm). A corner whose neighbours are
/// too close for that radius gets the largest radius that fits, so the shape never overlaps itself;
/// a straight "corner" is left alone. `radius <= 0` returns the polygon unchanged.
pub fn round_corners(poly: &[Point], radius: f64) -> Vec<Point> {
    let n = poly.len();
    if n < 3 || !(radius > 0.0) {
        return poly.to_vec();
    }
    let mut out = Vec::with_capacity(n * 8);
    for i in 0..n {
        let (p0, p1, p2) = (poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]);
        let (Some(v1), Some(v2)) = (unit(sub(p0, p1)), unit(sub(p2, p1))) else {
            out.push(p1);
            continue;
        };
        let cos = (v1.0 * v2.0 + v1.1 * v2.1).clamp(-1.0, 1.0);
        let theta = cos.acos(); // interior angle at the corner
        if theta > PI - 1e-3 || theta < 1e-3 {
            out.push(p1); // straight through, or a spike: nothing to round
            continue;
        }
        let half = theta / 2.0;
        // distance from the corner to the tangent points, limited to half of each neighbouring edge
        let reach = (p0.dist(p1).min(p2.dist(p1)) / 2.0).max(1e-9);
        let d = (radius / half.tan()).min(reach);
        let r = d * half.tan();
        let (t1, t2) = (Point::new(p1.x + v1.0 * d, p1.y + v1.1 * d), Point::new(p1.x + v2.0 * d, p1.y + v2.1 * d));
        let Some(bis) = unit((v1.0 + v2.0, v1.1 + v2.1)) else {
            out.push(p1);
            continue;
        };
        let k = r / half.sin();
        let c = Point::new(p1.x + bis.0 * k, p1.y + bis.1 * k);
        let (a1, a2) = ((t1.y - c.y).atan2(t1.x - c.x), (t2.y - c.y).atan2(t2.x - c.x));
        let mut sweep = a2 - a1;
        while sweep > PI {
            sweep -= 2.0 * PI;
        }
        while sweep < -PI {
            sweep += 2.0 * PI;
        }
        let steps = ((sweep.abs() / arc_step(r)).ceil() as usize).max(1);
        for s in 0..=steps {
            let a = a1 + sweep * s as f64 / steps as f64;
            out.push(Point::new(c.x + r * a.cos(), c.y + r * a.sin()));
        }
    }
    out
}

/// The points of the arc that starts at `a`, passes through `m` and ends at `b` (`a` is not in the list, `b` is). `None` when the
/// three points are on one line (there is no circle): the caller draws a straight line.
pub fn arc_through(a: Point, m: Point, b: Point) -> Option<Vec<Point>> {
    let d = 2.0 * (a.x * (m.y - b.y) + m.x * (b.y - a.y) + b.x * (a.y - m.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let (sa, sm, sb) = (a.x * a.x + a.y * a.y, m.x * m.x + m.y * m.y, b.x * b.x + b.y * b.y);
    let c = Point::new((sa * (m.y - b.y) + sm * (b.y - a.y) + sb * (a.y - m.y)) / d, (sa * (b.x - m.x) + sm * (a.x - b.x) + sb * (m.x - a.x)) / d);
    let r = c.dist(a);
    let ang = |p: Point| (p.y - c.y).atan2(p.x - c.x);
    let (a0, am, a1) = (ang(a), ang(m), ang(b));
    let norm = |x: f64| x.rem_euclid(2.0 * PI);
    // counter-clockwise when the middle point is reached before the end going that way, else clockwise
    let ccw = norm(am - a0) < norm(a1 - a0);
    let sweep = if ccw { norm(a1 - a0) } else { -norm(a0 - a1) };
    Some(arc_points(c, r, a0, sweep))
}

/// The points of the arc that starts at `a` going in the direction `t` (the arc is tangent to it there) and ends at `b`. `None` when
/// `b` lies on the line of `t` (a straight line).
pub fn arc_tangent(a: Point, t: (f64, f64), b: Point) -> Option<Vec<Point>> {
    let t = unit(t)?;
    let n = (-t.1, t.0);
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let denom = 2.0 * (dx * n.0 + dy * n.1);
    if denom.abs() < 1e-9 {
        return None;
    }
    let r = (dx * dx + dy * dy) / denom; // signed: positive when the centre is on the left of the direction
    let c = Point::new(a.x + n.0 * r, a.y + n.1 * r);
    let (a0, a1) = ((a.y - c.y).atan2(a.x - c.x), (b.y - c.y).atan2(b.x - c.x));
    let norm = |x: f64| x.rem_euclid(2.0 * PI);
    let sweep = if r > 0.0 { norm(a1 - a0) } else { -norm(a0 - a1) };
    Some(arc_points(c, r.abs(), a0, sweep))
}

/// The points along an arc of centre `c` and radius `r` from the angle `a0` turning by `sweep` (radians, negative: clockwise), the start left out.
fn arc_points(c: Point, r: f64, a0: f64, sweep: f64) -> Vec<Point> {
    let steps = ((sweep.abs() / arc_step(r)).ceil() as usize).max(1);
    (1..=steps)
        .map(|s| {
            let a = a0 + sweep * s as f64 / steps as f64;
            Point::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

/// A rectangle with rounded corners (`radius` is limited to half of the shorter side).
pub fn rounded_rect(x0: f64, y0: f64, x1: f64, y1: f64, radius: f64) -> Vec<Point> {
    let (lo, hi) = (Point::new(x0.min(x1), y0.min(y1)), Point::new(x0.max(x1), y0.max(y1)));
    let rect = vec![lo, Point::new(hi.x, lo.y), hi, Point::new(lo.x, hi.y)];
    round_corners(&rect, radius.min((hi.x - lo.x).min(hi.y - lo.y) / 2.0))
}

/// A rectangle with rounded corners as a shape that keeps its arcs: straight sides, and an arc at each corner (`radius` 0: four corners).
pub fn rounded_rect_arcs(x0: f64, y0: f64, x1: f64, y1: f64, radius: f64) -> crate::shape::Shape {
    use crate::shape::{Shape, Vertex};
    let (lo, hi) = (Point::new(x0.min(x1), y0.min(y1)), Point::new(x0.max(x1), y0.max(y1)));
    let r = radius.min((hi.x - lo.x).min(hi.y - lo.y) / 2.0);
    if r <= 1e-9 {
        return Shape::from_points(&[lo, Point::new(hi.x, lo.y), hi, Point::new(lo.x, hi.y)]);
    }
    let k = r * std::f64::consts::FRAC_1_SQRT_2;
    let (br, tr, tl, bl) = (Point::new(hi.x - r, lo.y + r), Point::new(hi.x - r, hi.y - r), Point::new(lo.x + r, hi.y - r), Point::new(lo.x + r, lo.y + r));
    let at = |p: Point, mid: Option<Point>| Vertex { p, arc_mid: mid };
    Shape {
        verts: vec![
            at(Point::new(lo.x + r, lo.y), None),
            at(Point::new(hi.x - r, lo.y), Some(Point::new(br.x + k, br.y - k))),
            at(Point::new(hi.x, lo.y + r), None),
            at(Point::new(hi.x, hi.y - r), Some(Point::new(tr.x + k, tr.y + k))),
            at(Point::new(hi.x - r, hi.y), None),
            at(Point::new(lo.x + r, hi.y), Some(Point::new(tl.x - k, tl.y + k))),
            at(Point::new(lo.x, hi.y - r), None),
            at(Point::new(lo.x, lo.y + r), Some(Point::new(bl.x - k, bl.y - k))),
        ],
    }
}

/// A circle (or the ellipse that fits the box when the box is not square) as a polygon.
pub fn ellipse(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
    let (c, rx, ry) = (Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0), (x1 - x0).abs() / 2.0, (y1 - y0).abs() / 2.0);
    let n = ((2.0 * PI / arc_step(rx.max(ry))).ceil() as usize).max((360.0 / ARC_STEP_DEG) as usize);
    (0..n)
        .map(|k| {
            let a = 2.0 * PI * k as f64 / n as f64;
            Point::new(c.x + rx * a.cos(), c.y + ry * a.sin())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{inside_polygon, signed_area};

    #[test]
    fn rounded_rectangle_loses_the_corner_area_and_stays_inside_the_box() {
        let p = rounded_rect(0.0, 0.0, 40.0, 25.0, 5.0);
        let expect = 40.0 * 25.0 - (4.0 - PI) * 25.0;
        assert!((signed_area(&p).abs() - expect).abs() < 1.0, "{} vs {expect}", signed_area(&p));
        assert!(p.iter().all(|q| (-1e-9..=40.0 + 1e-9).contains(&q.x) && (-1e-9..=25.0 + 1e-9).contains(&q.y)));
        // the original corner is cut off, the middle of the board is in
        assert!(!inside_polygon(&p, Point::new(0.2, 0.2)));
        assert!(inside_polygon(&p, Point::new(20.0, 12.0)));
        // the polygon keeps its orientation (counter-clockwise rectangle in, counter-clockwise out)
        assert!(signed_area(&p) > 0.0);
    }

    #[test]
    fn too_large_a_radius_is_limited_so_the_shape_stays_valid() {
        // 10 x 4 board, radius 50: becomes a stadium, never wider than the box
        let p = rounded_rect(0.0, 0.0, 10.0, 4.0, 50.0);
        assert!(p.iter().all(|q| q.x >= -1e-9 && q.x <= 10.0 + 1e-9 && q.y >= -1e-9 && q.y <= 4.0 + 1e-9));
        let area = signed_area(&p).abs();
        let stadium = 6.0 * 4.0 + PI * 4.0;
        assert!((area - stadium).abs() < 0.5, "{area} vs {stadium}");
        // a polygon corner whose neighbours are close gets a smaller arc, not a crossing path
        let tri = round_corners(&[Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(5.0, 1.0)], 3.0);
        assert!(tri.len() > 3 && signed_area(&tri) > 0.0);
    }

    #[test]
    fn an_arc_through_three_points_follows_the_circle_and_ends_at_the_last() {
        // a half circle of radius 5 from (-5,0) over the top (0,5) to (5,0)
        let up = arc_through(Point::new(-5.0, 0.0), Point::new(0.0, 5.0), Point::new(5.0, 0.0)).unwrap();
        assert!(up.last().unwrap().dist(Point::new(5.0, 0.0)) < 1e-9);
        assert!(up.iter().all(|p| (p.dist(Point::new(0.0, 0.0)) - 5.0).abs() < 1e-6 && p.y >= -1e-9), "over the top");
        // the same ends through the bottom go the other way round
        let down = arc_through(Point::new(-5.0, 0.0), Point::new(0.0, -5.0), Point::new(5.0, 0.0)).unwrap();
        assert!(down.iter().all(|p| p.y <= 1e-9));
        // three points on a line: no arc
        assert!(arc_through(Point::new(0.0, 0.0), Point::new(1.0, 0.0), Point::new(2.0, 0.0)).is_none());
    }

    #[test]
    fn a_tangent_arc_leaves_in_the_given_direction() {
        // from the origin going right, to (5,5): a quarter circle of radius 5 turning left (centre (0,5))
        let arc = arc_tangent(Point::new(0.0, 0.0), (1.0, 0.0), Point::new(5.0, 5.0)).unwrap();
        assert!(arc.last().unwrap().dist(Point::new(5.0, 5.0)) < 1e-9);
        assert!(arc.iter().all(|p| (p.dist(Point::new(0.0, 5.0)) - 5.0).abs() < 1e-6));
        assert!(arc[0].x > 0.0 && arc[0].y >= 0.0 && arc[0].y < arc[0].x, "leaves to the right, then curves up: {:?}", arc[0]);
        // turning right instead: centre below
        let right = arc_tangent(Point::new(0.0, 0.0), (1.0, 0.0), Point::new(5.0, -5.0)).unwrap();
        assert!(right.iter().all(|p| (p.dist(Point::new(0.0, -5.0)) - 5.0).abs() < 1e-6));
        // straight ahead: none
        assert!(arc_tangent(Point::new(0.0, 0.0), (1.0, 0.0), Point::new(5.0, 0.0)).is_none());
    }

    #[test]
    fn circle_and_non_corners() {
        let c = ellipse(0.0, 0.0, 20.0, 20.0);
        assert!((signed_area(&c) - PI * 100.0).abs() < 2.0, "{}", signed_area(&c));
        assert!(inside_polygon(&c, Point::new(10.0, 10.0)) && !inside_polygon(&c, Point::new(0.5, 0.5)));
        // a collinear vertex and radius 0 are left alone
        let line = vec![Point::new(0.0, 0.0), Point::new(5.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0), Point::new(0.0, 10.0)];
        assert_eq!(round_corners(&line, 0.0), line);
        assert!(round_corners(&line, 1.0).contains(&Point::new(5.0, 0.0)));
    }

    #[test]
    fn a_curved_board_goes_through_export_preview_and_drc() {
        use crate::model::*;
        let mut b = Board::new("round", 40.0, 25.0);
        b.outline = rounded_rect(0.0, 0.0, 40.0, 25.0, 6.0);
        // a pad in the middle is fine; one tucked into the cut-off corner is outside the board
        b.pads.push(Pad::new(Point::new(20.0, 12.0), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top));
        assert!(crate::drc::check(&b).is_empty(), "{:?}", crate::drc::check(&b));
        b.pads.push(Pad::new(Point::new(0.8, 0.8), PadShape::Circle { d: 1.6 }, Some(0.8), Side::Top));
        assert!(crate::drc::check(&b).iter().any(|v| v.message.contains("基板")), "the corner pad is off the curved board");
        b.pads.pop();
        let files = crate::export::fab_files(&b);
        let pv = crate::gerber_view::Preview::from_files(&files, 600).unwrap();
        assert!((pv.size_mm.0 - 40.0).abs() < 0.01 && (pv.size_mm.1 - 25.0).abs() < 0.01);
        // the corner of the bounding box is empty in the picture, the middle is not
        let img = pv.render(false, true, true);
        let (w, h) = (pv.frame.w, pv.frame.h);
        assert_eq!(img[2 * w + 2][3], 0, "rounded-off corner has no board");
        assert_eq!(img[(h / 2) * w + w / 2][3], 255);
    }
}
