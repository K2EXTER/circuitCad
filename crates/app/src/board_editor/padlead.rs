//! Where routes meet pads: extra snap points at the two ends of a pad (so two tracks can leave it side by side), and
//! the short stretch that leaves or enters a pad at a right angle to its edge, whatever the angle of the rest of the
//! route.

use super::*;
use std::rc::Rc;

/// The least (mm) a corner snap point lies from the centre of a pad, along an axis; nearer than this the corner and the
/// centre are one point.
const MIN_CORNER: f64 = 0.15;

impl BoardEditor {
    /// The pad whose copper holds point `p` on the working layer.
    pub(super) fn pad_holding(&self, p: Point) -> Option<&Pad> {
        self.board.pads.iter().find(|q| {
            let (w, h) = q.shape.size();
            (q.drill.is_some() || q.side == self.side) && (p.x - q.pos.x).abs() <= w / 2.0 + 1e-6 && (p.y - q.pos.y).abs() <= h / 2.0 + 1e-6
        })
    }

    /// The number of the pad whose copper holds point `p` on the working layer.
    pub(super) fn pad_index_holding(&self, p: Point) -> Option<usize> {
        self.board.pads.iter().position(|q| {
            let (w, h) = q.shape.size();
            (q.drill.is_some() || q.side == self.side) && (p.x - q.pos.x).abs() <= w / 2.0 + 1e-6 && (p.y - q.pos.y).abs() <= h / 2.0 + 1e-6
        })
    }

