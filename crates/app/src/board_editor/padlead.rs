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
