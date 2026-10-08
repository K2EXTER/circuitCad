//! Small polygon helpers shared by DRC, zones and the editor.

use crate::model::Point;

/// Even-odd point-in-polygon test.
pub fn inside_polygon(poly: &[Point], p: Point) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Signed area (positive for counter-clockwise).
pub fn signed_area(poly: &[Point]) -> f64 {
    let n = poly.len();
    (0..n).map(|i| poly[i].x * poly[(i + 1) % n].y - poly[(i + 1) % n].x * poly[i].y).sum::<f64>() / 2.0
}

fn cross(o: Point, a: Point, b: Point) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

fn in_triangle(p: Point, a: Point, b: Point, c: Point) -> bool {
    let (d1, d2, d3) = (cross(a, b, p), cross(b, c, p), cross(c, a, p));
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// Ear-clipping triangulation of a simple polygon (convex or concave). Returns an empty list
/// for degenerate input.
pub fn triangulate(poly: &[Point]) -> Vec<[Point; 3]> {
    if poly.len() < 3 {
        return vec![];
    }
    let mut idx: Vec<usize> = (0..poly.len()).collect();
    if signed_area(poly) < 0.0 {
        idx.reverse();
    }
    let mut out = vec![];
    let mut guard = 0;
    while idx.len() > 3 && guard < 10_000 {
        guard += 1;
        let n = idx.len();
        let mut clipped = false;
        for k in 0..n {
            let (ia, ib, ic) = (idx[(k + n - 1) % n], idx[k], idx[(k + 1) % n]);
            let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
            if cross(a, b, c) <= 1e-12 {
                continue; // reflex or degenerate corner
            }
            let blocked = idx.iter().any(|&j| j != ia && j != ib && j != ic && in_triangle(poly[j], a, b, c));
            if blocked {
                continue;
            }
            out.push([a, b, c]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            return out; // not a simple polygon: give back what we have
        }
    }
    if idx.len() == 3 {
        out.push([poly[idx[0]], poly[idx[1]], poly[idx[2]]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn point_in_polygon() {
        let sq = [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)];
        assert!(inside_polygon(&sq, p(5.0, 5.0)));
        assert!(!inside_polygon(&sq, p(11.0, 5.0)));
        assert!(!inside_polygon(&sq[..2], p(5.0, 5.0)));
    }

    #[test]
    fn triangulation_covers_the_area_of_concave_polygons() {
        // L shape (area 75), clockwise on purpose
        let l = [p(0.0, 0.0), p(0.0, 10.0), p(5.0, 10.0), p(5.0, 5.0), p(10.0, 5.0), p(10.0, 0.0)];
        assert!((signed_area(&l) + 75.0).abs() < 1e-9);
        let t = triangulate(&l);
        assert_eq!(t.len(), 4);
        let sum: f64 = t.iter().map(|tr| signed_area(tr).abs()).sum();
        assert!((sum - 75.0).abs() < 1e-9, "{sum}");
        // no triangle covers the notch
        assert!(t.iter().all(|tr| !in_triangle(p(7.5, 7.5), tr[0], tr[1], tr[2]) || signed_area(tr).abs() < 1e-9));
    }

    #[test]
    fn degenerate_input() {
        assert!(triangulate(&[p(0.0, 0.0), p(1.0, 1.0)]).is_empty());
        let tri = triangulate(&[p(0.0, 0.0), p(4.0, 0.0), p(0.0, 3.0)]);
        assert_eq!(tri.len(), 1);
    }
}
