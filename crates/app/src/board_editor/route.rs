//! Track routing: bend shapes and the segments a click adds.

use super::*;

/// The shortest straight piece a route is made of (mm) where it could be a hair: see `route_points_plain`.
const MIN_LEG: f64 = 0.3;

impl BoardEditor {
    /// Points to visit from `a` to `p` (the last one is `p`), honouring the route mode; `flip` is which way it bends.
    pub(super) fn route_points_plain(&self, a: Point, p: Point, flip: bool) -> Vec<Point> {
        let (dx, dy) = (p.x - a.x, p.y - a.y);
        let eps = 1e-9;
        if dx.abs() < eps || dy.abs() < eps || self.route_mode == RouteMode::Free {
            return vec![p];
        }
        match self.route_mode {
            RouteMode::Orthogonal => vec![if flip { Point::new(a.x, p.y) } else { Point::new(p.x, a.y) }, p],
            _ => {
                if (dx.abs() - dy.abs()).abs() < eps {
                    return vec![p];
                }
                let d = dx.abs().min(dy.abs());
                let (sx, sy) = (dx.signum(), dy.signum());
                // a straight piece of only a hair (the two legs of the way differ by a few hundredths of a mm, as when
                // a point off the grid, like a corner of a pad, is joined to one on it) would be a bump: lay the
                // way as a straight piece, the diagonal, and a short straight piece across, each one long enough
                let rest = dx.abs().max(dy.abs()) - d;
                if rest < MIN_LEG && d - MIN_LEG >= MIN_LEG {
                    let diag = d - MIN_LEG;
                    // the piece along the longer axis (rest + MIN_LEG) first, or the one across (MIN_LEG) with `flip`
                    let x_is_long = dx.abs() > dy.abs();
                    let long_first = !flip;
                    let along_x = long_first == x_is_long;
                    let len = if long_first { rest + MIN_LEG } else { MIN_LEG };
                    let first = if along_x { Point::new(a.x + sx * len, a.y) } else { Point::new(a.x, a.y + sy * len) };
                    let second = Point::new(first.x + sx * diag, first.y + sy * diag);
                    // what is left to `p` is one straight piece (along y after a first piece along x, and the other way round)
                    let straight = if along_x { (p.x - second.x).abs() < 1e-9 } else { (p.y - second.y).abs() < 1e-9 };
                    if straight {
                        return vec![first, second, p];
                    }
                }
                let diag_first = Point::new(a.x + sx * d, a.y + sy * d);
                let straight_first = if dx.abs() > dy.abs() {
                    Point::new(a.x + sx * (dx.abs() - d), a.y)
                } else {
                    Point::new(a.x, a.y + sy * (dy.abs() - d))
                };
                vec![if flip { diag_first } else { straight_first }, p]
            }
        }
    }

    /// Run from `a` to `p`, drop a via there and carry on from `p` on the other layer.
    pub(super) fn route_via(&mut self, a: Point, p: Point) {
        self.checkpoint();
        self.push_route(a, p);
        let net = self.net_at(a);
        let (_, via_dia, via_drill, _) = self.route_settings(net.as_deref());
        // a via dropped while routing is of the net that is being routed
        self.board.vias.push(Via { pos: p, diameter: via_dia, drill: via_drill, net: net.unwrap_or_default() });
        self.side = self.next_layer();
        self.pending = Some(p);
    }

    /// The tracks a route from `a` to `p` would lay on the working layer.
    pub(super) fn route_segments(&self, a: Point, p: Point) -> Vec<Track> {
        let mut from = a;
        let mut v = vec![];
        // the width of the net class of the net the route starts on
        let width = self.route_settings(self.net_at(a).as_deref()).0;
        for q in self.route_points(a, p) {
            if from.dist(q) > 1e-9 {
                v.push(Track { side: self.side, a: from, b: q, width });
            }
            from = q;
        }
        v
    }

    pub(super) fn push_route(&mut self, a: Point, p: Point) {
        let plan = self.auto_plan(a, p);
        if plan.found {
            self.status = "自動経路: 障害物をよけて配線しました".into();
            self.board.tracks.extend(plan.tracks);
        } else if self.auto_route && self.shove {
            // no way round (or nothing to connect to): the plain line, with the tracks in the way pushed aside
            let pushed = pcb_core::shove::add_tracks(&mut self.board, plan.tracks);
            if pushed > 0 {
                self.status = format!("押しのけ配線: 邪魔な配線 {pushed} 本を脇へ寄せました");
            }
        } else {
            self.board.tracks.extend(plan.tracks);
        }
    }
}
