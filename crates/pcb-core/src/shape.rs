//! A closed shape drawn like in a drafting program: corners, and between two corners a straight line or an arc. The shape keeps what
//! was drawn (the corners and the point each arc passes through), so it can be edited later; `flatten` gives the polygon of short
//! straight pieces that the pours, the DRC and the Gerber writer work with.

use crate::model::Point;
use crate::outline::{arc_through, rounded_rect_arcs};
use serde::{Deserialize, Serialize};

/// A corner of a shape. The piece from this corner to the next one is a straight line, or, with `arc_mid`, the arc that passes through
/// that point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
    pub p: Point,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc_mid: Option<Point>,
}

impl Vertex {
    pub fn line(p: Point) -> Vertex {
        Vertex { p, arc_mid: None }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub verts: Vec<Vertex>,
}

impl Shape {
    /// A shape of straight lines through `pts`.
    pub fn from_points(pts: &[Point]) -> Shape {
        Shape { verts: pts.iter().map(|p| Vertex::line(*p)).collect() }
    }

    /// A circle (two arcs).
    pub fn circle(c: Point, r: f64) -> Shape {
        Shape {
            verts: vec![
                Vertex { p: Point::new(c.x - r, c.y), arc_mid: Some(Point::new(c.x, c.y - r)) },
                Vertex { p: Point::new(c.x + r, c.y), arc_mid: Some(Point::new(c.x, c.y + r)) },
            ],
        }
    }

    /// A rectangle with rounded corners (`radius` 0: square corners).
    pub fn rect(x0: f64, y0: f64, x1: f64, y1: f64, radius: f64) -> Shape {
        rounded_rect_arcs(x0, y0, x1, y1, radius)
    }

    /// The polygon of short straight pieces (the corners, and the points along every arc).
    pub fn flatten(&self) -> Vec<Point> {
        let n = self.verts.len();
        let mut out = Vec::with_capacity(n * 4);
        for i in 0..n {
            let v = self.verts[i];
            out.push(v.p);
            if let Some(m) = v.arc_mid {
                let next = self.verts[(i + 1) % n].p;
                if let Some(mut arc) = arc_through(v.p, m, next) {
                    arc.pop(); // the end is the next corner
                    out.extend(arc);
                }
            }
        }
        out
    }

    /// The points of the piece from corner `i` to the next one: the two corners for a line, and the points along it for an arc.
    pub fn segment_points(&self, i: usize) -> Vec<Point> {
        let n = self.verts.len();
        if n == 0 {
            return vec![];
        }
        let a = self.verts[i % n];
        let b = self.verts[(i + 1) % n].p;
        match a.arc_mid.and_then(|m| arc_through(a.p, m, b)) {
            Some(mut pts) => {
                pts.insert(0, a.p);
                pts
            }
            None => vec![a.p, b],
        }
    }

    /// The piece (the index of the corner it starts at), the point on it, and the distance of that point to `p`, for the piece nearest to `p`.
    pub fn nearest_edge(&self, p: Point) -> Option<(usize, Point, f64)> {
        let mut best: Option<(usize, Point, f64)> = None;
        for i in 0..self.verts.len() {
            for w in self.segment_points(i).windows(2) {
                let c = closest_on_segment(p, w[0], w[1]);
                let d = c.dist(p);
                if best.is_none_or(|(_, _, bd)| d < bd) {
                    best = Some((i, c, d));
                }
            }
        }
        best
    }

    /// Put a new corner on the piece `i` at `at` (a point on it): a line becomes two lines, an arc becomes two arcs on the same circle.
    pub fn insert_vertex(&mut self, i: usize, at: Point) {
        let n = self.verts.len();
        if n == 0 {
            return;
        }
        let i = i % n;
        let chain = self.segment_points(i);
        // the piece of the chain that `at` is on
        let k = chain.windows(2).enumerate().min_by(|(_, x), (_, y)| closest_on_segment(at, x[0], x[1]).dist(at).total_cmp(&closest_on_segment(at, y[0], y[1]).dist(at))).map_or(0, |(k, _)| k);
        let new = if self.verts[i].arc_mid.is_some() {
            let mut first: Vec<Point> = chain[..=k].to_vec();
            first.push(at);
            let mut second = vec![at];
            second.extend_from_slice(&chain[k + 1..]);
            self.verts[i].arc_mid = Some(polyline_middle(&first));
            Vertex { p: at, arc_mid: Some(polyline_middle(&second)) }
        } else {
            Vertex::line(at)
        };
        self.verts.insert(i + 1, new);
    }

