//! Keep-out areas (rule areas): a polygon on some copper layers (or all) where tracks, vias, pours and / or parts must not
//! be. The DRC reports what lies in one, the router does not go through one, and pours leave one empty. Typical uses: under
//! an antenna, round a mounting screw's head or a heat sink, a strip along a connector that a case wall covers.

use crate::drc::{geo::point_seg, Violation};
use crate::geom::inside_polygon;
use crate::model::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Keepout {
    /// A name for the list and the findings (may be empty).
    #[serde(default)]
    pub name: String,
    pub polygon: Vec<Point>,
    /// The copper layers it holds for; empty: all of them.
    #[serde(default)]
    pub layers: Vec<Side>,
    pub tracks: bool,
    pub vias: bool,
    pub pours: bool,
    /// Parts may not stand in it (any pad, or the box round the pads, of a part mounted on one of its layers).
    pub parts: bool,
}

impl Keepout {
    /// An area that forbids everything on every layer.
    pub fn new(polygon: Vec<Point>) -> Self {
        Self { name: String::new(), polygon, layers: vec![], tracks: true, vias: true, pours: true, parts: true }
    }

    pub fn on(&self, side: Side) -> bool {
        self.layers.is_empty() || self.layers.contains(&side)
    }

    /// The shortest distance from `p` to the area (0 inside).
    pub fn distance(&self, p: Point) -> f64 {
        if self.polygon.len() < 3 {
            return f64::MAX;
        }
        if inside_polygon(&self.polygon, p) {
            return 0.0;
        }
        let n = self.polygon.len();
        (0..n).map(|i| point_seg(p, self.polygon[i], self.polygon[(i + 1) % n])).fold(f64::MAX, f64::min)
    }

    /// Does the segment `a`-`b` grown by `r` reach into the area?
    pub fn touches_segment(&self, a: Point, b: Point, r: f64) -> bool {
        if self.polygon.len() < 3 {
            return false;
        }
        if inside_polygon(&self.polygon, a) || inside_polygon(&self.polygon, b) {
            return true;
        }
        let n = self.polygon.len();
        (0..n).any(|i| {
            let (c, d) = (self.polygon[i], self.polygon[(i + 1) % n]);
            crate::drc::geo::segs_intersect(a, b, c, d) || point_seg(c, a, b) < r || point_seg(d, a, b) < r || point_seg(a, c, d) < r || point_seg(b, c, d) < r
        })
    }

    fn label(&self, i: usize) -> String {
        if self.name.is_empty() { format!("禁止領域 {}", i + 1) } else { format!("禁止領域「{}」", self.name) }
    }

    /// What it forbids, in words ("配線・ビア・ベタ・部品").
    pub fn what(&self) -> String {
        let mut w = vec![];
        for (on, s) in [(self.tracks, "配線"), (self.vias, "ビア"), (self.pours, "ベタ"), (self.parts, "部品")] {
            if on {
                w.push(s);
            }
        }
        w.join("・")
    }
}

/// The DRC findings: tracks, vias and parts inside an area that forbids them.
pub fn keepout_checks(board: &Board) -> Vec<Violation> {
    let mut out = vec![];
    for (i, k) in board.keepouts.iter().enumerate().filter(|(_, k)| k.polygon.len() >= 3) {
        if k.tracks {
            for t in board.tracks.iter().filter(|t| k.on(t.side)) {
                if k.touches_segment(t.a, t.b, t.width / 2.0) {
                    let at = if k.distance(t.a) == 0.0 { t.a } else if k.distance(t.b) == 0.0 { t.b } else { Point::new((t.a.x + t.b.x) / 2.0, (t.a.y + t.b.y) / 2.0) };
                    out.push(Violation { message: format!("{}に配線が入っています ({})。この領域の外を通るように引き直しましょう", k.label(i), t.side.label()), at });
                }
            }
        }
        if k.vias {
            for v in &board.vias {
                if k.distance(v.pos) < v.diameter / 2.0 {
                    out.push(Violation { message: format!("{}にビアがあります。領域の外へ動かしましょう", k.label(i)), at: v.pos });
                }
            }
        }
        if k.parts {
            for p in board.parts.iter().filter(|p| k.on(p.side)) {
                let pads: Vec<&Pad> = board.pads.iter().filter(|q| q.refdes == p.refdes).collect();
                let Some((lo, hi)) = pad_box(&pads) else { continue };
                let corners = [lo, Point::new(hi.x, lo.y), hi, Point::new(lo.x, hi.y)];
                let inside = (0..4).any(|c| k.touches_segment(corners[c], corners[(c + 1) % 4], 0.0)) || inside_polygon(&corners, k.polygon[0]);
                if inside {
                    out.push(Violation { message: format!("{}に部品 {} が入っています。部品を領域の外へ動かしましょう", k.label(i), p.refdes), at: p.pos });
                }
            }
        }
    }
    out
}

