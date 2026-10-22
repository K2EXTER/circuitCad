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

    /// The wire tool's box in trunk mode: the steps, and what the tool last said (the status line is overwritten with the
    /// cursor position, so a refusal is read here).
    pub(super) fn trunk_help(&self, ui: &mut egui::Ui) {
        ui.small("① 同じネットで隣り合うパッドを2つクリック (例: QFN の2本の LX ピン)");
        ui.small("② 幹を伸ばす先をクリック (パッドから 1mm 以上離れた所)");
        ui.small(format!("パッドの首は下の「線幅」({:.2} mm)、幹は、2つのパッドの外側の縁までの太さになります。右クリックで取り消し", self.track_width));
        ui.small("近くに別のネットがあって、間隔が足りなくなるときは、引かずに理由を出します");
        match &self.trunk_note {
            Some(Ok(m)) => {
                ui.colored_label(Color32::from_rgb(110, 200, 130), m.as_str());
            }
            Some(Err(m)) => {
                ui.colored_label(Color32::from_rgb(255, 130, 100), m.as_str());
            }
            None => {}
        }
        ui.separator();
    }

    /// The picture at the cursor in trunk mode: rings on the pads picked, and with two of them the neck and trunk as they would
    /// be drawn to the cursor, or why they cannot be.
    pub(super) fn draw_trunk_ghost(&self, painter: &egui::Painter, rect: Rect, p: Point, scr: Pos2) {
        let ring = Color32::from_rgb(90, 230, 120);
        for &i in &self.trunk_pads {
            if let Some(pd) = self.board.pads.get(i) {
                let (w, h) = pd.shape.size();
                painter.circle_stroke(self.to_screen(rect, pd.pos), (w.max(h) as f32 * self.zoom / 2.0 + 5.0).max(8.0), Stroke::new(2.0, ring));
            }
        }
        if let [i, j] = self.trunk_pads[..] {
            match pcb_core::trunk::fan(&self.board, i, j, p, self.track_width) {
                Ok(f) => {
                    let col = self.side_color(self.side).gamma_multiply(0.7);
                    for n in &f.necks {
                        painter.line_segment([self.to_screen(rect, n.a), self.to_screen(rect, n.b)], Stroke::new((n.width as f32 * self.zoom).max(1.5), col));
                    }
                    let mut from = f.start;
                    for to in self.trunk_path(&f, p) {
                        painter.line_segment([self.to_screen(rect, from), self.to_screen(rect, to)], Stroke::new((f.width as f32 * self.zoom).max(1.5), col));
                        from = to;
                    }
                }
                Err(e) => {
                    painter.text(scr + Vec2::new(14.0, -14.0), egui::Align2::LEFT_BOTTOM, e, egui::FontId::proportional(13.0), Color32::from_rgb(255, 120, 90));
                }
            }
        }
    }
}