    /// Take corner `i` away (the pieces on both sides of it become one straight line). Refused (false) when the shape would be less than a shape.
    pub fn remove_vertex(&mut self, i: usize) -> bool {
        let n = self.verts.len();
        if i >= n || n <= 2 {
            return false;
        }
        let mut next = self.clone();
        next.verts.remove(i);
        let prev = (i + n - 1) % n;
        let prev = if prev > i { prev - 1 } else { prev };
        next.verts[prev].arc_mid = None;
        if next.flatten().len() < 3 {
            return false;
        }
        *self = next;
        true
    }

    /// Make the piece from corner `i` an arc that bulges outwards (a quarter of its length), to be shaped by its middle point.
    pub fn make_arc(&mut self, i: usize) {
        let n = self.verts.len();
        if n < 2 {
            return;
        }
        let (a, b) = (self.verts[i % n].p, self.verts[(i + 1) % n].p);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        // outwards: to the right of a shape that goes round counter-clockwise, to the left of one that goes round clockwise
        let side = if crate::geom::signed_area(&self.flatten()) >= 0.0 { -1.0 } else { 1.0 };
        let mid = Point::new((a.x + b.x) / 2.0 - dy * 0.25 * side, (a.y + b.y) / 2.0 + dx * 0.25 * side);
        self.verts[i % n].arc_mid = Some(mid);
    }

    /// Make the piece from corner `i` a straight line.
    pub fn make_line(&mut self, i: usize) {
        let n = self.verts.len();
        if n > 0 {
            self.verts[i % n].arc_mid = None;
        }
    }

    /// The same shape with every corner moved by `d`.
    pub fn moved(&self, d: Point) -> Shape {
        Shape { verts: self.verts.iter().map(|v| Vertex { p: Point::new(v.p.x + d.x, v.p.y + d.y), arc_mid: v.arc_mid.map(|m| Point::new(m.x + d.x, m.y + d.y)) }).collect() }
    }

    /// The box round the flattened shape: (low corner, high corner).
    pub fn bounds(&self) -> Option<(Point, Point)> {
        let pts = self.flatten();
        let first = *pts.first()?;
        Some(pts.iter().fold((first, first), |(lo, hi), p| (Point::new(lo.x.min(p.x), lo.y.min(p.y)), Point::new(hi.x.max(p.x), hi.y.max(p.y)))))
    }
}

fn closest_on_segment(p: Point, a: Point, b: Point) -> Point {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-18 { 0.0 } else { (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0) };
    Point::new(a.x + dx * t, a.y + dy * t)
}

/// The point halfway along a chain of points.
fn polyline_middle(chain: &[Point]) -> Point {
    let total: f64 = chain.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut left = total / 2.0;
    for w in chain.windows(2) {
        let l = w[0].dist(w[1]);
        if left <= l && l > 0.0 {
            let t = left / l;
            return Point::new(w[0].x + (w[1].x - w[0].x) * t, w[0].y + (w[1].y - w[0].y) * t);
        }
        left -= l;
    }
    chain.last().copied().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::signed_area;

    #[test]
    fn a_polygon_shape_flattens_to_its_corners() {
        let pts = [Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 5.0), Point::new(0.0, 5.0)];
        assert_eq!(Shape::from_points(&pts).flatten(), pts.to_vec());
    }

