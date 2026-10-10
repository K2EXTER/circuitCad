//! Mouse interaction on the board: rubber-band selection and what the pointer does for the active tool.

use super::*;

impl BoardEditor {
    pub(super) fn box_selection(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, painter: &egui::Painter) {
        // rubber-band selection: left drag = fully enclosed, right drag = anything touched
        if self.tool == Tool::Select {
            let origin = ui.input(|i| i.pointer.press_origin());
            for (btn, crossing) in [(egui::PointerButton::Primary, false), (egui::PointerButton::Secondary, true)] {
                if resp.drag_started_by(btn) && self.drag_part.is_none() && self.gdrag.is_none()
                    && let Some(o) = origin {
                        let w0 = self.to_world(rect, o);
                        // a left drag that starts on a part or wire moves it instead (below); the inside
                        // of a copper pour does not count, or a box could not be drawn over one
                        if crossing || self.hit_test(w0).is_none_or(|h| matches!(h, Item::Zone(_))) {
                            self.boxsel = Some(BoxSel { start: w0, crossing });
                        }
                    }
            }
            if let Some(b) = &self.boxsel {
                let cur = ui.input(|i| i.pointer.latest_pos()).map(|p| self.to_world(rect, p)).unwrap_or(b.start);
                let r = Rect::from_two_pos(self.to_screen(rect, b.start), self.to_screen(rect, cur));
                let (fill, line) = if b.crossing {
                    (Color32::from_rgba_unmultiplied(60, 200, 90, 40), Color32::from_rgb(80, 220, 110))
                } else {
                    (Color32::from_rgba_unmultiplied(80, 140, 255, 40), Color32::from_rgb(110, 160, 255))
                };
                painter.rect_filled(r, 0.0, fill);
                painter.rect_stroke(r, 0.0, Stroke::new(1.2, line), egui::StrokeKind::Inside);
                if resp.drag_stopped() {
                    let (start, crossing) = (b.start, b.crossing);
                    let mut items = self.items_in_box(start, cur, crossing);
                    if ui.input(|i| i.modifiers.shift) {
                        let mut all = self.multi.clone();
                        all.extend(self.selected);
                        for it in items {
                            if !all.contains(&it) {
                                all.push(it);
                            }
                        }
                        items = all;
                    }
                    self.status = format!("{} 個を選択 ({}範囲)", items.len(), if crossing { "接触した物を含む" } else { "完全に囲んだ" });
                    self.set_selection(items);
                    self.boxsel = None;
                }
            }
        } else {
            self.boxsel = None;
        }
    }

    pub(super) fn pointer(&mut self, resp: &egui::Response, rect: Rect, painter: &egui::Painter) -> Option<Point> {
        let mut sp = None;
        if let Some(pos) = resp.hover_pos() {
            let w = self.to_world(rect, pos);
            let mut p = Point::new(snap(w.x, self.grid), snap(w.y, self.grid));
            if matches!(self.tool, Tool::Track) {
                // snap routing to pad / via centres so off-grid footprints can be connected
                let tol = 10.0 / self.zoom as f64;
                let near = self
                    .board
                    .pads
                    .iter()
                    .map(|p| p.pos)
                    .chain(self.board.vias.iter().map(|v| v.pos))
                    .filter(|c| c.dist(w) <= tol)
                    .min_by(|a, b| a.dist(w).total_cmp(&b.dist(w)));
                if let Some(c) = near {
                    p = c;
                }
            }
            sp = Some(p);
            self.draw_cursor(&painter, rect, p);

            if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select && self.multi.len() > 1 && self.boxsel.is_none()
                && self.hit_test(w).is_some_and(|h| self.multi.contains(&h)) {
                    self.checkpoint();
                    self.gdrag = Some(GroupDrag { anchor: p, orig: self.board.clone(), items: self.multi.clone() });
                }
            if let Some(g) = self.gdrag.take()
                && resp.dragged_by(egui::PointerButton::Primary) {
                    self.move_group(&g, p.x - g.anchor.x, p.y - g.anchor.y);
                    self.gdrag = Some(g);
                }
            if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select && self.gdrag.is_none() && self.multi.len() <= 1
                && let Some(Item::Part(i)) = self.hit_test(w) {
                    self.checkpoint();
                    let part = &self.board.parts[i];
                    self.drag_part = Some((part.refdes.clone(), Point::new(p.x - part.pos.x, p.y - part.pos.y)));
                    self.selected = Some(Item::Part(i));
                    self.multi.clear();
                }
            if let Some((r, off)) = self.drag_part.clone()
                && resp.dragged_by(egui::PointerButton::Primary)
                    && let Some(rot) = self.board.parts.iter().find(|q| q.refdes == r).map(|q| q.rot) {
                        self.board.place_part(&r, Point::new(p.x - off.x, p.y - off.y), rot);
                    }
            if resp.clicked_by(egui::PointerButton::Primary) {
                self.click(p);
            }
            if resp.double_clicked() && matches!(self.tool, Tool::OutlinePoly | Tool::Zone) {
                self.finish_polygon();
            }
            let layer = if self.side == Side::Top { "表面" } else { "裏面" };
            self.status = format!("X {:.2}  Y {:.2} mm   [{layer}]", p.x, p.y);
        }
        if resp.drag_stopped() {
            self.drag_part = None;
            self.gdrag = None;
        }
        if resp.clicked_by(egui::PointerButton::Secondary) {
            self.leave_tool();
        }
        sp
    }

    /// The right click, AutoCAD style: it confirms what is being drawn (ends the track; closes a
    /// polygon of three or more points, drops a shorter one) and the tool stays; with nothing in
    /// progress it returns to the Select tool. Returns whether anything happened.
    pub(super) fn leave_tool(&mut self) -> bool {
        if self.pending.take().is_some() {
            self.status = "配線を確定しました (もう一度右クリックで選択ツールに戻ります)".into();
            return true;
        }
        if !self.poly.is_empty() {
            if self.poly.len() >= 3 && matches!(self.tool, Tool::OutlinePoly | Tool::Zone) {
                self.finish_polygon();
            } else {
                self.poly.clear();
                self.status = "多角形の入力を取り消しました".into();
            }
            return true;
        }
        if self.tool == Tool::Select {
            return false;
        }
        self.tool = Tool::Select;
        self.status = "選択ツールに戻りました".into();
        true
    }
}
