//! Where routes meet pads: extra snap points at the two ends of a pad (so two tracks can leave it side by side), and
//! the short stretch that leaves or enters a pad at a right angle to its edge, whatever the angle of the rest of the
//! route.

use super::*;

/// How far (mm) from the pad's edge a snap point at its end lies inside the pad (besides half a track width).
const END_INSET: f64 = 0.05;

impl BoardEditor {
    /// The pad whose copper holds point `p` on the working layer.
    pub(super) fn pad_holding(&self, p: Point) -> Option<&Pad> {
        self.board.pads.iter().find(|q| {
            let (w, h) = q.shape.size();
            (q.drill.is_some() || q.side == self.side) && (p.x - q.pos.x).abs() <= w / 2.0 + 1e-6 && (p.y - q.pos.y).abs() <= h / 2.0 + 1e-6
        })
    }

    /// Snap points at the two ends of the pads that are long enough for two tracks side by side (besides the centre):
    /// along each side that has room for two tracks and the clearance between them, a track's width from the edge.
    pub(super) fn pad_end_points(&self) -> Vec<Point> {
        let need = self.track_width + self.board.rules.min_clearance;
        let mut out = vec![];
        for q in self.board.pads.iter().filter(|q| q.drill.is_some() || q.side == self.side) {
            if matches!(q.shape, PadShape::Circle { .. }) {
                continue;
            }
            let (w, h) = q.shape.size();
            for (len, along_x) in [(w, true), (h, false)] {
                let off = len / 2.0 - self.track_width / 2.0 - END_INSET;
                if off > 0.0 && 2.0 * off >= need {
                    let d = if along_x { Point::new(off, 0.0) } else { Point::new(0.0, off) };
                    out.push(Point::new(q.pos.x + d.x, q.pos.y + d.y));
                    out.push(Point::new(q.pos.x - d.x, q.pos.y - d.y));
                }
            }
        }
        out
    }

    /// The point where a route leaves (or enters) the pad holding `at`, on the way to (from) `other`: `pad_lead_len`
    /// outside the pad's edge, straight out from the edge. `None` where there is nothing to do: no pad, a round pad,
    /// the free route mode, or the setting off.
    pub(super) fn pad_lead_point(&self, at: Point, other: Point) -> Option<Point> {
        if !self.pad_lead || self.route_mode == RouteMode::Free {
            return None;
        }
        let q = self.pad_holding(at)?;
        if matches!(q.shape, PadShape::Circle { .. }) {
            return None;
        }
        let (w, h) = q.shape.size();
        let (ox, oy) = (at.x - q.pos.x, at.y - q.pos.y);
        let (dx, dy) = (other.x - at.x, other.y - at.y);
        if dx.hypot(dy) < 1e-9 {
            return None;
        }
        // an end point of the pad: out through the side, at a right angle to the line of the ends. From the middle: out
        // through the edge the other end lies beyond.
        let (nx, ny) = if ox.abs() > 1e-6 && oy.abs() <= 1e-6 {
            (0.0, if dy >= 0.0 { 1.0 } else { -1.0 })
        } else if oy.abs() > 1e-6 && ox.abs() <= 1e-6 {
            (if dx >= 0.0 { 1.0 } else { -1.0 }, 0.0)
        } else if dx.abs() * h >= dy.abs() * w {
            (dx.signum(), 0.0)
        } else {
            (0.0, dy.signum())
        };
        let exit = if nx != 0.0 { w / 2.0 - nx * ox } else { h / 2.0 - ny * oy };
        let lead = exit + self.pad_lead_len;
        Some(Point::new(at.x + nx * lead, at.y + ny * lead))
    }

    /// Points to visit from `a` to `p` (the last one is `p`), honouring the route mode and the pad leads.
    pub(super) fn route_points(&self, a: Point, p: Point) -> Vec<Point> {
        let (mut qa, mut qp) = (self.pad_lead_point(a, p), self.pad_lead_point(p, a));
        // two pads close together, or one pad: the leads would use up the whole route, so there are none
        let (la, lp) = (qa.map_or(0.0, |q| a.dist(q)), qp.map_or(0.0, |q| p.dist(q)));
        if la + lp > 0.8 * a.dist(p) {
            qa = None;
            qp = None;
        }
        let from = qa.unwrap_or(a);
        let to = qp.unwrap_or(p);
        // of the two ways to bend, the one whose first leg continues the lead out of `a` and whose last leg continues the lead into `p`
        let straight = |s: Point, e: Point| {
            let l = s.dist(e);
            if l < 1e-9 { (0.0, 0.0) } else { ((e.x - s.x) / l, (e.y - s.y) / l) }
        };
        let score = |mid: &[Point]| {
            let full: Vec<Point> = std::iter::once(from).chain(mid.iter().copied()).collect();
            let mut s = 0.0;
            if let Some(q) = qa
                && full.len() >= 2
            {
                let (lead, leg) = (straight(a, q), straight(full[0], full[1]));
                s += lead.0 * leg.0 + lead.1 * leg.1;
            }
            if let Some(q) = qp
                && full.len() >= 2
            {
                let (lead, leg) = (straight(q, p), straight(full[full.len() - 2], full[full.len() - 1]));
                s += lead.0 * leg.0 + lead.1 * leg.1;
            }
            s
        };
        let mut mid = self.route_points_plain(from, to, self.bend_flip);
        if qa.is_some() || qp.is_some() {
            let other = self.route_points_plain(from, to, !self.bend_flip);
            if score(&other) > score(&mid) + 1e-9 {
                mid = other;
            }
        }
        let mut out = vec![];
        out.extend(qa);
        out.extend(mid);
        if qp.is_some() {
            out.push(p);
        }
        out
    }
}