    /// A key for what the leads depend on: the copper, the working layer and the settings.
    fn lead_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        pcb_core::zonefill::geometry_key(&self.board).hash(&mut h);
        (format!("{:?}", self.side), self.pad_lead, self.route_mode == RouteMode::Free).hash(&mut h);
        for v in [self.track_width, self.pad_lead_len, self.board.rules.min_clearance] {
            v.to_bits().hash(&mut h);
        }
        h.finish()
    }

    /// The copper of the board as the leads see it, kept until the copper or the settings change.
    fn stretch_check(&self) -> Rc<drc::StretchCheck> {
        let key = self.lead_key();
        let mut cache = self.stretch_cache.borrow_mut();
        if let Some((k, c)) = cache.as_ref()
            && *k == key
        {
            return c.clone();
        }
        let c = Rc::new(drc::StretchCheck::new(&self.board));
        *cache = Some((key, c.clone()));
        c
    }

    /// Does the stretch from `at` (on pad number `pad`) out through its edge to `lead` keep clear of other copper?
    fn lead_clear(&self, pad: usize, at: Point, lead: Point) -> bool {
        self.stretch_check().clear(self.side, at, lead, self.track_width, pad)
    }

    /// How far (mm) from the centre of a pad `len` long a snap point lies along that direction: where a track of the
    /// working width has its edge flush with the pad's edge. 0 where that is less than `MIN_CORNER` from the centre
    /// (the point could not be told from the centre).
    fn flush_offset(&self, len: f64) -> f64 {
        let o = len / 2.0 - self.track_width / 2.0;
        if o >= MIN_CORNER { o } else { 0.0 }
    }

    /// Snap points of the rectangular and oval pads, besides the centre: the four corners, where a track of the working
    /// width lies flush with the pad's edges (so the tracks and the pad have their edges in line, and two tracks can
    /// leave one pad side by side). Where the pad is too narrow in one direction to tell the two apart, the two corners
    /// become one point on the middle line (the two ends of a thin pin). A point is left out where the neighbours leave
    /// no room for a lead.
    pub(super) fn pad_end_points(&self) -> Vec<Point> {
        let key = self.lead_key();
        if let Some((k, v)) = self.ends_cache.borrow().as_ref()
            && *k == key
        {
            return v.clone();
        }
        let leads = self.pad_lead && self.route_mode != RouteMode::Free;
        let mut out = vec![];
        for (n, q) in self.board.pads.iter().enumerate().filter(|(_, q)| q.drill.is_some() || q.side == self.side) {
            if matches!(q.shape, PadShape::Circle { .. }) {
                continue;
            }
            let (w, h) = q.shape.size();
            let (ox, oy) = (self.flush_offset(w), self.flush_offset(h));
            let xs: Vec<f64> = if ox > 0.0 { vec![-ox, ox] } else { vec![0.0] };
            let ys: Vec<f64> = if oy > 0.0 { vec![-oy, oy] } else { vec![0.0] };
            for &x in &xs {
                for &y in &ys {
                    if x == 0.0 && y == 0.0 {
                        continue; // the centre
                    }
                    let p = Point::new(q.pos.x + x, q.pos.y + y);
                    // a point is for leaving through a side: no use where the neighbours leave no room for the lead
                    if !leads || self.end_lead(n, p, None).is_some() {
                        out.push(p);
                    }
                }
            }
        }
        *self.ends_cache.borrow_mut() = Some((key, out.clone()));
        out
    }

    /// The point where a route leaves (or enters) the pad holding `at`, on the way to (from) `other`: `pad_lead_len`
    /// outside the pad's edge, straight out from the edge. `None` where there is nothing to do: no pad, a round pad,
    /// the free route mode, or the setting off.
    pub(super) fn pad_lead_point(&self, at: Point, other: Point) -> Option<Point> {
        if !self.pad_lead || self.route_mode == RouteMode::Free {
            return None;
        }
        let n = self.pad_index_holding(at)?;
        let q = &self.board.pads[n];
        if matches!(q.shape, PadShape::Circle { .. }) {
            return None;
        }
        let (w, h) = q.shape.size();
        let (ox, oy) = (at.x - q.pos.x, at.y - q.pos.y);
        let (dx, dy) = (other.x - at.x, other.y - at.y);
        if dx.hypot(dy) < 1e-9 {
            return None;
        }
        if ox.abs() > 1e-6 || oy.abs() > 1e-6 {
            // a corner or an end of the pad
            return self.end_lead(n, at, Some(other));
        }
        // from the middle: out through the edge the other end lies beyond
        let first = if dx.abs() * h >= dy.abs() * w { (dx.signum(), 0.0) } else { (0.0, dy.signum()) };
        self.first_clear_lead(n, at, other, first)
    }

    /// The ways out of pad `n` from `at`, a corner or an end of the pad (see [`Self::pad_end_points`]), best first. At a
    /// corner: through either of the two sides that meet there (so the track keeps flush with the other), the one more
    /// the way of `other` first. At an end of a thin pad: through the long side, at a right angle to the line of the
    /// ends, the way `other` lies first, then the other way, and last straight out of the end.
    fn end_directions(&self, n: usize, at: Point, other: Option<Point>) -> Vec<(f64, f64)> {
        let q = &self.board.pads[n];
        let (ox, oy) = (at.x - q.pos.x, at.y - q.pos.y);
        let (dx, dy) = other.map_or((0.0, 0.0), |o| (o.x - at.x, o.y - at.y));
        let sign = |v: f64, otherwise: f64| if v > 1e-9 { 1.0 } else if v < -1e-9 { -1.0 } else { otherwise };
        let (corner_x, corner_y) = (ox.abs() > 1e-6, oy.abs() > 1e-6);
        if corner_x && corner_y {
            let mut dirs = vec![(sign(ox, 1.0), 0.0), (0.0, sign(oy, 1.0))];
            dirs.sort_by(|a, b| (b.0 * dx + b.1 * dy).total_cmp(&(a.0 * dx + a.1 * dy)));
            dirs
        } else if corner_x {
            let first = sign(dy, sign(oy, 1.0));
            vec![(0.0, first), (0.0, -first), (sign(ox, 1.0), 0.0)]
        } else {
            let first = sign(dx, 1.0);
            vec![(first, 0.0), (-first, 0.0), (0.0, sign(oy, 1.0))]
        }
    }

    /// The lead out of the corner or end `at` of pad `n`: the first of [`Self::end_directions`] that has room, `None`
    /// where there is none. `other`: where the route goes (for the order of the ways out), if known.
    fn end_lead(&self, n: usize, at: Point, other: Option<Point>) -> Option<Point> {
        self.end_directions(n, at, other).into_iter().find_map(|d| self.lead_point(n, at, d).filter(|l| self.lead_clear(n, at, *l)))
    }

    /// The first of the four ways out of pad `n` from `at` that has room, starting with `first`, then the ones that
    /// point most the way of `other`.
    fn first_clear_lead(&self, n: usize, at: Point, other: Point, first: (f64, f64)) -> Option<Point> {
        let mut dirs: Vec<(f64, f64)> = vec![(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)].into_iter().filter(|d| *d != first).collect();
        let (dx, dy) = (other.x - at.x, other.y - at.y);
        dirs.sort_by(|a, b| (b.0 * dx + b.1 * dy).total_cmp(&(a.0 * dx + a.1 * dy)));
        std::iter::once(first).chain(dirs).find_map(|d| self.lead_point(n, at, d).filter(|l| self.lead_clear(n, at, *l)))
    }

    /// `pad_lead_len` outside the edge of pad `n` that the direction `d` (a unit vector along an axis) leaves through.
    fn lead_point(&self, n: usize, at: Point, d: (f64, f64)) -> Option<Point> {
        let q = &self.board.pads[n];
        let (w, h) = q.shape.size();
        let (ox, oy) = (at.x - q.pos.x, at.y - q.pos.y);
        let exit = if d.0 != 0.0 { w / 2.0 - d.0 * ox } else { h / 2.0 - d.1 * oy };
        let lead = exit + self.pad_lead_len;
        Some(Point::new(at.x + d.0 * lead, at.y + d.1 * lead))
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

    /// Make the tracks (`only`: these indices; all when `None`) leave their rectangular pads at a right angle, the
    /// two tracks of one wide pad from its two ends. The nets of "整えないネット" are left alone. Returns how many track
    /// ends were changed.
    pub(super) fn align_pad_entries(&mut self, only: Option<Vec<usize>>) -> usize {
        let conn = drc::connectivity(&self.board);
        let skip = |i: usize| conn.track_net(i).is_some_and(|n| self.board.finish_exclude.iter().any(|x| x == n));
        let list: Vec<usize> = only.unwrap_or_else(|| (0..self.board.tracks.len()).collect()).into_iter().filter(|&i| !skip(i)).collect();
        let opts = pcb_core::padentry::EntryOptions { lead: self.pad_lead_len, spread: self.pad_spread };
        pcb_core::padentry::align_entries(&mut self.board, &opts, Some(&list)).aligned
    }

    /// The button: do it for `only` (or every track), as one undo step.
    pub(super) fn align_pad_entries_button(&mut self, only: Option<Vec<usize>>) {
        self.checkpoint();
        let n = self.align_pad_entries(only);
        self.violations.clear();
        self.status = if n == 0 {
            self.undo.pop(); // nothing changed: no undo step
            "そろえる所がありませんでした (すでに垂直、丸いパッド、すき間が足りない、またはルール違反になる所だけです)".into()
        } else {
            format!("パッドの根元 {n} か所を、辺に垂直にそろえました")
        };
    }

    pub(super) fn panel_pad_entries(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("パッドの根元").on_hover_text("配線がパッドに入る・パッドから出る根元を、パッドの辺に垂直な短い区間にそろえます。自動配線の直後には、新しい配線に自動で行います (「パッドに垂直に出入り」が入っているとき)");
        ui.checkbox(&mut self.pad_spread, "2本は両端に寄せる").on_hover_text("同じパッドの同じ点から2本が出ているとき、パッドの両端に寄せて、2本を並べて出します (鋭角のくぼみができません)。2本ぶんの幅が入らない小さなパッドでは、寄せません");
        ui.horizontal(|ui| {
            if ui.button("全部の配線をそろえる").on_hover_text("すべての配線の、四角いパッドへの根元をそろえます。配線の経路が、パッドの近くだけ少し変わります。ルール違反が増える所と、接続が切れる所は変えません。「整えないネット」は除きます。元に戻すで取り消せます").clicked() {
                self.align_pad_entries_button(None);
            }
            let picked: Vec<usize> = self.selected.iter().chain(&self.multi).filter_map(|it| if let Item::Track(i) = it { Some(*i) } else { None }).collect();
            if ui.add_enabled(!picked.is_empty(), egui::Button::new(format!("選んだ ({} 本)", picked.len()))).on_hover_text("選択している配線の根元だけをそろえます").clicked() {
                self.align_pad_entries_button(Some(picked));
            }
        });
    }
}
