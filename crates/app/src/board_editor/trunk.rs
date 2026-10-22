//! The trunk tool ("幹"): two pads of one net side by side, then a place to go to. A thin neck leaves each pad and one wide
//! trunk takes over (see `pcb_core::trunk`).

use super::*;

impl BoardEditor {
    /// The surface-mount pad under `p`, if any (on a layer that is shown).
    pub(super) fn pad_at(&self, p: Point) -> Option<usize> {
        let tol = 4.0 / self.zoom as f64;
        self.board.pads.iter().enumerate().find_map(|(i, pd)| {
            let (w, h) = pd.shape.size();
            (pd.drill.is_none() && self.side_visible(pd.side) && (p.x - pd.pos.x).abs() <= w / 2.0 + tol && (p.y - pd.pos.y).abs() <= h / 2.0 + tol).then_some(i)
        })
    }

    /// Say something about the trunk tool: in the status line and in the tool's box (the status line is overwritten with the
    /// cursor position at once, so the box is where the reason of a refusal can be read).
    fn say(&mut self, r: Result<String, String>) {
        self.status = match &r {
            Ok(s) | Err(s) => s.clone(),
        };
        self.trunk_note = Some(r);
    }

    fn pad_name(&self, i: usize) -> String {
        let p = &self.board.pads[i];
        format!("{} の {} ({})", p.refdes, p.pin, p.net)
    }

    /// The trunk's path after its start: out of the pads for a first leg as long as the trunk is wide, then to `to`.
    pub(super) fn trunk_path(&self, fan: &pcb_core::trunk::Fan, to: Point) -> Vec<Point> {
        let lead = fan.width.max(0.5);
        let p1 = Point::new(fan.start.x + fan.dir.x * lead, fan.start.y + fan.dir.y * lead);
        let mut path = vec![p1];
        path.extend(self.route_points(p1, to));
        path
    }

    /// A click with the trunk tool: the first pad, the second pad, then where the trunk goes.
    pub(super) fn trunk_click(&mut self, p: Point) {
        if self.trunk_pads.len() < 2 {
            let Some(i) = self.pad_at(p) else {
                self.say(Err("パッドをクリックしてください (表面実装のパッドが対象です)".into()));
                return;
            };
            if self.trunk_pads.contains(&i) {
                self.say(Err("同じパッドです。同じネットの、隣のパッドをクリックしてください".into()));
                return;
            }
            if let Some(&first) = self.trunk_pads.first()
                && let Err(e) = pcb_core::trunk::check_pair(&self.board, first, i, self.track_width)
            {
                self.say(Err(e));
                return;
            }
            self.trunk_pads.push(i);
            let msg = if self.trunk_pads.len() == 1 {
                format!("{} を選びました。次に、同じネットで隣り合うパッドをクリックしてください", self.pad_name(i))
            } else {
                format!("{} と {} を選びました。幹を伸ばす先をクリックしてください (右クリックで取り消し)", self.pad_name(self.trunk_pads[0]), self.pad_name(i))
            };
            self.say(Ok(msg));
            return;
        }
        let (i, j) = (self.trunk_pads[0], self.trunk_pads[1]);
        let fan = match pcb_core::trunk::fan(&self.board, i, j, p, self.track_width) {
            Ok(f) => f,
            Err(e) => {
                self.say(Err(e));
                return;
            }
        };
        let path = self.trunk_path(&fan, p);
        self.checkpoint();
        match pcb_core::trunk::add(&mut self.board, &fan, &path) {
            Ok(n) => {
                self.say(Ok(format!("{n} 本の配線で、太い幹を引きました (幹の太さ {:.2} mm、パッドの首の太さ {:.2} mm)。続けて、別のパッドの組も選べます", fan.width, fan.necks[0].width)));
                self.trunk_pads.clear();
            }
            Err(e) => {
                self.undo.pop();
                self.say(Err(e));
            }
        }
    }
}
