//! Board outlines with curves. The board outline is a polygon; curves are stored as short
//! straight pieces (a fab reads the outline as a closed path of segments anyway), so rounded
//! corners and circles need no special support in the Gerber writer, the DRC or the editor.

use crate::model::Point;
use std::f64::consts::PI;

/// Longest turn of one straight piece of a curve (degrees): small enough to look round, large
/// enough to keep the file small.
pub const ARC_STEP_DEG: f64 = 6.0;

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
        let steps = ((sweep.abs() / ARC_STEP_DEG.to_radians()).ceil() as usize).max(1);
        for s in 0..=steps {
            let a = a1 + sweep * s as f64 / steps as f64;
            out.push(Point::new(c.x + r * a.cos(), c.y + r * a.sin()));
        }
    }
    out
}

/// A rectangle with rounded corners (`radius` is limited to half of the shorter side).
pub fn rounded_rect(x0: f64, y0: f64, x1: f64, y1: f64, radius: f64) -> Vec<Point> {
    let (lo, hi) = (Point::new(x0.min(x1), y0.min(y1)), Point::new(x0.max(x1), y0.max(y1)));
    let rect = vec![lo, Point::new(hi.x, lo.y), hi, Point::new(lo.x, hi.y)];
    round_corners(&rect, radius.min((hi.x - lo.x).min(hi.y - lo.y) / 2.0))
}

/// A circle (or the ellipse that fits the box when the box is not square) as a polygon.
pub fn ellipse(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
    let (c, rx, ry) = (Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0), (x1 - x0).abs() / 2.0, (y1 - y0).abs() / 2.0);
    let n = (360.0 / ARC_STEP_DEG) as usize;
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
