//! The live route of the Track tool: with "自動経路" on, a route to a pad (a via, the end of a track) is searched from the
//! start to the cursor on the router's grid, going round what is in the way. Where there is no way round, or the
//! cursor is not on anything to connect to, the route is the plain bent line, and other nets' tracks that are in the
//! way are pushed aside if the shove is on.

use super::*;
use pcb_core::autoroute::Router;
use std::rc::Rc;

/// The longest a search for the preview may take (a debug build is many times slower, so it is given more).
const SEARCH_BUDGET: std::time::Duration = std::time::Duration::from_millis(if cfg!(debug_assertions) { 1500 } else { 40 });

/// What a route from the start to the cursor is: the tracks laid and how they come about.
#[derive(Clone, Debug)]
pub(super) struct AutoPlan {
    pub tracks: Vec<Track>,
    /// The tracks come from the search (they go round what is in the way).
    pub found: bool,
}

impl BoardEditor {
    /// Is `p` something a route connects to: a pad, a via or the end of a track on the working layer?
    pub(super) fn on_object(&self, p: Point) -> bool {
        self.snap_object(p).is_some_and(|c| c.dist(p) < 1e-9)
    }

    /// The net of the copper at `p`, if any.
    pub(super) fn net_at(&self, p: Point) -> Option<String> {
        if let Some(q) = self.pad_holding(p).filter(|q| !q.net.is_empty()) {
            return Some(q.net.clone());
        }
        let conn = drc::connectivity(&self.board);
        let on_end = |t: &Track| t.side == self.side && (t.a.dist(p) < 1e-6 || t.b.dist(p) < 1e-6);
        if let Some(i) = self.board.tracks.iter().position(on_end) {
            return conn.track_net(i).map(str::to_string);
        }
        self.board.vias.iter().position(|v| v.pos.dist(p) < 1e-6).and_then(|k| conn.via_net(k).map(str::to_string))
    }

    /// The router of the board as it is (the obstacle grid), kept until the copper or the settings change.
    /// The width, via and clearance a route of `net` is drawn with: those of its net class, or the settings of the tool.
    pub(super) fn route_settings(&self, net: Option<&str>) -> (f64, f64, f64, Option<f64>) {
        match net.and_then(|n| self.board.class_of(n)) {
            Some(c) => (c.track_width, c.via_diameter, c.via_drill, (c.clearance > self.board.rules.min_clearance + 1e-9).then_some(c.clearance)),
            None => (self.track_width, self.via_dia, self.via_drill, None),
        }
    }

    /// [`router`](Self::router) for a net (one of a net class has its own width, via and clearance).
    pub(super) fn router_for(&self, net: Option<&str>) -> Option<Rc<Router>> {
        use std::hash::{Hash, Hasher};
        let (width, via_dia, via_drill, clearance) = self.route_settings(net);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        pcb_core::zonefill::geometry_key(&self.board).hash(&mut h);
        for v in [width, via_dia, via_drill, clearance.unwrap_or(0.0), actions::route_pitch(self.grid)] {
            v.to_bits().hash(&mut h);
        }
        // the classes decide what the obstacles keep away
        for c in &self.board.net_classes {
            c.clearance.to_bits().hash(&mut h);
            c.nets.hash(&mut h);
        }
        let key = h.finish();
        let mut cache = self.router_cache.borrow_mut();
        if let Some((k, r)) = cache.as_ref()
            && *k == key
        {
            return Some(r.clone());
        }
        let opt = RouteOptions { track_width: width, via_diameter: via_dia, via_drill, grid: actions::route_pitch(self.grid), follow: None, clearance, pad_lead: None };
        let r = Rc::new(Router::new(&self.board, &opt)?);
        *cache = Some((key, r.clone()));
        Some(r)
    }

    /// A route from `a` to `p` that goes round what is in the way, or `None` when `p` is not something to connect to,
    /// there is no way round (in the box around the two points), or the result would break a clearance rule.
    fn searched_route(&self, a: Point, p: Point) -> Option<Vec<Track>> {
        if !self.auto_route || !self.on_object(p) || a.dist(p) < 1e-9 {
            return None;
        }
        // the route leaves and enters the pads at a right angle, as the plain route does
        let (mut qa, mut qp) = (self.pad_lead_point(a, p), self.pad_lead_point(p, a));
        if qa.map_or(0.0, |q| a.dist(q)) + qp.map_or(0.0, |q| p.dist(q)) > 0.8 * a.dist(p) {
            qa = None;
            qp = None;
        }
        let (from, to) = (qa.unwrap_or(a), qp.unwrap_or(p));
        let net = self.net_at(a);
        let router = self.router_for(net.as_deref())?;
        let margin = 3.0 + 0.3 * a.dist(p);
        let found = router.path(net.as_deref(), from, to, self.side, false, margin, SEARCH_BUDGET)?;
        let mut tracks = vec![];
        let width = self.route_settings(net.as_deref()).0;
        if let Some(q) = qa {
            tracks.push(Track { side: self.side, a, b: q, width });
        }
        tracks.extend(found.tracks);
        if let Some(q) = qp {
            tracks.push(Track { side: self.side, a: q, b: p, width });
        }
        // the grid keeps the clearance, but a point that is off the grid (a pad's centre) may still be too close: check
        let count = |b: &Board| drc::clearance(b).iter().filter(|v| v.message.contains("銅箔どうし")).count();
        let mut trial = self.board.clone();
        trial.tracks.extend(tracks.iter().cloned());
        (count(&trial) <= count(&self.board)).then_some(tracks)
    }

    /// The route to lay from `a` to `p`: with "自動経路" off, the plain bent line; on, the searched route if there is
    /// one, else the plain line (which `push_route` lays with the shove).
    pub(super) fn auto_plan(&self, a: Point, p: Point) -> AutoPlan {
        match self.searched_route(a, p) {
            Some(tracks) => AutoPlan { tracks, found: true },
            None => AutoPlan { tracks: self.route_segments(a, p), found: false },
        }
    }
}
