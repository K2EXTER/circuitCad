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
    fn moving_a_shape_moves_the_arcs_with_it() {
        let c = Shape::circle(Point::new(0.0, 0.0), 2.0).moved(Point::new(10.0, 10.0));
        assert!(c.flatten().iter().all(|p| (p.dist(Point::new(10.0, 10.0)) - 2.0).abs() < 1e-6));
    }
}