    #[test]
    fn a_circle_flattens_to_a_round_polygon_and_keeps_its_two_corners() {
        let c = Shape::circle(Point::new(5.0, 5.0), 3.0);
        assert_eq!(c.verts.len(), 2);
        let poly = c.flatten();
        assert!(poly.len() > 20);
        assert!(poly.iter().all(|p| (p.dist(Point::new(5.0, 5.0)) - 3.0).abs() < 1e-6));
        assert!((signed_area(&poly).abs() - std::f64::consts::PI * 9.0).abs() < 0.2);
    }

    #[test]
    fn a_rounded_rectangle_keeps_arcs_between_its_straight_sides() {
        let r = Shape::rect(0.0, 0.0, 20.0, 10.0, 2.0);
        assert_eq!(r.verts.len(), 8);
        assert_eq!(r.verts.iter().filter(|v| v.arc_mid.is_some()).count(), 4);
        let area = signed_area(&r.flatten()).abs();
        assert!((area - (200.0 - (4.0 - std::f64::consts::PI) * 4.0)).abs() < 0.2, "{area}");
        // square corners: four lines
        assert_eq!(Shape::rect(0.0, 0.0, 20.0, 10.0, 0.0).verts.len(), 4);
        // the file keeps the arcs
        let again: Shape = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(again, r);
    }

    #[test]
    fn a_corner_is_put_on_a_line_and_on_an_arc_and_taken_away_again() {
        let mut sq = Shape::from_points(&[Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0), Point::new(0.0, 10.0)]);
        let (i, at, d) = sq.nearest_edge(Point::new(5.0, -1.0)).unwrap();
        assert_eq!((i, d), (0, 1.0));
        sq.insert_vertex(i, at);
        assert_eq!(sq.verts.len(), 5);
        assert_eq!(sq.verts[1].p, Point::new(5.0, 0.0));
        assert!(sq.remove_vertex(1));
        assert_eq!(sq.verts.len(), 4);
        // an arc (a half circle) split in two: both pieces stay on the same circle
        let mut half = Shape { verts: vec![Vertex { p: Point::new(-5.0, 0.0), arc_mid: Some(Point::new(0.0, 5.0)) }, Vertex::line(Point::new(5.0, 0.0))] };
        let (i, at, _) = half.nearest_edge(Point::new(3.6, 4.0)).unwrap();
        half.insert_vertex(i, at);
        assert_eq!(half.verts.len(), 3);
        assert!(half.verts[0].arc_mid.is_some() && half.verts[1].arc_mid.is_some());
        for v in &half.verts {
            assert!((v.p.dist(Point::new(0.0, 0.0)) - 5.0).abs() < 0.05 || v.p.y.abs() < 1e-9);
            if let Some(m) = v.arc_mid {
                assert!((m.dist(Point::new(0.0, 0.0)) - 5.0).abs() < 0.05, "the middle of a piece is on the circle: {m:?}");
            }
        }
        // a triangle cannot lose a corner
        let mut tri = Shape::from_points(&[Point::new(0.0, 0.0), Point::new(4.0, 0.0), Point::new(0.0, 4.0)]);
        assert!(!tri.remove_vertex(0) || tri.flatten().len() >= 3);
    }

    #[test]
    fn a_line_is_made_an_arc_and_back() {
        let mut sq = Shape::from_points(&[Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0), Point::new(0.0, 10.0)]);
        let before = signed_area_of(&sq);
        sq.make_arc(0);
        assert!(sq.verts[0].arc_mid.is_some());
        assert!(signed_area_of(&sq) > before, "the arc bulges out of the square");
        sq.make_line(0);
        assert!((signed_area_of(&sq) - before).abs() < 1e-9);
    }

    fn signed_area_of(s: &Shape) -> f64 {
        crate::geom::signed_area(&s.flatten()).abs()
    }

    #[test]
    fn moving_a_shape_moves_the_arcs_with_it() {
        let c = Shape::circle(Point::new(0.0, 0.0), 2.0).moved(Point::new(10.0, 10.0));
        assert!(c.flatten().iter().all(|p| (p.dist(Point::new(10.0, 10.0)) - 2.0).abs() < 1e-6));
    }
}
