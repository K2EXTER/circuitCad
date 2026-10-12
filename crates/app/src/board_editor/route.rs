//! Track routing: bend shapes and the segments a click adds.

use super::*;

impl BoardEditor {
    /// Points to visit from `a` to `p` (the last one is `p`), honouring the route mode.
    pub(super) fn route_points(&self, a: Point, p: Point) -> Vec<Point> {
        let (dx, dy) = (p.x - a.x, p.y - a.y);
        let eps = 1e-9;
        if dx.abs() < eps || dy.abs() < eps || self.route_mode == RouteMode::Free {
            return vec![p];
        }
        match self.route_mode {
            RouteMode::Orthogonal => vec![if self.bend_flip { Point::new(a.x, p.y) } else { Point::new(p.x, a.y) }, p],
            _ => {
                if (dx.abs() - dy.abs()).abs() < eps {
                    return vec![p];
                }
                let d = dx.abs().min(dy.abs());
                let (sx, sy) = (dx.signum(), dy.signum());
                let diag_first = Point::new(a.x + sx * d, a.y + sy * d);
                let straight_first = if dx.abs() > dy.abs() {
                    Point::new(a.x + sx * (dx.abs() - d), a.y)
                } else {
                    Point::new(a.x, a.y + sy * (dy.abs() - d))
                };
                vec![if self.bend_flip { diag_first } else { straight_first }, p]
            }
        }
    }

    /// Run from `a` to `p`, drop a via there and carry on from `p` on the other layer.
    pub(super) fn route_via(&mut self, a: Point, p: Point) {
        self.checkpoint();
        self.push_route(a, p);
        self.board.vias.push(Via { pos: p, diameter: self.via_dia, drill: self.via_drill });
        self.side = self.next_layer();
        self.pending = Some(p);
    }

    /// The tracks a route from `a` to `p` would lay on the working layer.
    pub(super) fn route_segments(&self, a: Point, p: Point) -> Vec<Track> {
        let mut from = a;
        let mut v = vec![];
        for q in self.route_points(a, p) {
            if from.dist(q) > 1e-9 {
                v.push(Track { side: self.side, a: from, b: q, width: self.track_width });
            }
            from = q;
        }
        v
    }

    pub(super) fn push_route(&mut self, a: Point, p: Point) {
        let new = self.route_segments(a, p);
        if self.shove {
            let pushed = pcb_core::shove::add_tracks(&mut self.board, new);
            if pushed > 0 {
                self.status = format!("押しのけ配線: 邪魔な配線 {pushed} 本を脇へ寄せました");
            }
        } else {
            self.board.tracks.extend(new);
        }
    }
}