fn pad_box(pads: &[&Pad]) -> Option<(Point, Point)> {
    let mut it = pads.iter();
    let first = it.next()?;
    let b = |p: &Pad| {
        let (w, h) = p.shape.size();
        (Point::new(p.pos.x - w / 2.0, p.pos.y - h / 2.0), Point::new(p.pos.x + w / 2.0, p.pos.y + h / 2.0))
    };
    let (mut lo, mut hi) = b(first);
    for p in it {
        let (a, c) = b(p);
        lo = Point::new(lo.x.min(a.x), lo.y.min(a.y));
        hi = Point::new(hi.x.max(c.x), hi.y.max(c.y));
    }
    Some((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drc;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
        vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)]
    }

    fn board() -> Board {
        let mut b = Board::new("t", 40.0, 30.0);
        for (x, y) in [(5.0, 15.0), (35.0, 15.0)] {
            let mut p = Pad::new(Point::new(x, y), PadShape::Rect { w: 1.0, h: 1.0 }, None, Side::Top);
            p.net = "SIG".into();
            b.pads.push(p);
        }
        b
    }

    #[test]
    fn a_track_and_a_via_in_the_area_are_reported_and_the_layer_matters() {
        let mut b = board();
        b.keepouts.push(Keepout { layers: vec![Side::Top], ..Keepout::new(rect(15.0, 10.0, 25.0, 20.0)) });
        b.tracks.push(Track { side: Side::Top, a: Point::new(5.0, 15.0), b: Point::new(35.0, 15.0), width: 0.25 });
        let found = drc::check(&b);
        assert!(found.iter().any(|v| v.message.contains("禁止領域 1に配線")), "{found:?}");
        // the same track on the bottom is allowed: the area holds for the top only
        b.tracks[0].side = Side::Bottom;
        assert!(keepout_checks(&b).is_empty());
        b.vias.push(Via { pos: Point::new(20.0, 15.0), diameter: 0.6, drill: 0.3, net: String::new() });
        assert!(keepout_checks(&b).iter().any(|v| v.message.contains("ビア")));
        // a track that only passes close by stays clear
        b.vias.clear();
        b.tracks[0] = Track { side: Side::Top, a: Point::new(5.0, 9.5), b: Point::new(35.0, 9.5), width: 0.25 };
        assert!(keepout_checks(&b).is_empty());
        b.tracks[0].a.y = 9.9;
        b.tracks[0].b.y = 9.9;
        assert_eq!(keepout_checks(&b).len(), 1, "the edge of the track reaches in");
    }

    #[test]
    fn the_router_goes_round_the_area_and_the_board_stays_clean() {
        let mut b = board();
        b.keepouts.push(Keepout::new(rect(15.0, 5.0, 25.0, 25.0)));
        let r = crate::autoroute::autoroute(&mut b, &Default::default(), None);
        assert!(r.failed.is_empty(), "{:?}", r.failed);
        assert!(crate::ratsnest::ratsnest(&b).is_empty());
        assert!(drc::check(&b).is_empty(), "{:?}", drc::check(&b));
        assert!(b.tracks.iter().any(|t| t.a.y.max(t.b.y) > 25.0 || t.a.y.min(t.b.y) < 5.0), "the route went round");
    }

    #[test]
    fn a_pour_leaves_the_area_empty_and_a_part_in_it_is_reported() {
        let mut b = board();
        b.pads[1].net = "GND".into();
        b.zones.push(Zone::new("GND", Side::Top, rect(1.0, 1.0, 39.0, 29.0)));
        b.keepouts.push(Keepout::new(rect(15.0, 10.0, 25.0, 20.0)));
        let conn = drc::connectivity(&b);
        let fill: Vec<_> = crate::zonefill::merged_fills(&b, &conn).into_iter().flat_map(|(_, _, f)| f).collect();
        assert!(crate::zonefill::copper_area(&fill, Some((Point::new(20.0, 15.0), 4.0))) < 1e-6);
        assert!(crate::zonefill::copper_area(&fill, Some((Point::new(30.0, 25.0), 2.0))) > 10.0);
        b.set_part(Part { refdes: "R1".into(), value: "1k".into(), footprint: "R_0805".into(), pos: Point::new(20.0, 15.0), rot: 0, side: Side::Top }).unwrap();
        assert!(keepout_checks(&b).iter().any(|v| v.message.contains("部品 R1")));
        // an area that allows parts says nothing
        b.keepouts[0].parts = false;
        assert!(!keepout_checks(&b).iter().any(|v| v.message.contains("部品")));
    }
}
